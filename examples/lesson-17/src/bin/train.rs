//! 離線訓練（不是 dora 節點）：用 candle 在 CPU 上訓練 lib.rs 的小 CNN，存成 safetensors。
//! 用法：cargo run --release --bin train -- [epochs]   → models/mnist-cnn.safetensors
use candle_core::{DType, Device, Tensor, D};
use candle_nn::{AdamW, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap, loss};
use lesson_17::{Mnist, Net, PIXELS, preprocess};
use std::time::Instant;

fn accuracy(net: &Net, ds: &Mnist, dev: &Device) -> eyre::Result<f64> {
    let mut correct = 0usize;
    for start in (0..ds.len()).step_by(1000) {
        let end = (start + 1000).min(ds.len());
        let x = preprocess(&ds.images[start * PIXELS..end * PIXELS], dev)?;
        let pred: Vec<u32> = net.forward(&x)?.argmax(D::Minus1)?.to_vec1()?;
        correct += pred.iter().zip(&ds.labels[start..end]).filter(|(p, l)| **p == **l as u32).count();
    }
    Ok(correct as f64 / ds.len() as f64)
}

fn main() -> eyre::Result<()> {
    let epochs: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let dev = Device::Cpu;
    let train = Mnist::load("data", "train")?;
    let test = Mnist::load("data", "t10k")?;
    let varmap = VarMap::new();
    let net = Net::new(VarBuilder::from_varmap(&varmap, DType::F32, &dev))?;
    let mut opt = AdamW::new(varmap.all_vars(), ParamsAdamW { lr: 1e-3, ..Default::default() })?;
    let n_params: usize = varmap.all_vars().iter().map(|v| v.elem_count()).sum();
    println!("[train] 訓練 {} 張、測試 {} 張，參數 {n_params} 個", train.len(), test.len());

    let bs = 64;
    let mut idx: Vec<usize> = (0..train.len()).collect();
    let mut rng = 0x2545F4914F6CDD1Du64; // xorshift，固定種子洗牌（權重初始化仍是 candle 的亂數，不固定）
    let t0 = Instant::now();
    for ep in 1..=epochs {
        for i in (1..idx.len()).rev() {
            rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
            idx.swap(i, (rng % (i as u64 + 1)) as usize);
        }
        let mut sum_loss = 0f32;
        let mut buf = Vec::with_capacity(bs * PIXELS);
        let mut lab = Vec::with_capacity(bs);
        for chunk in idx.chunks(bs) {
            buf.clear(); lab.clear();
            for &i in chunk { buf.extend_from_slice(train.image(i)); lab.push(train.labels[i] as u32); }
            let x = preprocess(&buf, &dev)?;
            let y = Tensor::new(lab.as_slice(), &dev)?;
            let l = loss::cross_entropy(&net.forward(&x)?, &y)?;
            opt.backward_step(&l)?;
            sum_loss += l.to_scalar::<f32>()?;
        }
        let acc = accuracy(&net, &test, &dev)?;
        println!("[train] epoch {ep}: 平均 loss {:.4}，測試集正確率 {:.2}%，累計 {:.1} s",
            sum_loss / (idx.len() as f32 / bs as f32), acc * 100.0, t0.elapsed().as_secs_f64());
    }
    std::fs::create_dir_all("models")?;
    varmap.save("models/mnist-cnn.safetensors")?;
    println!("[train] 已存 models/mnist-cnn.safetensors");
    Ok(())
}
