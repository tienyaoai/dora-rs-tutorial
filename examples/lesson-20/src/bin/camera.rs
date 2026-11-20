//! 影像來源節點。box 沒有實體攝影機，所以有兩種模擬來源（env SOURCE）：
//!   synth  ：程式內即時合成（背景＋移動的 MNIST 數字），WIDTH×HEIGHT 灰階
//!   ffmpeg ：開一個 ffmpeg 子行程，把 FFMPEG_INPUT 指定的來源解碼成灰階 rawvideo，從 stdout 一張一張讀。
//!            FFMPEG_INPUT="-i clip.mp4" 就是讀影片檔；換成真攝影機只要改成
//!            FFMPEG_INPUT="-f v4l2 -video_size 640x480 -i /dev/video0"（本篇沒有攝影機，沒實測）
//! 每個 tick 送一張（Arrow UInt8，WIDTH*HEIGHT byte）。metadata：seq、t_ns（擷取時間）、width、height、
//! encoding="mono8"，以及 label（正確答案，只為了讓 sink 算正確率；真的攝影機沒有，送 -1）。
//! 送完 FRAMES 張（或影片讀完）就結束，下游的輸入跟著關閉，整個 dataflow 自然結束。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_19::{Mnist, Synth, env_or, now_ns};
use std::io::{BufRead, Read};
use std::process::{Child, ChildStdout, Command, Stdio};

enum Source { Synth(Synth), Ffmpeg { child: Child, out: ChildStdout, labels: Vec<i64> } }

fn main() -> eyre::Result<()> {
    let w: usize = env_or("WIDTH", 320);
    let h: usize = env_or("HEIGHT", 240);
    let frames: u64 = env_or("FRAMES", 600);
    let kind: String = env_or("SOURCE", "synth".to_string());
    let mut src = match kind.as_str() {
        "synth" => {
            let ds = Mnist::load(&env_or("DATASET", "../lesson-17/data".to_string()), "t10k")?;
            Source::Synth(Synth::new(ds, w, h, env_or("SCALE", 4), env_or("HOLD", 15)))
        }
        "ffmpeg" => {
            let input: String = env_or("FFMPEG_INPUT", "-i clip.mp4".to_string());
            let mut cmd = Command::new("ffmpeg");
            cmd.args(["-hide_banner", "-loglevel", "error"]).args(input.split_whitespace())
               .args(["-vf", &format!("scale={w}:{h}"), "-pix_fmt", "gray", "-f", "rawvideo", "-"])
               .stdout(Stdio::piped()).stdin(Stdio::null());
            let mut child = cmd.spawn()?;
            let out = child.stdout.take().unwrap();
            // 影片檔旁邊的答案檔（每行一個數字）；沒有就全部 -1
            let labels = match std::fs::File::open(env_or("LABELS", "clip.labels".to_string())) {
                Ok(f) => std::io::BufReader::new(f).lines().map(|l| l.unwrap().trim().parse().unwrap_or(-1)).collect(),
                Err(_) => vec![],
            };
            println!("[camera] ffmpeg 來源：{input}（答案 {} 行）", labels.len());
            Source::Ffmpeg { child, out, labels }
        }
        other => eyre::bail!("不認得的 SOURCE={other}"),
    };

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out_id = DataId::from("image".to_owned());
    let mut buf = vec![0u8; w * h];
    let mut seq = 0u64;
    println!("[camera] SOURCE={kind} {w}x{h} FRAMES={frames} zero_copy_threshold={}", node.zero_copy_threshold());
    while seq < frames {
        let Some(ev) = events.recv() else { break };
        match ev {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                // 擷取：synth 是畫一張；ffmpeg 是從管線讀滿一張（read_exact）
                let label: i64 = match &mut src {
                    Source::Synth(s) => s.render(seq, &mut buf).0 as i64,
                    Source::Ffmpeg { out, labels, .. } => {
                        if out.read_exact(&mut buf).is_err() {
                            // 第一張就讀不到＝來源根本打不開（檔案不存在、沒有 /dev/video0…），當成錯誤
                            eyre::ensure!(seq > 0, "ffmpeg 一張都沒讀到，來源打不開？（看上面 ffmpeg 的錯誤訊息）");
                            println!("[camera] 影片讀完"); break;
                        }
                        labels.get(seq as usize).copied().unwrap_or(-1)
                    }
                };
                let t_ns = now_ns(); // 擷取完成的時間；端到端延遲從這裡開始算
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq as i64));
                p.insert("t_ns".into(), Parameter::Integer(t_ns as i64));
                p.insert("width".into(), Parameter::Integer(w as i64));
                p.insert("height".into(), Parameter::Integer(h as i64));
                p.insert("encoding".into(), Parameter::String("mono8".into()));
                p.insert("label".into(), Parameter::Integer(label));
                // 直接寫進 dora 配好的輸出緩衝（大於門檻時是共享記憶體），不另外配置 Vec
                node.send_output_raw(out_id.clone(), p, w * h, |dst| dst.copy_from_slice(&buf))?;
                seq += 1;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    if let Source::Ffmpeg { mut child, .. } = src { let _ = child.kill(); let _ = child.wait(); }
    println!("[camera] 結束：送出 {seq} 張");
    Ok(())
}
