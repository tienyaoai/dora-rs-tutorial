//! 結果輸出節點（第 20 篇會換成顯示節點）：
//!   文字：前 PRINT 筆印到 log；每筆寫一行到 RESULT_DIR/results.csv
//!   存圖：每 SAVE_EVERY 筆，把對應的原始畫面畫上外框，存成 RESULT_DIR/frame_<seq>_pred<d>.png
//!   統計：正確率、序號缺口（被丟的幀）、端到端延遲與各段延遲的 p50/p99/max（前 WARMUP 筆不計）
//! 輸入：result（detect/result）、image（camera/image，只為了存圖；小佇列，舊的丟掉沒關係）
use dora_node_api::{DoraNode, Event, Parameter, get_integer_param};
use lesson_19::{BBox, draw_box, env_or, now_ns, save_png, summary_us};
use std::collections::VecDeque;
use std::io::Write;

fn main() -> eyre::Result<()> {
    let out_dir: String = env_or("RESULT_DIR", "run/out".to_string());
    let save_every: i64 = env_or("SAVE_EVERY", 0);
    let warmup: u64 = env_or("WARMUP", 30);
    let print_n: u64 = env_or("PRINT", 3);
    std::fs::create_dir_all(&out_dir)?;
    let mut csv = std::io::BufWriter::new(std::fs::File::create(format!("{out_dir}/results.csv"))?);
    writeln!(csv, "seq,label,pred,score,found,bx,by,bw,bh,e2e_us")?;

    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut frames: VecDeque<(i64, usize, usize, Vec<u8>)> = VecDeque::new(); // 最近幾張原始畫面
    let (mut n, mut correct, mut not_found, mut gaps, mut last_seq, mut saved, mut no_frame) = (0u64, 0u64, 0u64, 0u64, -1i64, 0u64, 0u64);
    let (mut n_img, mut n_labeled) = (0u64, 0u64);
    let (mut e2e, mut s_cam, mut s_det, mut s_out) = (vec![], vec![], vec![], vec![]);
    while let Some(ev) = events.recv() {
        let Event::Input { id, metadata, data } = ev else {
            if matches!(ev, Event::Stop(_)) { break; }
            continue;
        };
        let arrive = now_ns() as i64;
        let pm = &metadata.parameters;
        let gi = |k: &str| get_integer_param(pm, k).unwrap_or(-1);
        match id.as_str() {
            "image" if save_every > 0 => {
                let px: &[u8] = (&data).try_into()?;
                let seq = gi("seq");
                if seq % save_every == 0 { // 只留之後要存的那幾張，其他的不複製
                    frames.push_back((seq, gi("width") as usize, gi("height") as usize, px.to_vec()));
                    while frames.len() > 4 { frames.pop_front(); }
                }
                n_img += 1;
            }
            "result" => {
                let probs: &[f32] = (&data).try_into()?;
                let (seq, label, pred, found) = (gi("seq"), gi("label"), gi("pred"), gi("found") == 1);
                let score = match pm.get("score") { Some(Parameter::Float(f)) => *f, _ => f64::NAN };
                let b = BBox { x: gi("bx") as usize, y: gi("by") as usize, w: gi("bw") as usize, h: gi("bh") as usize };
                let (t_ns, t_in, t_out) = (gi("t_ns"), gi("t_in_ns"), gi("t_out_ns"));
                if last_seq >= 0 && seq > last_seq + 1 { gaps += (seq - last_seq - 1) as u64; }
                last_seq = seq;
                n += 1;
                if !found { not_found += 1; }
                if label >= 0 { n_labeled += 1; if pred == label { correct += 1; } }
                let e = ((arrive - t_ns) / 1000) as u64;
                if n > warmup {
                    e2e.push(e);
                    s_cam.push(((t_in - t_ns) / 1000) as u64);
                    s_det.push(((t_out - t_in) / 1000) as u64);
                    s_out.push(((arrive - t_out) / 1000) as u64);
                }
                writeln!(csv, "{seq},{label},{pred},{score:.4},{},{},{},{},{},{e}", found as u8, b.x, b.y, b.w, b.h)?;
                if n <= print_n {
                    println!("[sink] 第 {n} 筆：seq={seq} label={label} pred={pred} score={score:.3} 外框=({},{},{}x{}) 機率 {} 個 f32 端到端 {:.2} ms",
                        b.x, b.y, b.w, b.h, probs.len(), e as f64 / 1e3);
                }
                if save_every > 0 && seq % save_every == 0 {
                    if let Some(pos) = frames.iter().position(|f| f.0 == seq) {
                        let (_, w, h, mut img) = frames.remove(pos).unwrap();
                        if found { draw_box(&mut img, w, h, b); }
                        save_png(&format!("{out_dir}/frame_{seq:05}_label{label}_pred{pred}.png"), &img, w, h)?;
                        saved += 1;
                    } else { no_frame += 1; }
                }
            }
            _ => {}
        }
    }
    csv.flush()?;
    println!("[sink] 結果 {n} 筆（正確 {correct}/{n_labeled} = {:.2}%，找不到數字 {not_found}），序號缺口 {gaps}；收到原始畫面 {n_img} 張，存圖 {saved} 張（找不到對應畫面 {no_frame}）",
        correct as f64 * 100.0 / n_labeled.max(1) as f64);
    println!("[sink] 延遲（前 {warmup} 筆不計，共 {} 筆）端到端 {}", e2e.len(), summary_us(&mut e2e));
    println!("[sink]   camera→detect {}；detect 內部 {}；detect→sink {}", summary_us(&mut s_cam), summary_us(&mut s_det), summary_us(&mut s_out));
    Ok(())
}
