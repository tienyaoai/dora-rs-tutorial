# dora-rs 教學 第 19 篇：綜合專題（上）——攝影機 → 辨識 → 結果輸出：影像來源節點、candle 辨識節點與延遲量測

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0），rustc／cargo 1.99.1，`candle-core` 0.11.1（沿用第 17 篇），`png` 0.18.1，ffmpeg 7.1.5（系統內建）。驗證日期：2026-11-19（台北時間），只在 x86_64 Linux 虛擬機（8 核、沒有 GPU、**沒有攝影機**）上跑。三個節點「camera → detect → sink」：camera 有兩種模擬來源——程式內合成（320×240 灰階背景上放一個放大 4 倍、會移動的 MNIST 測試圖數字）與「用 ffmpeg 解碼真的 H.264 影片檔」（影片本身是用同一個合成器產生、再用 libx264 壓縮）；detect 用「二值化找外框 → 裁成 28×28 → 第 17 篇訓練好的 candle CNN」辨識；sink 印文字、寫 CSV、每 100 張存一張畫了外框的 PNG，並沿用第 18 篇的 `t_ns` 時間戳算 p50／p99。實測摘要（全部 `dora run` 實跑）：（1）合成來源 600 張（約 30 張/秒），600 張都找到數字，正確 585 張（97.50%），0 丟幀，端到端 p50 1.69 ms、p99 2.85 ms；（2）影片檔來源 300 張，正確 297 張（99.00%），0 丟幀，端到端 p50 1.65 ms、p99 2.69 ms；（3）壓力測試 640×480、200 張/秒、detect 每張故意多睡 10 ms：`queue_size: 1` 與 `10` 都只處理 925 張（約丟一半多），但端到端 p50 分別是 14.74 ms 與 59.58 ms；（4）換成真攝影機的 `ffmpeg -f v4l2 -i /dev/video0` 設定**有實際跑**，但這台沒有 `/dev/video0`，結果是預期中的「打不開裝置」錯誤，**沒有拍到任何一張真實畫面**。**沒驗證**：真實攝影機、`v4l` crate（純 Rust 的 V4L2 綁定）、真實場景中的數字（只有合成畫面）、多物體偵測、ZYNQ 上的執行。

> **關於日誌：** 本篇引用的 `run/*.log` 等完整執行日誌**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。範例可依「怎麼跑／執行」小節自行重現。

