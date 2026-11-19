//! 第 19 篇共用程式：合成影像、找數字（二值化＋外框）、裁切縮放成 MNIST 格式、畫框、統計。
//! 模型、MNIST 讀檔、前處理、top1 直接沿用第 17 篇（lesson_17）。
pub use lesson_17::{CLASSES, H as MH, Lat, Mnist, Net, PIXELS, W as MW, env_or, now_ns, preprocess, top1};

/// 外框（像素座標，含 x、y，寬 w、高 h）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox { pub x: usize, pub y: usize, pub w: usize, pub h: usize }

/// 合成「攝影機畫面」：灰階背景（漸層＋雜訊）上放一個放大的 MNIST 數字，數字會慢慢移動。
/// 每 HOLD 張換一個數字。回傳畫面、正確答案、數字實際所在的外框（只給驗證用）。
pub struct Synth { pub ds: Mnist, pub w: usize, pub h: usize, pub scale: usize, pub hold: u64, rng: u64 }

impl Synth {
    pub fn new(ds: Mnist, w: usize, h: usize, scale: usize, hold: u64) -> Self {
        assert!(w > MW * scale && h > MH * scale, "畫面太小，放不下 {scale} 倍的數字");
        Self { ds, w, h, scale, hold: hold.max(1), rng: 0x9E37_79B9_7F4A_7C15 }
    }
    fn rand(&mut self) -> u64 { // xorshift64：每次執行結果一樣，方便重現
        self.rng ^= self.rng << 13; self.rng ^= self.rng >> 7; self.rng ^= self.rng << 17; self.rng
    }
    /// 把第 seq 張畫到 dst（長度 w*h）
    pub fn render(&mut self, seq: u64, dst: &mut [u8]) -> (u8, BBox) {
        let (w, h, s) = (self.w, self.h, self.scale);
        let idx = ((seq / self.hold) as usize * 7919) % self.ds.len(); // 跳著取，避免連續幾張都同一類
        let label = self.ds.labels[idx];
        // 背景：左上暗、右下亮的漸層（0..40）＋雜訊（0..31），最亮約 71，遠低於找數字的門檻 128
        for y in 0..h {
            for x in 0..w {
                let g = ((x + y) * 40 / (w + h)) as u8;
                dst[y * w + x] = g + (self.rand() & 31) as u8;
            }
        }
        // 位置：x 來回移動、y 慢慢上下飄
        let (dw, dh) = (MW * s, MH * s);
        let span_x = (w - dw) as u64; let span_y = (h - dh) as u64;
        let px = { let t = (seq * 3) % (2 * span_x); if t < span_x { t } else { 2 * span_x - t } } as usize;
        let py = { let t = seq % (2 * span_y); if t < span_y { t } else { 2 * span_y - t } } as usize;
        // 雙線性放大 28×28 → (28s)×(28s)，取 max 疊在背景上
        let img = self.ds.image(idx);
        let at = |x: isize, y: isize| -> f32 {
            if x < 0 || y < 0 || x >= MW as isize || y >= MH as isize { 0.0 } else { img[y as usize * MW + x as usize] as f32 }
        };
        for oy in 0..dh {
            for ox in 0..dw {
                let fx = (ox as f32 + 0.5) / s as f32 - 0.5; let fy = (oy as f32 + 0.5) / s as f32 - 0.5;
                let (x0, y0) = (fx.floor() as isize, fy.floor() as isize);
                let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
                let v = at(x0, y0) * (1.0 - ax) * (1.0 - ay) + at(x0 + 1, y0) * ax * (1.0 - ay)
                      + at(x0, y0 + 1) * (1.0 - ax) * ay + at(x0 + 1, y0 + 1) * ax * ay;
                let d = &mut dst[(py + oy) * w + px + ox];
                *d = (*d).max(v.round().clamp(0.0, 255.0) as u8);
            }
        }
        (label, BBox { x: px, y: py, w: dw, h: dh })
    }
}

/// 找數字：亮度 > thr 的像素的外框。沒有任何像素超過門檻就回傳 None。
/// 這是最簡單的「偵測」：一個畫面只有一個亮的物體時才成立（真實場景要換成偵測模型，見重點說明）。
pub fn locate(img: &[u8], w: usize, h: usize, thr: u8) -> Option<BBox> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        let row = &img[y * w..(y + 1) * w];
        for (x, &v) in row.iter().enumerate() {
            if v > thr { x0 = x0.min(x); x1 = x1.max(x); y0 = y0.min(y); y1 = y1.max(y); }
        }
    }
    (x0 != usize::MAX).then(|| BBox { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 })
}

