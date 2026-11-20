//! 辨識節點：image（mono8，W×H）→ 找數字外框 → 裁切成 28×28 → 第 17 篇的 candle CNN → result。
//! result 的資料是 10 個 Float32 機率；metadata 原樣帶上 seq、t_ns、label，再加：
//!   pred、score、found（有沒有找到數字）、bx/by/bw/bh（外框）、
//!   t_in_ns（本節點收到的時間）、t_out_ns（送出前的時間）、pre_us（找框＋裁切）、infer_us（模型）
//! env：MODEL（safetensors）、THRESHOLD（二值化門檻，預設 128）、SLOW_MS（⚠️示範用，模擬重模型）
use candle_core::{Device, Module};
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId,
                    get_integer_param};
use lesson_19::{CLASSES, Lat, Net, PIXELS, env_or, locate, now_ns, preprocess, to_mnist, top1};
use std::time::{Duration, Instant};

fn main() -> eyre::Result<()> {
    let model: String = env_or("MODEL", "../lesson-17/models/mnist-cnn.safetensors".to_string());
    let thr: u8 = env_or("THRESHOLD", 128);
    let slow = Duration::from_millis(env_or("SLOW_MS", 0u64));
    // ① 載入與暖機在事件迴圈之前（第 17 篇的做法）
    let dev = Device::Cpu;
    let t = Instant::now();
    let net = Net::load(&model, &dev)?;
    let _ = net.forward(&preprocess(&[0u8; PIXELS], &dev)?)?.to_vec2::<f32>()?;
    println!("[detect] 模型 {model} 載入＋暖機 {:.2} ms；THRESHOLD={thr} SLOW_MS={}", t.elapsed().as_secs_f64() * 1e3, slow.as_millis());

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("result".to_owned());
    let (mut n, mut n_found, mut n_drop) = (0u64, 0u64, 0u64);
    let (mut pre_lat, mut inf_lat) = (Lat::default(), Lat::default());
    let mut crop = [0u8; PIXELS];
    while let Some(ev) = events.recv() {
        let Event::Input { id, metadata, data } = ev else {
            if matches!(ev, Event::Stop(_)) { break; }
            continue;
        };
        if id.as_str() != "image" { continue; }
        let t_in_ns = now_ns();
        for (_, d) in events.drain_drop_counts() { n_drop += d; }
        let pm = &metadata.parameters;
        let w = get_integer_param(pm, "width").unwrap_or(0) as usize;
        let h = get_integer_param(pm, "height").unwrap_or(0) as usize;
        let img: &[u8] = (&data).try_into()?; // ② 不複製，直接讀共享記憶體
        eyre::ensure!(img.len() == w * h && w > 0, "影像大小 {} ≠ {w}x{h}", img.len());

        // ③ 前處理：找框 → 裁切縮放成 MNIST 格式
        let t = Instant::now();
        let bbox = locate(img, w, h, thr);
        if let Some(b) = bbox { to_mnist(img, w, b, thr, &mut crop); }
        let pre_ns = t.elapsed().as_nanos() as u64;
        pre_lat.push(pre_ns);

        // ④ 推論（找不到數字就不呼叫模型，送全 0 機率、pred=-1）
        let t = Instant::now();
        let (probs, pred, score) = if bbox.is_some() {
            let l = net.forward(&preprocess(&crop, &dev)?)?.to_vec2::<f32>()?.remove(0);
            let (p, s) = top1(&l);
            let m = l.iter().cloned().fold(f32::MIN, f32::max);
            let z: f32 = l.iter().map(|v| (v - m).exp()).sum();
            (l.iter().map(|v| (v - m).exp() / z).collect::<Vec<f32>>(), p as i64, s as f64)
        } else { (vec![0f32; CLASSES], -1, 0.0) };
        if !slow.is_zero() { std::thread::sleep(slow); }
        let inf_ns = t.elapsed().as_nanos() as u64;
        if bbox.is_some() { inf_lat.push(inf_ns); n_found += 1; }

        // ⑤ 送出：沿用上游的 metadata（seq、t_ns、label…），再加自己的欄位
        let mut p: MetadataParameters = metadata.parameters.clone();
        let b = bbox.unwrap_or(lesson_19::BBox { x: 0, y: 0, w: 0, h: 0 });
        for (k, v) in [("pred", pred), ("found", bbox.is_some() as i64), ("bx", b.x as i64), ("by", b.y as i64),
                       ("bw", b.w as i64), ("bh", b.h as i64), ("t_in_ns", t_in_ns as i64),
                       ("pre_us", (pre_ns / 1000) as i64), ("infer_us", (inf_ns / 1000) as i64)] {
            p.insert(k.into(), Parameter::Integer(v));
        }
        p.insert("score".into(), Parameter::Float(score));
        p.insert("t_out_ns".into(), Parameter::Integer(now_ns() as i64));
        node.send_output(out.clone(), p, probs)?;
        n += 1;
    }
    println!("[detect] 結束：處理 {n} 張，找到數字 {n_found} 張；前處理 {}；模型 {}；dora 佇列丟棄 {n_drop}",
        pre_lat.summary(), inf_lat.summary());
    Ok(())
}
