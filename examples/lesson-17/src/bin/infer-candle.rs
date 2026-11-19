//! candle 推論節點：image（UInt8 784）→ result（Float32 10 個機率）＋ metadata。
//!
//! 設定（dataflow.yml 的 env）：
//!   MODEL        safetensors 路徑
//!   BATCH        一次最多推論幾張（只拿「已經在佇列裡」的，不為了湊批次而等待）。預設 1
//!   RAYON_NUM_THREADS  candle CPU kernel 的執行緒數（candle 自己讀這個變數）
//!   MAX_AGE_MS   影像擷取後超過這麼久才輪到，就直接丟掉不推論（0＝不丟）
//!   SLOW_MS      ⚠️ 示範用：每次呼叫模型後額外 sleep，模擬「比較重的模型」。正式節點不要設
use candle_core::{Device, Module};
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId,
                    get_integer_param};
use lesson_17::{Lat, Net, PIXELS, env_or, now_ns, preprocess, top1};
use std::time::{Duration, Instant};

struct Frame { seq: i64, label: i64, t_ns: i64, pixels: Vec<u8> }

fn main() -> eyre::Result<()> {
    let model: String = env_or("MODEL", "models/mnist-cnn.safetensors".to_string());
    let batch: usize = env_or("BATCH", 1usize).max(1);
    let max_age_ns: u64 = env_or("MAX_AGE_MS", 0u64) * 1_000_000;
    let slow = Duration::from_millis(env_or("SLOW_MS", 0u64));

    // ① 模型載入與暖機放在事件迴圈「之前」：慢的事一次做完，失敗就讓節點直接結束（main 回傳 Err）。
    let t = Instant::now();
    let dev = Device::Cpu;
    let net = Net::load(&model, &dev)?;
    let load_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let _ = net.forward(&preprocess(&vec![0u8; PIXELS * batch], &dev)?)?.to_vec2::<f32>()?; // 暖機
    println!("[candle] 模型 {model} 載入 {load_ms:.2} ms、暖機 {:.2} ms；BATCH={batch} RAYON_NUM_THREADS={} MAX_AGE_MS={} SLOW_MS={}",
        t.elapsed().as_secs_f64() * 1e3, std::env::var("RAYON_NUM_THREADS").unwrap_or("(未設)".into()),
        max_age_ns / 1_000_000, slow.as_millis());

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("result".to_owned());
    let (mut n_frames, mut n_calls, mut n_stale, mut n_queue_drop) = (0u64, 0u64, 0u64, 0u64);
    let mut infer_lat = Lat::default();
    let mut pending: Vec<Frame> = Vec::with_capacity(batch);
    let mut stop = false;

    let take = |event: Event, pending: &mut Vec<Frame>, stop: &mut bool| -> eyre::Result<()> {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "image" => {
                let px: &[u8] = (&data).try_into()?; // ② Arrow UInt8Array → &[u8]（不複製）
                eyre::ensure!(px.len() == PIXELS, "影像大小 {} ≠ {PIXELS}", px.len());
                let pm = &metadata.parameters;
                pending.push(Frame {
                    seq: get_integer_param(pm, "seq").unwrap_or(-1),
                    label: get_integer_param(pm, "label").unwrap_or(-1),
                    t_ns: get_integer_param(pm, "t_ns").unwrap_or(0),
                    pixels: px.to_vec(), // 要湊批次就得留住資料，這裡複製 784 byte
                });
            }
            Event::Stop(_) => *stop = true,
            _ => {}
        }
        Ok(())
    };

    while !stop {
        let Some(ev) = events.recv() else { break };
        take(ev, &mut pending, &mut stop)?;
        // ③ 「順手批次」：把已經排在佇列裡的也拿出來，最多 BATCH 張；佇列空了就不等。
        while pending.len() < batch && !stop {
            match events.try_recv() {
                Ok(ev) => take(ev, &mut pending, &mut stop)?,
                Err(_) => break,
            }
        }
        for (_, n) in events.drain_drop_counts() { n_queue_drop += n; } // dora 佇列滿了丟掉的
        // ④ 太舊的影像直接丟（追不上時，寧可跳幀也不要延遲越積越大）
        if max_age_ns > 0 {
            let now = now_ns();
            let before = pending.len();
            pending.retain(|f| now.saturating_sub(f.t_ns as u64) <= max_age_ns);
            n_stale += (before - pending.len()) as u64;
        }
        if pending.is_empty() { continue; }

        // ⑤ 推論：(B,1,28,28) → logits (B,10)
        let t = Instant::now();
        let px: Vec<u8> = pending.iter().flat_map(|f| f.pixels.iter().copied()).collect();
        let logits: Vec<Vec<f32>> = net.forward(&preprocess(&px, &dev)?)?.to_vec2()?;
        if !slow.is_zero() { std::thread::sleep(slow); }
        let infer_ns = t.elapsed().as_nanos() as u64;
        infer_lat.push(infer_ns);
        n_calls += 1;

        // ⑥ 每張圖各送一筆結果，metadata 帶上原本的 seq/t_ns（下游才能對齊、算延遲）與推論資訊
        for (f, l) in pending.iter().zip(&logits) {
            let (pred, score) = top1(l);
            let m = l.iter().cloned().fold(f32::MIN, f32::max);
            let s: f32 = l.iter().map(|v| (v - m).exp()).sum();
            let probs: Vec<f32> = l.iter().map(|v| (v - m).exp() / s).collect();
            let mut p = MetadataParameters::new();
            p.insert("seq".into(), Parameter::Integer(f.seq));
            p.insert("label".into(), Parameter::Integer(f.label));
            p.insert("t_ns".into(), Parameter::Integer(f.t_ns));
            p.insert("pred".into(), Parameter::Integer(pred as i64));
            p.insert("score".into(), Parameter::Float(score as f64));
            p.insert("infer_us".into(), Parameter::Integer((infer_ns / 1000) as i64));
            p.insert("batch".into(), Parameter::Integer(pending.len() as i64));
            p.insert("backend".into(), Parameter::String("candle".into()));
            node.send_output(out.clone(), p, probs)?;
        }
        n_frames += pending.len() as u64;
        pending.clear();
    }
    println!("[candle] 結束：推論 {n_frames} 張／{n_calls} 次呼叫（平均每批 {:.2} 張），每次呼叫 {}；過期丟棄 {n_stale}，dora 佇列丟棄 {n_queue_drop}",
        n_frames as f64 / n_calls.max(1) as f64, infer_lat.summary());
    Ok(())
}