/// 把外框裡的內容轉成 MNIST 的樣子：長邊縮到 20 px（區域平均），背景扣掉，
/// 再依「質心」放到 28×28 的中央（MNIST 資料集本身就是這樣做的）。輸出 784 byte。
pub fn to_mnist(img: &[u8], w: usize, b: BBox, thr: u8, out: &mut [u8; PIXELS]) {
    let side = b.w.max(b.h) as f32;
    let k = 20.0 / side;
    let (nw, nh) = (((b.w as f32 * k).round() as usize).clamp(1, 20), ((b.h as f32 * k).round() as usize).clamp(1, 20));
    let mut small = vec![0f32; nw * nh];
    for oy in 0..nh {
        let sy0 = b.y + oy * b.h / nh; let sy1 = (b.y + (oy + 1) * b.h / nh).max(sy0 + 1);
        for ox in 0..nw {
            let sx0 = b.x + ox * b.w / nw; let sx1 = (b.x + (ox + 1) * b.w / nw).max(sx0 + 1);
            let mut acc = 0u32;
            for y in sy0..sy1 { for x in sx0..sx1 {
                let v = img[y * w + x];
                // 背景（≤ 門檻的一半）視為 0，其餘線性拉回 0..255，避免背景雜訊被模型當成筆畫
                let lo = thr / 2;
                acc += if v <= lo { 0 } else { ((v - lo) as u32 * 255) / (255 - lo) as u32 };
            }}
            small[oy * nw + ox] = acc as f32 / ((sy1 - sy0) * (sx1 - sx0)) as f32;
        }
    }
    let (mut sum, mut cx, mut cy) = (0f32, 0f32, 0f32);
    for y in 0..nh { for x in 0..nw { let v = small[y * nw + x]; sum += v; cx += v * x as f32; cy += v * y as f32; } }
    let (cx, cy) = if sum > 0.0 { (cx / sum, cy / sum) } else { (nw as f32 / 2.0, nh as f32 / 2.0) };
    let ox = (14.0 - cx).round().clamp(0.0, (MW - nw) as f32) as usize;
    let oy = (14.0 - cy).round().clamp(0.0, (MH - nh) as f32) as usize;
    out.fill(0);
    for y in 0..nh { for x in 0..nw { out[(oy + y) * MW + ox + x] = small[y * nw + x].round().min(255.0) as u8; } }
}

/// 在灰階圖上畫外框（白線 2 px，外面再一圈黑線，任何背景都看得到）
pub fn draw_box(img: &mut [u8], w: usize, h: usize, b: BBox) {
    let mut put = |x: isize, y: isize, v: u8| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h { img[y as usize * w + x as usize] = v; }
    };
    let (x0, y0, x1, y1) = (b.x as isize, b.y as isize, (b.x + b.w) as isize - 1, (b.y + b.h) as isize - 1);
    for (d, v) in [(-3, 0u8), (-2, 255), (-1, 255)] {
        for x in x0 + d..=x1 - d { put(x, y0 + d, v); put(x, y1 - d, v); }
        for y in y0 + d..=y1 - d { put(x0 + d, y, v); put(x1 - d, y, v); }
    }
}

/// 存灰階 PNG
pub fn save_png(path: &str, img: &[u8], w: usize, h: usize) -> eyre::Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, w as u32, h as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(img)?;
    Ok(())
}

/// 分位數（v 必須已排序）
pub fn pct(v: &[u64], p: f64) -> u64 {
    if v.is_empty() { return 0; }
    v[(((v.len() - 1) as f64) * p).round() as usize]
}
/// 一行 p50/p99/max（µs → ms 顯示）
pub fn summary_us(v: &mut [u64]) -> String {
    v.sort_unstable();
    format!("p50 {:.2} / p99 {:.2} / max {:.2} ms", pct(v, 0.5) as f64 / 1e3, pct(v, 0.99) as f64 / 1e3, pct(v, 1.0) as f64 / 1e3)
}
