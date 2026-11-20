//! 顯示節點（取代第 19 篇的 sink）：把「原始畫面＋外框＋辨識結果＋延遲」畫成一張 RGB 影格，送到一個或多個輸出。
//! 輸入：image（camera/image，原始灰階畫面）、result（detect/result，10 個機率＋外框 metadata）。
//! 兩者用 metadata 的 seq 配對：image 先到就先放在小緩衝裡，result 到了再找同一個 seq 的畫面來畫。
//! env OUTPUT（逗號分隔，可多選）：
//!   mp4    ：開 ffmpeg 子行程，把 rgb24 影格從 stdin 餵進去，壓成 MP4_PATH（H.264）
//!   png    ：每 PNG_EVERY 張存一張 PNG 到 RESULT_DIR
//!   ascii  ：每 ASCII_EVERY 張在 log 印一張 ASCII_COLS×ASCII_ROWS 的終端預覽
//!   fb     ：寫到 Linux framebuffer（FB_DEV，例如 /dev/fb0；ZYNQ 接 DisplayPort／HDMI 時常見）。
//!            是 /dev/fb* 時從 sysfs 讀解析度、bpp、stride；否則（測試用一般檔案）用 FB_W、FB_H、FB_BPP
//!   window ：minifb 視窗（要 cargo build --features window，需要 X11）
//! 其他：SCALE（放大倍數，預設 2）、FONT（字放大倍數，預設 2）、RESULT_DIR（⚠️不要叫 OUT_DIR，見常見錯誤 2）、WARMUP（前幾筆不計入統計）
use dora_node_api::{DoraNode, Event, Parameter, get_integer_param};
use lesson_20::{BBox, Canvas, GREEN, RED, WHITE, YELLOW, env_or, now_ns, summary_us};
use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Instant;

struct Fb { file: std::fs::File, w: usize, h: usize, bpp: usize, stride: usize, row: Vec<u8> }

impl Fb {
    fn open(dev: &str) -> eyre::Result<Self> {
        let (w, h, bpp, stride) = if let Some(n) = dev.strip_prefix("/dev/fb") {
            // 真的 framebuffer：解析度、每像素位元數、每列 byte 數都從 sysfs 讀
            let rd = |f: &str| std::fs::read_to_string(format!("/sys/class/graphics/fb{n}/{f}"));
            let vs = rd("virtual_size")?; let (a, b) = vs.trim().split_once(',').ok_or_else(|| eyre::eyre!("virtual_size 格式不對"))?;
            let bpp: usize = rd("bits_per_pixel")?.trim().parse()?;
            let stride: usize = rd("stride")?.trim().parse()?;
            (a.parse()?, b.parse()?, bpp, stride)
        } else {
            let (w, h, bpp): (usize, usize, usize) = (env_or("FB_W", 800), env_or("FB_H", 600), env_or("FB_BPP", 32));
            (w, h, bpp, w * bpp / 8)
        };
        eyre::ensure!(bpp == 32 || bpp == 16, "只支援 32 或 16 bpp，這個裝置是 {bpp}");
        let file = std::fs::OpenOptions::new().write(true).create(!dev.starts_with("/dev/")).open(dev)?;
        if !dev.starts_with("/dev/") { file.set_len((stride * h) as u64)?; } // 測試用檔案：先撐到整個畫面的大小
        println!("[display] framebuffer {dev}：{w}x{h} {bpp}bpp stride {stride}");
        Ok(Self { file, w, h, bpp, stride, row: vec![] })
    }
    /// 把畫布貼到左上角（超出的部分裁掉），一列一列 pwrite
    fn show(&mut self, c: &Canvas) -> eyre::Result<()> {
        let cols = c.w.min(self.w);
        self.row.resize(c.w * self.bpp / 8, 0);
        for y in 0..c.h.min(self.h) {
            c.row_to_fb(y, self.bpp, &mut self.row);
            self.file.write_all_at(&self.row[..cols * self.bpp / 8], (y * self.stride) as u64)?;
        }
        Ok(())
    }
}

enum Out {
    Mp4 { child: Child, stdin: Option<ChildStdin>, path: String },
    Png { dir: String, every: i64, saved: u64 },
    Ascii { every: i64, cols: usize, rows: usize },
    Fb(Fb),
    #[cfg(feature = "window")]
    Window { win: minifb::Window, buf: Vec<u32> },
}

