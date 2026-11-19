//! 第 17 篇共用程式：MNIST 讀檔、candle 版的小型 CNN、前處理、統計小工具。
//! 推論節點（infer-candle）、訓練程式（train）、離線量測（bench）都用同一份 `Net` 與 `preprocess`，
//! 確保「訓練時怎麼前處理，推論時就怎麼前處理」。

use candle_core::{DType, Device, Module, Result as CResult, Tensor};
use candle_nn::{Conv2d, Linear, VarBuilder, conv2d, linear};
use std::time::{SystemTime, UNIX_EPOCH};

pub const W: usize = 28;
pub const H: usize = 28;
pub const PIXELS: usize = W * H;
pub const CLASSES: usize = 10;

pub fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

/// MNIST 的 idx 檔（從 ossci-datasets 下載、gunzip 後的原始格式）。
pub struct Mnist {
    pub images: Vec<u8>, // n × 784，0..=255
    pub labels: Vec<u8>,
}

impl Mnist {
    /// `split` = "train" 或 "t10k"
    pub fn load(dir: &str, split: &str) -> eyre::Result<Self> {
        let img = std::fs::read(format!("{dir}/{split}-images-idx3-ubyte"))?;
        let lab = std::fs::read(format!("{dir}/{split}-labels-idx1-ubyte"))?;
        let be = |b: &[u8], o: usize| u32::from_be_bytes(b[o..o + 4].try_into().unwrap()) as usize;
        eyre::ensure!(be(&img, 0) == 2051 && be(&lab, 0) == 2049, "不是 MNIST idx 檔（magic 不對）");
        let n = be(&img, 4);
        eyre::ensure!(be(&img, 8) == H && be(&img, 12) == W && be(&lab, 4) == n, "尺寸不符");
        Ok(Self { images: img[16..].to_vec(), labels: lab[8..].to_vec() })
    }
    pub fn len(&self) -> usize { self.labels.len() }
    pub fn is_empty(&self) -> bool { self.labels.is_empty() }
    pub fn image(&self, i: usize) -> &[u8] { &self.images[i * PIXELS..(i + 1) * PIXELS] }
}

/// 小型 CNN：conv5×5(16) → pool → conv5×5(32) → pool → fc64 → fc10。共 46,730 個參數（train 實際印出）。
pub struct Net { c1: Conv2d, c2: Conv2d, f1: Linear, f2: Linear }

impl Net {
    pub fn new(vb: VarBuilder) -> CResult<Self> {
        Ok(Self {
            c1: conv2d(1, 16, 5, Default::default(), vb.pp("c1"))?,
            c2: conv2d(16, 32, 5, Default::default(), vb.pp("c2"))?,
            f1: linear(32 * 4 * 4, 64, vb.pp("f1"))?,
            f2: linear(64, CLASSES, vb.pp("f2"))?,
        })
    }
    /// 從 safetensors 載入（推論節點用）。mmap 讀檔，模型檔不會整個複製一份到 heap。
    pub fn load(path: &str, dev: &Device) -> CResult<Self> {
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[path], DType::F32, dev)? };
        Self::new(vb)
    }
}

impl Module for Net {
    /// xs: (B, 1, 28, 28) f32，數值 0..1。輸出 logits (B, 10)。
    fn forward(&self, xs: &Tensor) -> CResult<Tensor> {
        xs.apply(&self.c1)?.relu()?.max_pool2d(2)?
            .apply(&self.c2)?.relu()?.max_pool2d(2)?
            .flatten_from(1)?
            .apply(&self.f1)?.relu()?
            .apply(&self.f2)
    }
}

/// 前處理：B 張 u8 灰階圖（連續排在一起）→ (B,1,28,28) f32 / 255。
/// 這裡會複製一次（u8 → candle 自己的 storage），再做一次型別轉換。
pub fn preprocess(pixels: &[u8], dev: &Device) -> CResult<Tensor> {
    let b = pixels.len() / PIXELS;
    Tensor::from_slice(pixels, (b, 1, H, W), dev)?.to_dtype(DType::F32)? / 255.0
}

/// 延遲統計（單位 ns），印 p50/p99/max。
#[derive(Default)]
pub struct Lat(pub Vec<u64>);
impl Lat {
    pub fn push(&mut self, ns: u64) { self.0.push(ns) }
    pub fn summary(&mut self) -> String {
        if self.0.is_empty() { return "—".into(); }
        self.0.sort_unstable();
        let q = |f: f64| self.0[((self.0.len() - 1) as f64 * f) as usize] as f64 / 1e6;
        format!("p50 {:.2} ms / p99 {:.2} ms / max {:.2} ms", q(0.5), q(0.99), q(1.0))
    }
}

pub fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// argmax 與 softmax 機率（logits 一列）
pub fn top1(logits: &[f32]) -> (usize, f32) {
    let m = logits.iter().cloned().fold(f32::MIN, f32::max);
    let sum: f32 = logits.iter().map(|v| (v - m).exp()).sum();
    let (i, v) = logits.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
    (i, (v - m).exp() / sum)
}
