//! 離線量測（不是 dora 節點）：同一份 MNIST 測試集，比較 candle 與 ONNX Runtime 的
//! 正確率、載入時間、不同批次大小的每次呼叫延遲。執行緒數請用環境變數設定：
//!   candle：RAYON_NUM_THREADS=N（candle 的 CPU kernel 讀這個變數）
//!   ort   ：ORT_THREADS=N（程式內呼叫 with_intra_threads）
//! 用法：bench candle|ort
use candle_core::{D, Device, Module};
use lesson_17::{Lat, Mnist, Net, PIXELS, env_or, preprocess};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let backend = std::env::args().nth(1).unwrap_or("candle".into());
    let test = Mnist::load("data", "t10k")?;
    match backend.as_str() {
        "candle" => candle(&test),
        #[cfg(feature = "onnx")]
        "ort" => ort_bench(&test),
        b => eyre::bail!("不認得的 backend：{b}"),
    }
}

fn candle(test: &Mnist) -> eyre::Result<()> {
    let dev = Device::Cpu;
    let t = Instant::now();
    let net = Net::load("models/mnist-cnn.safetensors", &dev)?;
    println!("[bench candle] RAYON_NUM_THREADS={} 載入 {:.2} ms",
        std::env::var("RAYON_NUM_THREADS").unwrap_or("(未設)".into()), t.elapsed().as_secs_f64() * 1e3);
    // 正確率：整個測試集，batch 100
    let mut correct = 0;
    for s in (0..test.len()).step_by(100) {
        let p: Vec<u32> = net.forward(&preprocess(&test.images[s * PIXELS..(s + 100) * PIXELS], &dev)?)?
            .argmax(D::Minus1)?.to_vec1()?;
        correct += p.iter().zip(&test.labels[s..s + 100]).filter(|(a, b)| **a == **b as u32).count();
    }
    println!("[bench candle] 測試集 10000 張正確 {correct}（{:.2}%）", correct as f64 / 100.0);
    for bs in [1usize, 8, 32] {
        let mut lat = Lat::default();
        let calls = 2000 / bs;
        for k in 0..calls + 20 {
            let s = (k * bs) % (test.len() - bs);
            let t = Instant::now();
            let _p: Vec<u32> = net.forward(&preprocess(&test.images[s * PIXELS..(s + bs) * PIXELS], &dev)?)?
                .argmax(D::Minus1)?.to_vec1()?;
            if k >= 20 { lat.push(t.elapsed().as_nanos() as u64); } // 前 20 次當暖機
        }
        let p50 = { lat.0.sort_unstable(); lat.0[lat.0.len() / 2] as f64 / 1e6 };
        println!("[bench candle] batch {bs:>2}：每次呼叫 {}，每張約 {:.3} ms（{} 次）", lat.summary(), p50 / bs as f64, calls);
    }
    Ok(())
}

#[cfg(feature = "onnx")]
fn ort_bench(test: &Mnist) -> eyre::Result<()> {
    use ort::{session::Session, value::TensorRef};
    let threads: usize = env_or("ORT_THREADS", 1);
    let t = Instant::now();
    let env = ort::init().with_name("bench").build().map_err(|e| eyre::eyre!("{e}"))?;
    let mut sess = Session::builder(&env).map_err(|e| eyre::eyre!("{e}"))?
        .with_intra_threads(threads).map_err(|e| eyre::eyre!("{e}"))?
        .commit_from_file("models/mnist-12.onnx").map_err(|e| eyre::eyre!("{e}"))?;
    println!("[bench ort] ORT_THREADS={threads} 載入 {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    for i in sess.inputs() { println!("[bench ort] 輸入 {} {:?}", i.name(), i.dtype()); }
    for o in sess.outputs() { println!("[bench ort] 輸出 {} {:?}", o.name(), o.dtype()); }
    // mnist-12 的輸入是 (1,1,28,28) 固定 batch 1。像素要不要 /255？兩種都量。
    for (scale, name) in [(1.0f32, "像素 0..255"), (1.0 / 255.0, "像素 /255")] {
        let mut correct = 0;
        let mut lat = Lat::default();
        let mut x = vec![0f32; PIXELS];
        for i in 0..test.len() {
            for (d, s) in x.iter_mut().zip(test.image(i)) { *d = *s as f32 * scale; }
            let t = Instant::now();
            let out = sess.run(ort::inputs![TensorRef::from_array_view(([1usize, 1, 28, 28], &*x))?])?;
            let (_shape, logits) = out[0].try_extract_tensor::<f32>()?;
            let (p, _) = lesson_17::top1(logits);
            lat.push(t.elapsed().as_nanos() as u64);
            if p == test.labels[i] as usize { correct += 1; }
        }
        println!("[bench ort] {name}：正確 {correct}（{:.2}%），batch 1 每次呼叫 {}", correct as f64 / 100.0, lat.summary());
    }
    Ok(())
}