fn main() -> eyre::Result<()> {
    let scale: usize = env_or("SCALE", 2);
    let font: usize = env_or("FONT", 2);
    let warmup: u64 = env_or("WARMUP", 30);
    let out_dir: String = env_or("RESULT_DIR", "run/out".to_string());
    let fps_hint: u32 = env_or("FPS", 30);
    let outputs: String = env_or("OUTPUT", "png".to_string());
    std::fs::create_dir_all(&out_dir)?;

    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut outs: Vec<Out> = vec![];
    let mut canvas: Option<Canvas> = None;          // 第一張畫面到了才知道大小
    let mut frames: VecDeque<(i64, usize, usize, Vec<u8>)> = VecDeque::new();
    let (mut shown, mut missing, mut correct, mut labeled, mut wrong_shown) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut e2e_in, mut e2e_shown, mut render, mut emit) = (vec![], vec![], vec![], vec![]);
    let (mut t_first, mut t_last) = (0u64, 0u64);
    let mut last_shown_ns = 0u64;
    let mut fps = 0.0f64;
    let (mut drop_img, mut drop_res, mut first_seq, mut last_seq, mut gaps) = (0u64, 0u64, -1i64, -1i64, 0u64);

    while let Some(ev) = events.recv() {
        let Event::Input { id, metadata, data } = ev else {
            if matches!(ev, Event::Stop(_)) { break; }
            continue;
        };
        let arrive = now_ns();
        for (k, d) in events.drain_drop_counts() { if k.as_str() == "result" { drop_res += d } else { drop_img += d } }
        let pm = &metadata.parameters;
        let gi = |k: &str| get_integer_param(pm, k).unwrap_or(-1);
        match id.as_str() {
            "image" => {
                let px: &[u8] = (&data).try_into()?;
                frames.push_back((gi("seq"), gi("width") as usize, gi("height") as usize, px.to_vec()));
                while frames.len() > 8 { frames.pop_front(); }
            }
            "result" => {
                let seq = gi("seq");
                if first_seq < 0 { first_seq = seq; }
                if last_seq >= 0 && seq > last_seq + 1 { gaps += (seq - last_seq - 1) as u64; }
                last_seq = seq;
                // 找同一個 seq 的畫面；比它舊的畫面不會再用到，一起丟掉
                while frames.front().is_some_and(|f| f.0 < seq) { frames.pop_front(); }
                let Some(f) = frames.pop_front().filter(|f| f.0 == seq) else { missing += 1; continue; };
                let (_, w, h, gray) = f;

                // ---- 第一次：依畫面大小建立畫布與輸出 ----
                if canvas.is_none() {
                    let (cw, ch) = (w * scale, h * scale);
                    canvas = Some(Canvas::new(cw, ch));
                    for o in outputs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        outs.push(match o {
                            "mp4" => {
                                let path: String = env_or("MP4_PATH", format!("{out_dir}/display.mp4"));
                                let mut child = Command::new("ffmpeg")
                                    .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
                                           "-s", &format!("{cw}x{ch}"), "-r", &fps_hint.to_string(), "-i", "-",
                                           "-c:v", "libx264", "-preset", "veryfast", "-crf", &env_or("MP4_CRF", 28).to_string(), "-pix_fmt", "yuv420p", &path])
                                    .stdin(Stdio::piped()).spawn()?;
                                let stdin = child.stdin.take();
                                Out::Mp4 { child, stdin, path }
                            }
                            "png" => Out::Png { dir: out_dir.clone(), every: env_or("PNG_EVERY", 100), saved: 0 },
                            "ascii" => Out::Ascii { every: env_or("ASCII_EVERY", 150), cols: env_or("ASCII_COLS", 64), rows: env_or("ASCII_ROWS", 24) },
                            "fb" => Out::Fb(Fb::open(&env_or("FB_DEV", "/dev/fb0".to_string()))?),
                            #[cfg(feature = "window")]
                            "window" => {
                                let mut win = minifb::Window::new("dora-rs lesson 20", cw, ch, minifb::WindowOptions::default())?;
                                win.set_target_fps(0); // 不讓 minifb 自己睡覺限速，節奏由 camera 決定
                                Out::Window { win, buf: vec![0; cw * ch] }
                            }
                            other => eyre::bail!("不認得的 OUTPUT={other}（window 要用 --features window 編譯）"),
                        });
                    }
                    println!("[display] 畫布 {cw}x{ch}（原始 {w}x{h} ×{scale}），輸出：{outputs}");
                }
                let c = canvas.as_mut().unwrap();

                // ---- 畫：原圖 → 外框 → 上方結果列 → 下方延遲列 ----
                let t0 = Instant::now();
                let (label, pred, found) = (gi("label"), gi("pred"), gi("found") == 1);
                let score = match pm.get("score") { Some(Parameter::Float(v)) => *v, _ => 0.0 };
                if label >= 0 { labeled += 1; if pred == label { correct += 1; } }
                let ok = label < 0 || pred == label;        // 真攝影機沒有答案（label = -1）時一律畫綠框
                let color = if !found { YELLOW } else if ok { GREEN } else { RED };
                c.blit_gray(&gray, w, h, scale);
                if found {
                    let b = BBox { x: gi("bx") as usize * scale, y: gi("by") as usize * scale, w: gi("bw") as usize * scale, h: gi("bh") as usize * scale };
                    c.rect(b, 2, color);
                }
                let bar = 7 * font + 6;
                c.fill(0, 0, c.w, bar, [0, 0, 0], 170);
                let head = if found { format!("#{seq} PRED {pred} {:.1}%", score * 100.0) } else { format!("#{seq} NO DIGIT") };
                let x = c.text(4, 3, font, color, &head);
                if label >= 0 { c.text(x + 6 * font, 3, font, WHITE, &format!("LABEL {label} {}", if ok { "OK" } else { "NG" })); }
                let e2e_ms = (arrive as i64 - gi("t_ns")) as f64 / 1e6;
                let det_ms = (gi("t_out_ns") - gi("t_in_ns")) as f64 / 1e6;
                c.fill(0, c.h - bar, c.w, bar, [0, 0, 0], 170);
                c.text(4, c.h - bar + 3, font, WHITE, &format!("E2E {e2e_ms:.2}MS DET {det_ms:.2}MS FPS {fps:.1}"));
                let t_render = t0.elapsed().as_nanos() as u64;

                // ---- 送到各個輸出 ----
                let t1 = Instant::now();
                for o in outs.iter_mut() {
                    match o {
                        Out::Mp4 { stdin, .. } => { if let Some(s) = stdin { s.write_all(&c.px)?; } }
                        Out::Png { dir, every, saved } => if seq % *every == 0 {
                            c.save_png(&format!("{dir}/frame_{seq:05}_label{label}_pred{pred}.png"))?; *saved += 1;
                        },
                        Out::Ascii { every, cols, rows } => if seq % *every == 0 {
                            println!("[display] ASCII 預覽 seq={seq}（{head}；G=綠框、R=紅框）\n{}", c.ascii(*cols, *rows));
                        },
                        Out::Fb(fb) => fb.show(c)?,
                        #[cfg(feature = "window")]
                        Out::Window { win, buf } => {
                            for (d, p) in buf.iter_mut().zip(c.px.chunks_exact(3)) {
                                *d = (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
                            }
                            if !win.is_open() { println!("[display] 視窗被關掉"); break; }
                            win.update_with_buffer(buf, c.w, c.h)?;
                        }
                    }
                }
                let done = now_ns();
                shown += 1;
                if found && !ok { wrong_shown += 1; }
                // 顯示幀率：用相鄰兩次「顯示完成」的間隔做指數平均
                if last_shown_ns > 0 {
                    let inst = 1e9 / (done - last_shown_ns).max(1) as f64;
                    fps = if fps == 0.0 { inst } else { fps * 0.9 + inst * 0.1 };
                }
                last_shown_ns = done;
                if shown == 1 { t_first = done; }
                t_last = done;
                if shown > warmup {
                    e2e_in.push((arrive as i64 - gi("t_ns")) as u64 / 1000);
                    e2e_shown.push((done as i64 - gi("t_ns")) as u64 / 1000);
                    render.push(t_render / 1000);
                    emit.push(t1.elapsed().as_nanos() as u64 / 1000);
                }
            }
            _ => {}
        }
    }

    // ---- 收尾：關掉 ffmpeg 的 stdin，等它把 mp4 寫完 ----
    for o in outs.iter_mut() {
        match o {
            Out::Mp4 { child, stdin, path } => {
                drop(stdin.take());
                let st = child.wait()?;
                let size = std::fs::metadata(&*path).map(|m| m.len()).unwrap_or(0);
                println!("[display] ffmpeg 結束（{st}），{path} {size} byte");
            }
            Out::Png { saved, .. } => println!("[display] 存了 {saved} 張 PNG"),
            _ => {}
        }
    }
    println!("[display] result 序號 {first_seq}..={last_seq}，缺口 {gaps}；dora 佇列丟棄 result {drop_res}、image {drop_img}");
    let secs = (t_last.saturating_sub(t_first)) as f64 / 1e9;
    println!("[display] 顯示 {shown} 張（找不到對應畫面 {missing}），正確 {correct}/{labeled} = {:.2}%，紅框 {wrong_shown} 張；平均顯示幀率 {:.2} 張/秒",
        correct as f64 * 100.0 / labeled.max(1) as f64, if secs > 0.0 { (shown - 1) as f64 / secs } else { 0.0 });
    println!("[display] 延遲（前 {warmup} 筆不計，共 {} 筆）擷取→display 收到 {}", e2e_in.len(), summary_us(&mut e2e_in));
    println!("[display]   擷取→畫面送出 {}；畫圖 {}；輸出 {}", summary_us(&mut e2e_shown), summary_us(&mut render), summary_us(&mut emit));
    Ok(())
}