> **本 repo 說明：** 本篇範例依賴第 17 篇的 `examples/lesson-17/`（程式庫、`data/` 測試集、`models/mnist-cnn.safetensors`），請先依第 17 篇準備。影片檔 `clip.mp4`（約 2.6 MB）與 `clip.labels` 是產生物，**未**納入本 repo；編好 release 後執行 `./make-clip.sh 300` 即可重新產生（需要 `ffmpeg`）。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 18 篇](lesson-18.md)
- 下一課：第 20 篇**尚未發布**（僅規劃；見 [課程大綱](../../README.md#課程大綱20-篇規劃)）
- 本課範例：[examples/lesson-19/](../../examples/lesson-19/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [常見錯誤](#常見錯誤)
- [小練習](#小練習)
- [本篇沒有驗證成功的部分](#本篇沒有驗證成功的部分)
## 重點說明

第 19～20 篇是綜合專題：把前面學過的東西接成一條「攝影機 → 辨識 → 顯示」的管線。本篇做前半段——**影像來源、辨識、結果輸出（文字＋存圖）與量測**；第 20 篇再做顯示、整合與 ZYNQ 部署。

```
┌─────────────┐  image（mono8, W×H）  ┌──────────────┐  result（10×f32＋外框）  ┌──────────┐
│ camera      │ ─────────────────────▶ │ detect       │ ───────────────────────▶ │ sink     │
│ synth/ffmpeg│   queue_size 1         │ 找框＋CNN    │   queue_size 100         │ 文字/存圖│
└─────────────┘ ─────────────────────────────────────────────────────────────▶ └──────────┘
                  image（同一份，只為了存圖；queue_size 2）
```

### 1. 專題架構：每個節點只做一件事

| 節點 | 輸入 | 輸出 | 換掉它時 |
|---|---|---|---|
| `camera` | `dora/timer/millis/33`（約 30 張/秒） | `image`：Arrow UInt8，W×H byte，metadata 帶 `seq`、`t_ns`、`width`、`height`、`encoding="mono8"`、`label` | 換成真攝影機、ZYNQ 上的 DMA 影像（第 16 篇） |
| `detect` | `camera/image` | `result`：10 個 Float32 機率，metadata 加 `pred`、`score`、`found`、外框 `bx/by/bw/bh`、各段時間 | 換成 ONNX（第 17 篇的 ort）、換成 FPGA 加速 |
| `sink` | `detect/result`、`camera/image` | 檔案（CSV、PNG）與 log | 第 20 篇換成顯示節點 |

**介面定在 metadata 上**：只要新的節點送出一樣的 `image`（UInt8＋`width`/`height`/`encoding`）或一樣的 `result`，其他節點完全不用改。這就是用 dora 的好處：第 20 篇上 ZYNQ 時，`camera` 可以換成讀 FPGA DMA 的節點、`detect` 可以換成呼叫 FPGA CNN 加速器的節點，`sink` 一行都不用動。

### 2. 影像來源節點：沒有攝影機時怎麼模擬

box 是雲端虛擬機，沒有 `/dev/video*`。本篇的 `camera` 節點提供兩種來源（環境變數 `SOURCE`）：

- **`synth`（程式內合成）**：灰階背景（漸層 0～40 加上 0～31 的雜訊）上放一個 MNIST 測試集的數字，用雙線性內插放大 4 倍（112×112），每張往右移 3 px、往下移 1 px，碰到邊界就折返，每 15 張換一個數字。好處是**有正確答案**（`label`），sink 可以算正確率；而且每次執行內容一樣（固定亂數種子），方便比較。
- **`ffmpeg`（影片檔）**：開一個 `ffmpeg` 子行程，把影片解碼、縮放成 W×H 灰階 rawvideo，從它的 stdout 每次 `read_exact` 一張。影片檔 `clip.mp4` 是 `make-clip` 用同一個合成器寫出 300 張、再用 libx264（`-crf 23`）壓縮的，**是真的有損壓縮影片**，壓縮雜訊會稍微改變像素值（所以正確率和合成來源不完全一樣，見實測輸出 2）。答案另外放在 `clip.labels`。

兩種來源都用 `send_output_raw` 直接寫進 dora 配好的輸出緩衝。320×240 = 76,800 byte，大於 log 印出的 `zero_copy_threshold=4096`，所以會走共享記憶體（第 10 篇）。

### 3. 換成真攝影機（V4L2）的做法——**本篇沒有攝影機，只驗證了「設定可以跑、打不開時會正確報錯」**

Linux 的 USB 攝影機（UVC）與 ZYNQ 上很多 MIPI 攝影機的驅動都走 V4L2（Video4Linux2），裝置是 `/dev/video0` 這類檔案。有兩條路：

1. **沿用本篇的 ffmpeg 來源**（最省事）：只改 dataflow 的 env，
   `FFMPEG_INPUT: "-f v4l2 -video_size 640x480 -framerate 30 -i /dev/video0"`（見 `dataflow-v4l2.yml`）。ffmpeg 會處理 YUYV／MJPEG 解碼，再縮成灰階。本篇實際跑了這個設定，因為沒有裝置，ffmpeg 印出 `Cannot open video device /dev/video0: No such file or directory`，camera 節點回傳錯誤、`dora run` 結束碼 1（見實測輸出 4）。
2. **用 `v4l` crate 直接讀**（少一個行程、少一次管線複製，ZYNQ 上比較省）：概念是 `Device::new(0)` → `set_format`（例如 640×480 YUYV）→ `MmapStream` 取得 mmap 的驅動緩衝 → YUYV 每 2 byte 的第一個就是亮度 Y，取出來就是灰階圖 → `send_output_raw`。**本篇沒有加這個 crate、沒有編譯也沒有跑**（沒有裝置可測，依規定不擴大範圍）；第 20 篇講 ZYNQ 部署時會再說明。

另外要注意**節奏**：本篇用 dora 的 timer 每 33 ms 讀一張，這對「播放影片檔」是對的（影片檔不會自己控制速度，讀多快就解多快）。真攝影機自己會用固定幀率送畫面，若 timer 比攝影機慢，ffmpeg 管線或驅動緩衝裡的舊畫面會越積越多、延遲變大；比較好的做法是不要 timer，改成「阻塞讀一張就送一張」，並且同時處理 dora 的 Stop 事件。這部分本篇**沒有實作也沒有測**。

### 4. 辨識節點：找框 → 裁切 → 第 17 篇的 CNN

第 17 篇的模型吃的是 28×28、黑底白字、置中的 MNIST 圖；攝影機畫面是 320×240，數字在任意位置。所以 `detect` 先做一個**最簡單的「偵測」**：

1. `locate`：亮度 > 128（`THRESHOLD`）的像素算外框。背景最亮約 71，遠低於門檻，所以在這個合成畫面上一定找得到、而且只會找到數字。**真實場景不會這麼乾淨**（光線、陰影、別的亮東西），要換成真的偵測模型（例如 YOLO 類的 ONNX 模型）；本篇沒有做。
2. `to_mnist`：把外框長邊用區域平均縮成 20 px，背景（≤ 門檻一半）壓成 0，再依「質心」放到 28×28 中央——這是 MNIST 資料集本身的做法，前處理和訓練資料一致，正確率才會接近第 17 篇。
3. 呼叫第 17 篇的 `Net`（直接用 path 相依 `lesson-17 = { path = "../lesson-17", default-features = false }`，關掉 onnx feature，不必再編 ort）。模型載入與暖機在事件迴圈之前。
4. 輸出時 `metadata.parameters.clone()` 把上游的 `seq`、`t_ns`、`label` 原樣帶下去（第 18 篇的做法），再加上自己的欄位。

### 5. sink 與量測：沿用第 18 篇的 t_ns

- camera 在**擷取完成**時蓋 `t_ns`；detect 收到時記 `t_in_ns`、送出前記 `t_out_ns`；sink 收到時再取一次時間。於是端到端延遲可以拆成三段：**camera→detect**（傳輸＋排隊）、**detect 內部**（找框＋推論）、**detect→sink**。三個節點在同一台機器、同一個時鐘，可以直接相減（跨機器要改用 HLC，第 10 篇）。
- 前 30 筆不計（`WARMUP`），印 p50／p99／max。
- `seq` 不連續的地方就是丟掉的幀（序號缺口）。

### 6. 佇列設定：辨識只要最新的那張

- `detect` 的輸入 `queue_size: 1`：辨識追不上時丟舊的、只留最新的一張，延遲不會越積越大（第 17、18 篇的結論）。實測輸出 3 會看到：模型變慢時，`queue_size: 10` 並**沒有**少丟幀（兩者都處理 925 張），只是每張多等了約 45 ms。
- `sink` 的 `result` 輸入 `queue_size: 100`：結果很小（10 個 f32＋metadata），每筆都要記錄，佇列給大。
- `sink` 的 `image` 輸入 `queue_size: 2`：只為了存圖，丟了也沒關係；而且 sink 只複製 `seq % SAVE_EVERY == 0` 的那幾張，其他畫面看一眼就放掉。

## 完整範例

位置：[examples/lesson-19/](../../examples/lesson-19/)（需要第 17 篇的 `../lesson-17/`：程式庫、`data/` 的 MNIST 測試集、`models/mnist-cnn.safetensors`）。

```
examples/lesson-19/
├── Cargo.toml
├── Cargo.lock            # 從第 17 篇複製，套件版本與第 17 篇一致
├── dataflow.yml          # 合成來源，320×240，約 30 張/秒，600 張
├── dataflow-file.yml     # 影片檔來源（ffmpeg 解碼 clip.mp4）
├── dataflow-v4l2.yml     # 真攝影機設定（本機沒有 /dev/video0，預期失敗）
├── dataflow-stress-q1.yml / dataflow-stress-q10.yml  # 壓力測試（640×480、200 張/秒、慢模型）
├── make-clip.sh          # 產生 clip.mp4＋clip.labels
├── run-demo.sh           # dora build → dora run
└── src/
    ├── lib.rs            # 合成器、找框、裁切、畫框、存 PNG、分位數
    └── bin/{camera,detect,sink,make-clip}.rs
```

執行步驟：

```bash
cd examples/lesson-19   # 自 repo 根目錄
export RUSTUP_TOOLCHAIN=1.99.1-x86_64-unknown-linux-gnu   # 只有 rustup 沒設預設 toolchain 時才需要
nohup cargo build --release > run/build.log 2>&1 &        # 第一次約 2.5 分鐘，用背景跑
./make-clip.sh 300                                        # 產生測試影片
./run-demo.sh dataflow.yml 60
./run-demo.sh dataflow-file.yml 60
./run-demo.sh dataflow-stress-q1.yml 60; ./run-demo.sh dataflow-stress-q10.yml 60
./run-demo.sh dataflow-v4l2.yml 30                        # 沒有攝影機：預期失敗
```

### Cargo.toml

```toml
[package]
name = "lesson-19"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
candle-core = "0.11"
png = "0.18"
# 直接沿用第 17 篇的程式庫：MNIST 讀檔、小型 CNN（Net）、前處理、top1、Lat。
# 關掉預設的 onnx feature，就不會編 ort（也就不需要第 17 篇的 libstdc++ 繞道）。
lesson-17 = { path = "../lesson-17", default-features = false }
```

### src/lib.rs

```rust
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
```

### src/bin/camera.rs

```rust
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
```

### src/bin/detect.rs

```rust
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
```

### src/bin/sink.rs

```rust
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
```

### src/bin/make-clip.rs

```rust
//! 產生測試影片的原始畫面：用和 camera 的 synth 一模一樣的合成器，寫出 N 張灰階 rawvideo（clip.y8）
//! 與每張的答案（clip.labels，每行一個數字）。再用 ffmpeg 壓成 clip.mp4（見 make-clip.sh）。
//! 用法：make-clip 輸出.y8 輸出.labels 張數 [寬 高]
use lesson_19::{Mnist, Synth};
use std::io::Write;

fn main() -> eyre::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    eyre::ensure!(a.len() >= 4, "用法：make-clip 輸出.y8 輸出.labels 張數 [寬 高]");
    let n: u64 = a[3].parse()?;
    let (w, h): (usize, usize) = (a.get(4).map_or(Ok(320), |s| s.parse())?, a.get(5).map_or(Ok(240), |s| s.parse())?);
    let ds = Mnist::load("../lesson-17/data", "t10k")?;
    let mut s = Synth::new(ds, w, h, 4, 15);
    let mut y8 = std::io::BufWriter::new(std::fs::File::create(&a[1])?);
    let mut lab = std::io::BufWriter::new(std::fs::File::create(&a[2])?);
    let mut buf = vec![0u8; w * h];
    for seq in 0..n {
        let (label, _) = s.render(seq, &mut buf);
        y8.write_all(&buf)?;
        writeln!(lab, "{label}")?;
    }
    println!("寫出 {n} 張 {w}x{h} 到 {}，答案到 {}", a[1], a[2]);
    Ok(())
}
```

### dataflow.yml

```yaml
# 第 19 篇：camera（合成影像 320×240，約 30 張/秒）→ detect（找框＋candle CNN）→ sink（文字＋存圖＋延遲統計）
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: synth
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "600"          # 約 20 秒；送完 camera 結束，下游跟著結束
    inputs:
      tick: dora/timer/millis/33
    outputs:
      - image

  - id: detect
    build: cargo build --release --bin detect
    path: target/release/detect
    env:
      MODEL: ../lesson-17/models/mnist-cnn.safetensors
      RAYON_NUM_THREADS: "1"
    inputs:
      image:
        source: camera/image
        queue_size: 1        # 辨識只要「最新的一張」：追不上時丟舊的，延遲不會越積越大（第 17、18 篇）
    outputs:
      - result

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    env:
      RESULT_DIR: run/out-synth
      SAVE_EVERY: "100"
    inputs:
      result:
        source: detect/result
        queue_size: 100      # 結果很小（10 個 f32），全部都要記錄，佇列給大一點
      image:
        source: camera/image
        queue_size: 2        # 只為了存圖，丟了沒關係
```

### dataflow-file.yml

```yaml
# 第 19 篇：camera（ffmpeg 解碼 clip.mp4，320×240，約 30 張/秒）→ detect（找框＋candle CNN）→ sink（文字＋存圖＋延遲統計）
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: ffmpeg
      FFMPEG_INPUT: "-i clip.mp4"
      LABELS: clip.labels
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "100000"       # 影片讀完就結束
    inputs:
      tick: dora/timer/millis/33
    outputs:
      - image

  - id: detect
    build: cargo build --release --bin detect
    path: target/release/detect
    env:
      MODEL: ../lesson-17/models/mnist-cnn.safetensors
      RAYON_NUM_THREADS: "1"
    inputs:
      image:
        source: camera/image
        queue_size: 1        # 辨識只要「最新的一張」：追不上時丟舊的，延遲不會越積越大（第 17、18 篇）
    outputs:
      - result

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    env:
      RESULT_DIR: run/out-file
      SAVE_EVERY: "100"
    inputs:
      result:
        source: detect/result
        queue_size: 100      # 結果很小（10 個 f32），全部都要記錄，佇列給大一點
      image:
        source: camera/image
        queue_size: 2        # 只為了存圖，丟了沒關係
```

### dataflow-v4l2.yml

```yaml
# 第 19 篇：camera（ffmpeg 讀 v4l2 攝影機 /dev/video0，縮成 320×240，約 30 張/秒）→ detect（找框＋candle CNN）→ sink（文字＋存圖＋延遲統計）
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: ffmpeg
      FFMPEG_INPUT: "-f v4l2 -video_size 640x480 -framerate 30 -i /dev/video0"
      LABELS: none
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "100000"       # 影片讀完就結束
    inputs:
      tick: dora/timer/millis/33
    outputs:
      - image

  - id: detect
    build: cargo build --release --bin detect
    path: target/release/detect
    env:
      MODEL: ../lesson-17/models/mnist-cnn.safetensors
      RAYON_NUM_THREADS: "1"
    inputs:
      image:
        source: camera/image
        queue_size: 1        # 辨識只要「最新的一張」：追不上時丟舊的，延遲不會越積越大（第 17、18 篇）
    outputs:
      - result

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    env:
      RESULT_DIR: run/out-v4l2
      SAVE_EVERY: "100"
    inputs:
      result:
        source: detect/result
        queue_size: 100      # 結果很小（10 個 f32），全部都要記錄，佇列給大一點
      image:
        source: camera/image
        queue_size: 2        # 只為了存圖，丟了沒關係
```

### dataflow-stress-q1.yml

```yaml
# 第 19 篇壓力測試：640×480、200 張/秒，detect 每張故意多睡 10 ms（模擬較重的模型，上限約 90 張/秒），
# detect 輸入 queue_size=1。比較「丟幀」與「延遲」的取捨。
nodes:
  - id: camera
    path: target/release/camera
    env:
      SOURCE: synth
      WIDTH: "640"
      HEIGHT: "480"
      FRAMES: "2000"
    inputs:
      tick: dora/timer/millis/5
    outputs:
      - image

  - id: detect
    path: target/release/detect
    env:
      MODEL: ../lesson-17/models/mnist-cnn.safetensors
      RAYON_NUM_THREADS: "1"
      SLOW_MS: "10"
    inputs:
      image:
        source: camera/image
        queue_size: 1
    outputs:
      - result

  - id: sink
    path: target/release/sink
    env:
      RESULT_DIR: run/out-stress-q1
    inputs:
      result:
        source: detect/result
        queue_size: 1000
```

`dataflow-stress-q10.yml` 與上面完全相同，只有註解裡的數字、detect 的 `queue_size: 10` 與 `RESULT_DIR: run/out-stress-q10` 不同。壓力測試的兩個 yml 沒有 `build:` 欄位，直接用前面編好的 `target/release/`。

### make-clip.sh

```bash
#!/usr/bin/env bash
# 產生測試影片：make-clip 寫 rawvideo＋答案 → ffmpeg 壓成 H.264 的 clip.mp4（有損壓縮，像真的影片檔）
set -euo pipefail
cd "$(dirname "$0")"
N=${1:-300}
target/release/make-clip clip.y8 clip.labels "$N" 320 240
ffmpeg -hide_banner -loglevel error -y -f rawvideo -pix_fmt gray -s 320x240 -r 30 -i clip.y8 \
       -c:v libx264 -crf 23 -pix_fmt yuv420p clip.mp4
rm clip.y8
ls -l clip.mp4 clip.labels
```

### run-demo.sh

```bash
#!/usr/bin/env bash
# 一鍵實測：dora build → dora run（最多 N 秒）。用法：./run-demo.sh [dataflow.yml] [秒數]
set -uo pipefail
YML=${1:-dataflow.yml}; SECS=${2:-60}; TAG=$(basename "$YML" .yml)
cd "$(dirname "$0")"
DORA=${DORA:-dora}
mkdir -p run
$DORA build "$YML" > run/$TAG-build.log 2>&1 || { echo "dora build 失敗，見 run/$TAG-build.log"; exit 1; }
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/$TAG.log
$DORA run "$YML" --stop-after ${SECS}s >> run/$TAG.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/$TAG.log
```

## 實際執行

環境：box（x86_64 Linux 虛擬機，8 核，沒有 GPU、沒有 `/dev/video*`），dora v1.1.0 CLI 在 `.../bin/dora`。以下輸出都是 `run/*.log` 的真實片段，只用 `grep` 拿掉了 `Compiling …` 與 `DEBUG` 行。

第一次 `cargo build --release`（背景執行）花了 2 分 29 秒，0 個警告。`./make-clip.sh 300` 的輸出（`run/make-clip.log`，另加上 `ffprobe` 檢查）：

```
寫出 300 張 320x240 到 clip.y8，答案到 clip.labels
-rw-r--r-- 1 box box     600 Nov 19 07:51 clip.labels
-rw-r--r-- 1 box box 2680303 Nov 19 07:52 clip.mp4
stream|codec_name=h264|width=320|height=240|r_frame_rate=30/1|nb_frames=300
```

（300 張的影片有 2.6 MB，偏大：背景是隨機雜訊，H.264 很難壓。）

### 實測輸出 1：合成來源 `dataflow.yml`（2026-11-19 07:59:30 開始，台北時間）

```
開始 2026-11-19 07:59:30 CST
07:59:31 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 3.13 ms；THRESHOLD=128 SLOW_MS=0
07:59:32 stdout camera: [camera] SOURCE=synth 320x240 FRAMES=600 zero_copy_threshold=4096
07:59:32 stdout sink: [sink] 第 1 筆：seq=0 label=7 pred=7 score=1.000 外框=(25,29,61x79) 機率 10 個 f32 端到端 2.63 ms
07:59:32 stdout sink: [sink] 第 2 筆：seq=1 label=7 pred=7 score=1.000 外框=(28,30,61x79) 機率 10 個 f32 端到端 1.49 ms
07:59:32 stdout sink: [sink] 第 3 筆：seq=2 label=7 pred=7 score=1.000 外框=(31,31,61x79) 機率 10 個 f32 端到端 1.35 ms
07:59:52 stdout camera: [camera] 結束：送出 600 張
07:59:52 stdout detect: [detect] 結束：處理 600 張，找到數字 600 張；前處理 p50 0.05 ms / p99 0.09 ms / max 0.11 ms；模型 p50 0.51 ms / p99 0.90 ms / max 1.97 ms；dora 佇列丟棄 0
07:59:52 INFO   camera: daemon camera finished successfully
07:59:52 stdout sink: [sink] 結果 600 筆（正確 585/600 = 97.50%，找不到數字 0），序號缺口 0；收到原始畫面 600 張，存圖 6 張（找不到對應畫面 0）
07:59:52 stdout sink: [sink] 延遲（前 30 筆不計，共 570 筆）端到端 p50 1.69 / p99 2.85 / max 8.00 ms
07:59:52 stdout sink: [sink]   camera→detect p50 0.62 / p99 1.33 / max 6.98 ms；detect 內部 p50 0.58 / p99 0.97 / max 2.04 ms；detect→sink p50 0.44 / p99 1.06 / max 3.21 ms
07:59:52 INFO   detect: daemon detect finished successfully
07:59:52 INFO   sink: daemon sink finished successfully
dora run 結束碼 0
```

`run/out-synth/results.csv` 開頭：

```
seq,label,pred,score,found,bx,by,bw,bh,e2e_us
0,7,7,0.9997,1,25,29,61,79,2628
1,7,7,0.9997,1,28,30,61,79,1489
2,7,7,0.9997,1,31,31,61,79,1353
```

存下的 6 張圖：`frame_00000_label7_pred7.png`、`frame_00100_label8_pred8.png`、`frame_00200_label6_pred6.png`、`frame_00300_label6_pred6.png`、`frame_00400_label9_pred9.png`、`frame_00500_label9_pred9.png`。打開 `frame_00200` 看過：暗灰色雜訊背景，右上方一個白色的「6」，外面有白框，框得剛好。

怎麼讀：

- 15 張錯的**全是同一個數字**：`seq` 45～59（同一張 MNIST 圖「8」連續出現 15 張），每張都被認成 3，score 只有約 0.45。也就是 40 個不同的數字裡錯了 1 個。為什麼這張 8 會錯（是 MNIST 圖本身難認、還是本篇的裁切讓它變形）**沒有追查**。
- 延遲：端到端 p50 1.69 ms，三段各約 0.4～0.6 ms，其中模型本身 p50 0.51 ms。在 30 張/秒（每 33 ms 一張）下，整條管線大約只用了 5% 的時間預算。
- 0 丟幀（`dora 佇列丟棄 0`、序號缺口 0）。

### 實測輸出 2：影片檔來源 `dataflow-file.yml`（2026-11-19 07:59:52 開始，台北時間）

```
開始 2026-11-19 07:59:52 CST
07:59:53 stdout camera: [camera] ffmpeg 來源：-i clip.mp4（答案 300 行）
07:59:53 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 3.77 ms；THRESHOLD=128 SLOW_MS=0
07:59:54 stdout camera: [camera] SOURCE=ffmpeg 320x240 FRAMES=100000 zero_copy_threshold=4096
07:59:54 stdout sink: [sink] 第 1 筆：seq=0 label=7 pred=7 score=1.000 外框=(24,29,62x79) 機率 10 個 f32 端到端 2.61 ms
07:59:54 stdout sink: [sink] 第 2 筆：seq=1 label=7 pred=7 score=1.000 外框=(27,30,62x79) 機率 10 個 f32 端到端 2.34 ms
07:59:54 stdout sink: [sink] 第 3 筆：seq=2 label=7 pred=7 score=1.000 外框=(30,31,62x79) 機率 10 個 f32 端到端 1.52 ms
08:00:04 stdout camera: [camera] 影片讀完
08:00:04 stdout camera: [camera] 結束：送出 300 張
08:00:04 stdout detect: [detect] 結束：處理 300 張，找到數字 300 張；前處理 p50 0.06 ms / p99 0.09 ms / max 0.23 ms；模型 p50 0.51 ms / p99 1.17 ms / max 2.71 ms；dora 佇列丟棄 0
08:00:04 stdout sink: [sink] 結果 300 筆（正確 297/300 = 99.00%，找不到數字 0），序號缺口 0；收到原始畫面 300 張，存圖 3 張（找不到對應畫面 0）
08:00:04 stdout sink: [sink] 延遲（前 30 筆不計，共 270 筆）端到端 p50 1.65 / p99 2.69 / max 4.08 ms
08:00:04 stdout sink: [sink]   camera→detect p50 0.57 / p99 1.18 / max 1.39 ms；detect 內部 p50 0.58 / p99 1.23 / max 2.79 ms；detect→sink p50 0.40 / p99 0.98 / max 1.04 ms
dora run 結束碼 0
```

錯的 3 張（`run/out-file/results.csv`）：

```
45,8,3,0.4676,1,151,69,80,63,2233
46,8,3,0.4510,1,154,70,80,63,1685
57,8,3,0.4548,1,187,81,80,63,3683
```

影片的前 300 張和合成來源的前 300 張是同一批畫面。合成來源前 300 張正確 285 張（seq 45～59 全錯），影片來源正確 297 張——**同一張「8」，經過 H.264 壓縮後反而有 12 張認對了**。壓縮改變了像素值，剛好把這張難認的 8 推過了邊界。score 都只有約 0.45，表示模型本來就在 3 和 8 之間猶豫；這不代表「壓縮讓辨識變好」，只是這張圖剛好在邊界上。外框也從 61×79 變成 62×79（壓縮雜訊讓邊緣多了一點點亮的像素）。

### 實測輸出 3：壓力測試——640×480、200 張/秒、detect 每張多睡 10 ms（2026-11-19 07:58:37 開始，台北時間）

detect 每張約 10.6 ms，上限約 94 張/秒，追不上 200 張/秒。兩次只差 detect 的 `queue_size`：

```
開始 2026-11-19 07:58:37 CST                                      ← queue_size: 1
07:58:38 stdout sink: [sink] 第 1 筆：seq=0 label=7 pred=7 score=1.000 外框=(25,29,61x79) 機率 10 個 f32 端到端 15.02 ms
07:58:38 stdout sink: [sink] 第 2 筆：seq=3 label=7 pred=7 score=1.000 外框=(34,32,61x79) 機率 10 個 f32 端到端 13.80 ms
07:58:38 stdout sink: [sink] 第 3 筆：seq=5 label=7 pred=7 score=1.000 外框=(40,34,61x79) 機率 10 個 f32 端到端 14.80 ms
07:58:48 stdout camera: [camera] 結束：送出 2000 張
07:58:48 stdout detect: [detect] 結束：處理 925 張，找到數字 925 張；前處理 p50 0.10 ms / p99 0.18 ms / max 0.32 ms；模型 p50 10.57 ms / p99 11.06 ms / max 16.40 ms；dora 佇列丟棄 1074
07:58:48 stdout sink: [sink] 結果 925 筆（正確 890/925 = 96.22%，找不到數字 0），序號缺口 1074；收到原始畫面 0 張，存圖 0 張（找不到對應畫面 0）
07:58:48 stdout sink: [sink] 延遲（前 30 筆不計，共 895 筆）端到端 p50 14.74 / p99 17.94 / max 29.88 ms
07:58:48 stdout sink: [sink]   camera→detect p50 3.45 / p99 6.36 / max 9.43 ms；detect 內部 p50 10.69 / p99 11.17 / max 16.55 ms；detect→sink p50 0.57 / p99 1.13 / max 14.77 ms
dora run 結束碼 0

開始 2026-11-19 07:58:48 CST                                      ← queue_size: 10
07:58:48 stdout sink: [sink] 第 1 筆：seq=0 label=7 pred=7 score=1.000 外框=(25,29,61x79) 機率 10 個 f32 端到端 14.11 ms
07:58:48 stdout sink: [sink] 第 2 筆：seq=1 label=7 pred=7 score=1.000 外框=(28,30,61x79) 機率 10 個 f32 端到端 21.98 ms
07:58:48 stdout sink: [sink] 第 3 筆：seq=2 label=7 pred=7 score=1.000 外框=(31,31,61x79) 機率 10 個 f32 端到端 28.02 ms
07:58:58 stdout camera: [camera] 結束：送出 2000 張
07:58:58 stdout detect: [detect] 結束：處理 925 張，找到數字 925 張；前處理 p50 0.12 ms / p99 0.21 ms / max 0.74 ms；模型 p50 10.53 ms / p99 11.03 ms / max 16.97 ms；dora 佇列丟棄 1065
07:58:58 stdout sink: [sink] 結果 925 筆（正確 890/925 = 96.22%，找不到數字 0），序號缺口 1065；收到原始畫面 0 張，存圖 0 張（找不到對應畫面 0）
07:58:58 stdout sink: [sink] 延遲（前 30 筆不計，共 895 筆）端到端 p50 59.58 / p99 63.45 / max 68.56 ms
07:58:58 stdout sink: [sink]   camera→detect p50 48.34 / p99 51.88 / max 55.19 ms；detect 內部 p50 10.67 / p99 11.14 / max 13.32 ms；detect→sink p50 0.54 / p99 1.14 / max 4.18 ms
dora run 結束碼 0
```

| detect `queue_size` | 處理張數 | dora 佇列丟棄 | 端到端 p50 / p99 | 其中 camera→detect p50 |
|---|---|---|---|---|
| 1 | 925 / 2000 | 1,074 | 14.74 / 17.94 ms | 3.45 ms |
| 10 | 925 / 2000 | 1,065 | 59.58 / 63.45 ms | 48.34 ms |

怎麼讀：

- 模型跟不上時，**佇列加大不會讓你多處理幾張**（925 = 925，被模型速度卡死），只會讓每張在佇列裡多等約 45 ms（≈ 佇列裡排了幾張 × 每張 10.6 ms）。所以辨識節點用 `queue_size: 1`，這和第 17、18 篇的結論一致。
- 640×480 的前處理（找框）p50 從 0.05 ms 變成 0.10～0.12 ms，大致跟像素數（4 倍）成正比，但仍遠小於模型。
- 兩次正確率相同（890/925）而且處理張數相同，但 queue 1 第 2 筆是 seq=3、queue 10 是 seq=1，表示實際處理到的畫面不同；兩者正確數剛好一樣是巧合還是必然，**沒有核對**。
- 小出入：2000 − 925 = 1,075，但「dora 佇列丟棄」是 1,074 與 1,065，兩者都和序號缺口相同；差額（1 張與 10 張）推測是結束時最後幾張的計算方式（序號缺口只算「兩個收到的 seq 之間」），**沒有追查**。

### 實測輸出 4：真攝影機設定 `dataflow-v4l2.yml`（2026-11-19 07:59:23 開始，台北時間）——預期失敗

```
開始 2026-11-19 07:59:23 CST
07:59:24 stdout camera: [camera] ffmpeg 來源：-f v4l2 -video_size 640x480 -framerate 30 -i /dev/video0（答案 0 行）
07:59:24 stdout camera: [video4linux2,v4l2 @ 0x55dfcfc7e140] Cannot open video device /dev/video0: No such file or directory
07:59:24 stdout camera: [in#0 @ 0x55dfcfc7de80] Error opening input: No such file or directory
07:59:24 stdout camera: Error opening input file /dev/video0.
07:59:24 stdout camera: [camera] SOURCE=ffmpeg 320x240 FRAMES=100000 zero_copy_threshold=4096
07:59:24 stdout detect: [detect] 結束：處理 0 張，找到數字 0 張；前處理 —；模型 —；dora 佇列丟棄 0
07:59:24 stdout camera: Error: ffmpeg 一張都沒讀到，來源打不開？（看上面 ffmpeg 的錯誤訊息）
...
[ERROR]
Dataflow failed:

Node `camera` failed: exited with code 1 with stderr output:
---------------------------------------------------------------------------------
Error: ffmpeg 一張都沒讀到，來源打不開？（看上面 ffmpeg 的錯誤訊息）
...
dora run 結束碼 1
```

這證明的只有兩件事：這台的 ffmpeg 有 V4L2 輸入（`ffmpeg -demuxers` 列出 `video4linux2,v4l2`），以及「來源打不開」時 camera 會報錯、dataflow 會失敗，而不是靜靜地送 0 張就結束（第一版程式就是這樣，所以後來才加了 `seq > 0` 的檢查）。**真攝影機的畫面、延遲、YUYV／MJPEG 格式處理都沒有測到。**

## 常見錯誤

1. **`cargo build --offline` 找不到套件**：本篇第一次用 `--offline` 編，出現 `error: no matching package named 'candle-core' found`（雖然 `~/.cargo/registry/cache` 裡有 candle）。拿掉 `--offline` 就正常了。原因沒有追查（推測是本機的索引快取不完整）。
2. **dora 每次 build 都重編一大堆（每次約 1 分鐘）**：第一版的 sink 用環境變數 `OUT_DIR` 指定輸出資料夾。結果 `dora build` 時 sink 花了 59.20 秒；`dora run`（它也會先 build）時 camera 花了 1 分 01 秒、sink 又 59.68 秒，每次都在重編 `ring`、`rustls`、`zenoh`…（這兩份 log 後來被重跑覆蓋，數字取自當時的終端機輸出）。原因：dora 會把節點的 `env:` 也套用到 `build:` 指令上，而 `ring` 的 build script 宣告了 `cargo:rerun-if-env-changed=OUT_DIR`（存證在 `run/ring-rerun-if-env.txt`），所以「有 OUT_DIR 的 sink」和「沒有 OUT_DIR 的 camera」輪流 build 時，cargo 會認為 ring 的環境變了，從 ring 開始整串重編。把變數改名成 `RESULT_DIR` 之後，三個節點的 build 都變成 0.2～0.3 秒。**教訓：節點的 env 不要用 `OUT_DIR`、`CC`、`CFLAGS`、`TARGET` 這類 cargo／build script 會看的名字。**
3. **來源打不開卻「正常結束」**：第一版 camera 在 `read_exact` 失敗時一律當成「影片讀完」，打不開 `/dev/video0` 時會送 0 張、結束碼 0，看起來像成功。現在第一張就讀不到會回傳錯誤。
4. **找不到第 17 篇的檔案**：`detect` 的 `MODEL`、`camera` 的 `DATASET` 預設是 `../lesson-17/...` 的相對路徑，dora 以 dataflow.yml 所在目錄為工作目錄啟動節點。若搬動資料夾，要一起改這兩個 env。
5. **外框找到背景**：如果把 `THRESHOLD` 調得比背景最亮值（約 71）還低，`locate` 會把整個畫面當成外框，辨識結果全亂。本篇沒有實測低門檻的情形，是由程式邏輯推出的。

## 小練習

1. **看看那張被認錯的「8」**：在 `detect` 裡，當 `seq` 是 45 時，把 `crop`（28×28）用 `lesson_19::save_png` 存成 `run/crop_45.png`，再和 MNIST 原圖比較，猜猜為什麼會被認成 3。提示：要在 `to_mnist` 之後存；注意 28×28 的圖很小，可以用 `ffmpeg -i crop_45.png -vf scale=280:280:flags=neighbor big.png` 放大看。
2. **加一個「信心門檻」**：sink 裡 score < 0.6 的結果改印成「不確定」，不算錯也不算對，重新算正確率。想一想：在真實產品上，「不確定時不回答」通常比「答錯」好嗎？
3. （進階，本篇沒有實測）把 `SOURCE=ffmpeg` 的 `FFMPEG_INPUT` 換成 ffmpeg 內建的測試訊號 `-f lavfi -i testsrc=size=320x240:rate=30`（沒有答案檔，label 會是 -1），觀察 `locate` 在一個到處都是亮色塊的畫面上會框出什麼。

## 本篇沒有驗證成功的部分

- **真實攝影機**：box 沒有 `/dev/video*`。`dataflow-v4l2.yml` 有跑，但只得到「打不開裝置」的錯誤（實測輸出 4），沒有拍到任何畫面；YUYV／MJPEG 格式、真攝影機的幀率與延遲、驅動緩衝的排隊都沒測。
- **`v4l` crate（純 Rust 讀 V4L2）**：只在重點說明 3 描述做法，沒有加入相依、沒有編譯、沒有跑。
- **用攝影機自己的節奏取代 timer**：只說明，沒有實作也沒有測。
- **真實場景的辨識**：畫面全是合成的（影片檔也是由同一個合成器產生再壓縮），背景乾淨、一張只有一個數字；`locate` 的「二值化找外框」在真實場景（光線、陰影、多個物體）幾乎一定不夠用，要換成偵測模型，本篇沒有做。
- **那張「8」為什麼被認成 3**：沒有追查（留作小練習 1）。
- **壓力測試的小出入**：「2000 − 處理張數」與「dora 佇列丟棄」差 1 張與 10 張的原因沒有追查；兩次正確數相同是否巧合也沒有核對。每個設定只跑了 1 次，這台是共用虛擬機，雜訊很大（第 18 篇），表中的數字只看量級。
- **ONNX Runtime（ort）版的 detect**：本篇只用 candle，沒有把第 17 篇的 `infer-ort` 接進來。
- **`cargo build --offline` 失敗的原因**：沒有追查（常見錯誤 1）。
- **ZYNQ／aarch64**：沒有交叉編譯、沒有上板子。
- **下一篇預告**：第 20 篇（最終篇）會把 `sink` 換成**顯示節點**（把畫面＋外框＋辨識結果即時畫出來），把本篇的 camera → detect → 顯示整合成一條完整的管線，並收尾到 **ZYNQ 部署**：交叉編譯到 aarch64、把 camera 換成 V4L2 或第 16 篇的 DMA 節點、在板子上的延遲預算怎麼估，以及這 20 篇學到的東西怎麼接到 FPGA 上的 CNN 加速。

收尾：對 `lesson-19` 執行 `cargo clean`，刪除 `target/`（`du` 清除前量到 1.6 GB；`cargo clean` 回報移除 4,677 個檔案、共 1.7 GiB）；刪除本篇執行時留在 `/dev/shm` 的 zenoh 殘檔 `1400051614.zenoh`（12 KB，時間戳 07:59:53，在本篇 `dora run` 期間；開始前 `/dev/shm` 是空的，只刪這一個）；確認沒有殘留的 dora、lesson-19 或 ffmpeg 行程（`pgrep` 找不到）。保留：原始碼、各 yml 與腳本、`clip.mp4`（2.6 MB）與 `clip.labels`、`run/` 裡的 log、CSV 與存下的 PNG，以及 dora 自己寫在 `out/` 的 session 紀錄（`du` 量到 140 KB）。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 18 篇](lesson-18.md)
- 下一課：第 20 篇**尚未發布**（僅規劃；見 [課程大綱](../../README.md#課程大綱20-篇規劃)）
- 本課範例：[examples/lesson-19/](../../examples/lesson-19/)
