# dora-rs 教學 第 17 篇：AI 推論節點——candle 與 ONNX Runtime（ort）跑 MNIST 影像辨識

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0），rustc 1.99.1，`candle-core`／`candle-nn` 0.11.1，`ort` 2.0.0-rc.15（內含 ONNX Runtime 1.31.0 預編譯靜態庫）。驗證日期：2026-11-17（台北時間），只在 x86_64 Linux 虛擬機（8 核、15 GB RAM、沒有 GPU）上跑。**模型是真的**：candle 用的是本篇自己在 CPU 上用 candle 訓練 3 個 epoch 的小 CNN（46,730 個參數，訓練 3 分 46 秒），ort 用的是 ONNX Model Zoo 下載的 `mnist-12.onnx`（26 KB）；影像是 MNIST 測試集（10,000 張），由「假攝影機」節點送出。實測：離線量測整個測試集，candle 模型正確 9,809 張（98.09%），`mnist-12.onnx` 正確 9,891 張（98.91%）；batch 1 單次呼叫 p50 candle 0.24 ms、ort 0.03 ms。dataflow「camera（200 張/秒）→ infer-candle ＋ infer-ort → sink」跑 15 秒，`dora run` 結束碼 0：camera 送出 2,885 張，candle 推論 2,882 張（正確率 97.29%，端到端 p50 0.87 ms），ort 推論 2,883 張（98.23%，p50 0.56 ms）。「模型追不上攝影機」實驗（每次呼叫故意多睡 20 ms）比較了 5 種策略，數字見「實測輸出 3」。**沒驗證**：ZYNQ／aarch64 上的推論（沒有交叉編譯、沒有上板子，第 6 節全是說明）；GPU／CUDA；量化模型；`candle-onnx`（用 candle 直接讀 .onnx）。ort 第一次編譯時遇到兩個錯誤（缺 OpenSSL、缺 `libstdc++.so`），都記錄在「常見錯誤」，繞過後 ort 有跑通。

> **關於日誌：** 本篇引用的 `run/*.log` 等完整執行日誌**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。範例可依「怎麼跑／執行」小節自行重現。

> **本 repo 說明：** 下方目錄樹中的 `.cargo/config.toml`、`linkfix/`（作者那台機器缺 `libstdc++.so` 的繞道，路徑寫死為本機位置）與 `data/`（MNIST 資料集，約 7.9 MB 以上）**未**納入本 repo。一般機器 `apt install g++` 即可；資料請執行 `./fetch-data.sh` 下載。`models/` 兩個小檔（共約 210 KB）有納入。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 16 篇](lesson-16.md)
- 下一課：[第 18 篇 →](lesson-18.md)
- 本課範例：[examples/lesson-17/](../../examples/lesson-17/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [常見錯誤](#常見錯誤)
- [小練習](#小練習)
- [本篇沒有驗證成功的部分](#本篇沒有驗證成功的部分)
## 重點說明

第 16 篇把硬體資料接進 dataflow；第 19～20 篇的綜合專題是「攝影機 → 辨識 → 顯示」。中間缺的就是本篇：**在 dora 節點裡跑神經網路推論**。本篇用最小的影像分類任務（MNIST 手寫數字，28×28 灰階）把整條路打通——模型夠小，在任何 CPU 上都跑得動，換成真正的攝影機與物件偵測模型時，節點的結構完全不用改，只換前處理和模型。

### 1. 推論節點的骨架

```
┌─ 事件迴圈「之前」─────────────────────┐
│ 讀設定（env）→ 載入模型 → 暖機一次     │  ← 慢、可能失敗：失敗就讓 main 回傳 Err
└──────────────────────────────────────┘
loop events.recv():
   Arrow 輸入 → &[u8]（不複製）
   （選用）順手把佇列裡已經到的也拿出來湊批次
   （選用）丟掉太舊的影像
   前處理 → Tensor → forward → 後處理（argmax／softmax）
   send_output(結果, metadata：seq、t_ns、pred、score、infer_us、batch、backend)
```

| 設計點 | 為什麼 | 本篇做法 |
|---|---|---|
| **模型載入放在事件迴圈前**（甚至放在 `DoraNode::init_from_env()` 之前） | 載入 safetensors／建 ONNX session 可能要幾十毫秒到幾秒。放進迴圈裡（例如第一張影像到了才載入）會讓第一張的延遲暴增，而且出錯時已經在處理資料了 | `Net::load()`、`Session::builder()…commit_from_file()` 都在 `init_from_env()` 之前。實測 log：兩個推論節點都先印出「模型載入」，之後 daemon 才印 `node is ready`、`all nodes are ready, starting dataflow`——dora 會等所有節點 ready 才開始送 timer，所以載入期間沒有影像被丟掉。（載入要很久時 daemon 會不會逾時，本篇沒測） |
| **暖機一次** | 第一次 forward 會配置記憶體、初始化執行緒池，比之後慢 | 用全 0 的假影像跑一次。實測 candle 載入 0.45 ms、暖機 2.83 ms，而穩定後每次呼叫 p50 0.38 ms |
| **Arrow 輸入 → tensor** | dora 的資料是 Arrow 陣列；模型要的是 `(B, C, H, W)` 的 f32 tensor | camera 用 `send_output_raw` 送 784 byte（接收端看到的是 `UInt8Array`）。推論節點 `let px: &[u8] = (&data).try_into()?` 拿到切片（不複製），再 `Tensor::from_slice(px, (b,1,28,28))` → `to_dtype(F32)` → `/255`。這裡有一次複製＋一次型別轉換，對 784 byte 無所謂；1080p 彩色影像（6 MB）就要考慮把縮放、正規化合成一次迴圈 |
| **前處理與訓練時一致** | 最常見的「正確率莫名其妙很低」原因 | `preprocess()` 放在 `lib.rs`，訓練程式、bench、推論節點共用同一個函式 |
| **輸出加 metadata** | 下游要知道「這個結果是哪一張影像的」「花了多久」「誰算的」 | 原樣帶回 `seq`、`t_ns`（擷取時間），加上 `pred`、`score`、`infer_us`、`batch`、`backend`。資料本體送 10 個機率（Float32），metadata 放摘要，下游只要 top-1 時不必解析陣列 |
| **批次** | 一次推多張可以攤平每次呼叫的固定成本（排程、配置記憶體、GPU kernel 啟動） | 「**順手批次**」：`recv()` 拿到一張後，用 `try_recv()` 把**已經在佇列裡**的也拿出來，最多 `BATCH` 張；佇列空了就馬上推論，**不為了湊滿批次而等待**。來得及時批次大小自然是 1（延遲最低），追不上時批次自動變大（吞吐量變高） |
| **CPU 執行緒數** | 推論節點和同一台機器上的其他節點搶 CPU；執行緒開太多反而變慢 | candle：環境變數 `RAYON_NUM_THREADS`（candle 的 CPU kernel 讀這個變數決定執行緒數）。ort：`SessionBuilder::with_intra_threads(n)`。本篇節點都設 1，理由見第 3 節的量測 |
| **背壓／丟幀** | 攝影機不會等模型。模型慢的時候，延遲和丟幀只能選一個 | 輸入用 `queue_size` 小的 `drop_oldest`（只留最近幾張）；或節點內 `MAX_AGE_MS`：擷取超過這麼久的影像不推論直接丟。不要用 `queue_policy: backpressure`（實測延遲 0.5 秒，而且照樣丟幀，見實測輸出 3） |

### 2. candle 與 ONNX Runtime（ort）比較

| | candle（`candle-core`／`candle-nn`） | ONNX Runtime（`ort` crate） |
|---|---|---|
| 是什麼 | Hugging Face 的純 Rust 張量／神經網路函式庫 | 微軟的 C++ 推論引擎；`ort` 是 Rust 綁定 |
| 模型怎麼來 | 用 Rust 寫出網路結構（本篇 `Net`），權重從 safetensors 讀；HF 上很多模型有現成的 candle 實作（`candle-transformers`） | 任何能匯出 ONNX 的框架（PyTorch `torch.onnx.export`、TF、sklearn…），**不用改 Rust 程式**就能換模型 |
| 建置 | 純 Rust，`cargo build` 就好；交叉編譯和其他 Rust crate 一樣 | 要連結 ONNX Runtime。`download-binaries` feature 會在建置時下載預編譯檔（本篇是 107 MB 的 `libonnxruntime.a`，放在 `~/.cache/ort.pyke.io/`），需要網路、C++ 標準函式庫（見常見錯誤 1、2） |
| 執行檔大小（本篇，release、沒 strip，都含 dora-node-api） | `infer-candle` 41.3 MB（沒用到模型的 `camera` 是 39.1 MB） | `infer-ort` 69.1 MB，多了約 30 MB 的 ONNX Runtime；動態連結 `libstdc++.so.6` |
| 本篇速度（batch 1，單執行緒，p50） | 0.24 ms／張（bench），節點內 0.38 ms | 0.03 ms／張（bench），節點內 0.08 ms |
| 正確率（整個測試集） | 98.09%（本篇自己訓練的模型） | 98.91%（Model Zoo 的模型） |
| 適合 | 想要單一 Rust 執行檔、好交叉編譯、模型是 HF 上常見架構、或要在 Rust 裡自己訓練／微調 | 模型來自 PyTorch 等外部流程、要最好的 CPU 效能、要用 TensorRT／OpenVINO 等加速器（execution provider） |

注意：兩邊**不是同一個模型**（結構和權重都不同），速度比較只能說明「在這個大小的 CNN 上，ONNX Runtime 的 CPU kernel 明顯比 candle 快」，不能推到其他模型。要公平比較，應該把同一份權重分別載入兩邊（例如 PyTorch 訓練後同時匯出 ONNX 與 safetensors）；本篇沒有做。

`mnist-12.onnx` 的輸入是固定的 `(1,1,28,28)`（模型檔裡寫死 batch 1），所以 `infer-ort` 只能一張一張推。想要批次就要用動態 batch 維度重新匯出模型。

另外，`bench` 量了 `mnist-12.onnx` 吃「0..255 原始像素」和「除以 255」兩種輸入：正確分別是 9,891 與 9,890 張，幾乎一樣，所以 `infer-ort` 直接送 0..255。這是這個模型的特性（本篇沒有去查它內部有沒有正規化層），換模型時一定要先用離線資料量一次。

### 3. 批次與執行緒：離線量測

`bench`（不是 dora 節點）在同一份測試集上量每次呼叫的延遲（各 2,000 張，扣掉前 20 次暖機）：

| candle | batch 1（每張） | batch 8（每次呼叫／每張） | batch 32（每次呼叫／每張） |
|---|---|---|---|
| `RAYON_NUM_THREADS=1` | 0.24 ms | 1.64 ms／0.205 ms | 6.41 ms／0.201 ms |
| `RAYON_NUM_THREADS=4` | 0.25 ms | 0.85 ms／0.106 ms | 2.85 ms／0.089 ms |
| `RAYON_NUM_THREADS=8` | 0.26 ms | 2.07 ms／0.259 ms | 4.98 ms／0.156 ms |

（表內都是 p50；p99 與 max 見實測輸出 1。）讀法：

- **batch 1 時執行緒數沒有幫助**（0.24～0.26 ms）：一張 28×28 的計算量太小，分給多條執行緒的成本比省下的還多。
- **批次大時，4 條執行緒最快**；8 條反而比 4 條慢——這台虛擬機有 8 個 vCPU，執行緒開滿就和系統、其他行程搶。dataflow 裡還有其他節點在跑，推論節點更不該開滿。
- 批次換來吞吐量，代價是延遲：單執行緒 batch 32 一次要 6.41 ms，最先到的那張要等整批算完。
- ort 在 batch 1、`ORT_THREADS` 1 與 4 的 p50 都是 0.03～0.04 ms，差別看不出來。

結論：小模型＋即時串流，推論節點設 1～2 條執行緒、batch 1（或順手批次）就好；大模型才需要細調。要量自己的模型時，**一定要在目標機器上量**，x86 的數字不能拿來估 ZYNQ。

### 4. 背壓與丟幀：模型追不上攝影機時

攝影機 200 張/秒，模型一次 20 ms（最多 50 次呼叫/秒），一定會有 3/4 的影像處理不到。重點是**丟哪些**、**延遲多大**：

| 策略 | 設定 | 預期 |
|---|---|---|
| `q10` | 預設 `queue_size` 10、`drop_oldest` | 佇列一直是滿的，每張都要先排隊，延遲 ＝ 排隊時間＋推論時間 |
| `q1` | `queue_size: 1` | 永遠只處理最新的那張，延遲最低 |
| `bp` | `queue_policy: backpressure`（上限 10×10＝100 筆） | 不主動丟，但上限一滿照樣丟，而且排隊最長 |
| `batch8` | 順手批次，`BATCH: 8` | 每次呼叫的固定成本被攤平，可能全部處理得完 |
| `age30` | 預設佇列＋節點內 `MAX_AGE_MS: 30` | 太舊的直接丟、不推論，丟得很快，所以佇列不會滿 |

注意 `SLOW_MS` 是「每次呼叫固定多 20 ms」，不論批次大小。這等於假設**固定成本占大部分**（像 GPU／NPU 上的 kernel 啟動、DMA 傳輸），所以對 `batch8` 特別有利；如果是 CPU 上的大模型，每多一張就多一份計算，批次的效果會小很多（見第 3 節）。實際結果在「實測輸出 3」。

對第 19～20 篇的「攝影機→辨識→顯示」，建議是：**推論節點的輸入用 `queue_size: 1`**（或很小的值），結果帶上 `t_ns`，顯示節點用它把框畫在對的那一幀上；要更高吞吐量再加順手批次。

### 5. dora 佇列丟幀怎麼看

`events.drain_drop_counts()` 回傳「上次呼叫以來，每個輸入被 dora 丟掉幾筆」（包括 scheduler 的 `queue_size` 淘汰，以及 zenoh ingress channel 滿了的丟棄）。本篇每處理一批就累加一次，結束時印出「dora 佇列丟棄 N」。下游再用 `seq` 算缺口，兩個數字對照就知道幀是在哪一段掉的。

### 6. 上 ZYNQ（aarch64）的考量——**本篇沒有實測，以下全是說明**

- **target**：ZYNQ UltraScale+（Cortex-A53）是 `aarch64-unknown-linux-gnu`；ZYNQ-7000（Cortex-A9）是 32 位元的 `armv7-unknown-linux-gnueabihf`（第 15 篇）。
- **candle**：純 Rust，用第 15 篇的 `cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28` 應該就能編（本篇沒有試）。為了交叉編譯方便，`Cargo.toml` 把 ort 放在 `onnx` feature 裡，`--no-default-features` 就只編 candle 的節點。armv7 上 candle 能不能編、速度如何，都沒有驗證。
- **ort**：`ort-sys` 2.0.0-rc.15 的預編譯清單（原始碼 `build/download/dist.tsv`）裡有 `aarch64-unknown-linux-gnu`，**沒有任何 armv7 Linux 的項目**。所以 ZYNQ-7000 要用 ONNX Runtime，就得自己從原始碼交叉編譯 ONNX Runtime，再用 `ORT_LIB_LOCATION` 指過去（或 `load-dynamic` feature 在執行時載入 `.so`）。aarch64 的預編譯檔需要的 glibc 版本是否符合 PetaLinux rootfs，本篇沒查。
- **記憶體與儲存**：本篇 x86 的 `infer-ort` 是 69 MB（沒 strip）。板子上的 rootfs／eMMC 空間有限，可以用第 15 篇的 `zynq` profile（strip、LTO）縮小，並考慮 candle（本篇在 x86 上比 ort 少約 28 MB）。注意那個 profile 用 `opt-level = "s"`，對推論這種計算密集的程式可能變慢，推論節點也許該維持 `opt-level = 3`——這點本篇沒有量。
- **速度**：Cortex-A53／A9 比這台 x86 虛擬機慢很多，還要算上影像縮放與色彩轉換。務必在板子上重跑 `bench` 再決定解析度、模型大小、`queue_size`。
- **FPGA 加速**：PL 端的 DPU（Vitis AI）不是 candle 或標準 ONNX Runtime 能直接用的；Vitis AI 有自己的 runtime（ONNX Runtime 有 Vitis AI execution provider，`ort` 也有 `vitis` feature）。這屬於第 16 篇「自訂硬體節點」的範圍，本篇不談，也沒有驗證。

## 完整範例

位置：[examples/lesson-17/](../../examples/lesson-17/)

```
examples/lesson-17/
├── Cargo.toml
├── .cargo/config.toml        # ⚠️ 只為了這台機器缺 libstdc++.so（見常見錯誤 2）
├── linkfix/libstdc++.so      # 同上：指向系統的 libstdc++.so.6 的 symlink
├── fetch-data.sh             # 下載 MNIST 與 mnist-12.onnx 並核對 sha256
├── dataflow.yml              # camera → infer-candle ＋ infer-ort → sink
├── dataflow-slow.yml         # 模型追不上攝影機：五種策略比較
├── run-demo.sh               # dora build → dora run N 秒，log 寫到 run/
├── data/                     # MNIST（t10k 測試集；train 訓練集執行完已刪，fetch-data.sh 可再抓）
├── models/
│   ├── mnist-cnn.safetensors # train 產生（187,496 byte）
│   └── mnist-12.onnx         # ONNX Model Zoo（26,143 byte）
├── run/                      # 實測 log
└── src/
    ├── lib.rs                # MNIST 讀檔、Net（candle CNN）、preprocess、統計
    └── bin/
        ├── train.rs          # 離線訓練（不是 dora 節點）
        ├── bench.rs          # 離線量測（不是 dora 節點）
        ├── camera.rs         # 假攝影機：MNIST 測試圖 → image
        ├── infer-candle.rs   # candle 推論節點
        ├── infer-ort.rs      # ONNX Runtime 推論節點
        └── sink.rs           # 統計：張數、正確率、序號缺口、端到端延遲
```

資料與模型來源：MNIST 從 `ossci-datasets.s3.amazonaws.com/mnist/` 下載；`mnist-12.onnx` 從 `github.com/onnx/models`（`validated/vision/classification/mnist/model/`）下載，sha256 寫在 `fetch-data.sh`。

### Cargo.toml

```toml
[package]
name = "lesson-17"
version = "0.1.0"
edition = "2024"

[features]
default = ["onnx"]
# ort（ONNX Runtime）另外開一個 feature：拿掉它（--no-default-features）就只編 candle，
# 交叉編譯到 ZYNQ 時很有用（見重點說明 6）。
onnx = ["dep:ort"]

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
candle-core = "0.11"
candle-nn = "0.11"
# 預設 feature 的 tls-native 會拉進 openssl-sys（要系統裝 libssl-dev＋pkg-config），
# 這台機器沒有，所以關掉預設 feature，改用 rustls 下載 ONNX Runtime 預編譯檔（見常見錯誤 1）。
ort = { version = "2.0.0-rc.15", optional = true, default-features = false, features = ["std", "download-binaries", "copy-dylibs", "tls-rustls", "api-24"] }

[[bin]]
name = "infer-ort"
required-features = ["onnx"]
```

### .cargo/config.toml

只為了這台機器（見常見錯誤 2），一般機器不需要。

```toml
# 這台機器只有 libstdc++ 的執行期檔（libstdc++.so.6），沒有開發用的 libstdc++.so 連結，
# 而 apt 鏡像當時回 500 裝不了 libstdc++-14-dev。ort 靜態連結 ONNX Runtime（C++）需要 -lstdc++，
# 所以在專案內放一個 symlink：linkfix/libstdc++.so -> /usr/lib/x86_64-linux-gnu/libstdc++.so.6。
# 正常的機器請直接 apt install g++（或 libstdc++-NN-dev），不需要這個檔案。
[target.x86_64-unknown-linux-gnu]
rustflags = ["-L", "native=<專案目錄>/linkfix"]
```

### src/lib.rs

```rust
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
```

### src/bin/train.rs

```rust
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
```

### src/bin/bench.rs

```rust
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
```

### src/bin/camera.rs

```rust
//! 假攝影機：每個 tick 從 MNIST 測試集取下一張 28×28 灰階圖送出（Arrow UInt8，784 byte）。
//! metadata 帶 seq（序號）、label（正確答案，只為了讓 sink 算正確率；真的攝影機當然沒有）、
//! t_ns（擷取時間，算端到端延遲用）、width/height。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_17::{H, Mnist, PIXELS, W, env_or, now_ns};

fn main() -> eyre::Result<()> {
    let dir: String = env_or("DATASET", "data".to_string());
    let ds = Mnist::load(&dir, "t10k")?;
    let max: u64 = env_or("MAX_FRAMES", u64::MAX);
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("image".to_owned());
    let mut seq = 0u64;
    println!("[camera] 載入 {} 張測試圖，MAX_FRAMES={max}", ds.len());
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                if seq >= max { continue; }
                let i = (seq as usize) % ds.len();
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq as i64));
                p.insert("label".into(), Parameter::Integer(ds.labels[i] as i64));
                p.insert("t_ns".into(), Parameter::Integer(now_ns() as i64));
                p.insert("width".into(), Parameter::Integer(W as i64));
                p.insert("height".into(), Parameter::Integer(H as i64));
                let img = ds.image(i);
                node.send_output_raw(out.clone(), p, PIXELS, |dst| dst.copy_from_slice(img))?;
                seq += 1;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[camera] 結束：送出 {seq} 張");
    Ok(())
}
```

### src/bin/infer-candle.rs

本篇的主角。

```rust
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
```

### src/bin/infer-ort.rs

```rust
//! ONNX Runtime（ort crate）推論節點：介面和 infer-candle 一樣（image → result），
//! 模型是 ONNX Model Zoo 的 mnist-12.onnx（輸入固定 (1,1,28,28)，所以一次只推一張）。
//!   MODEL        .onnx 路徑
//!   ORT_THREADS  ONNX Runtime 的 intra-op 執行緒數（預設 1）
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId,
                    get_integer_param};
use lesson_17::{Lat, PIXELS, env_or, top1};
use ort::{session::Session, value::TensorRef};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let model: String = env_or("MODEL", "models/mnist-12.onnx".to_string());
    let threads: usize = env_or("ORT_THREADS", 1usize);
    let t = Instant::now();
    // ort 的錯誤型別 Error<SessionBuilder> 不是 Send/Sync，不能直接用 ? 轉 eyre，所以先轉成字串
    let env = ort::init().with_name("dora-infer-ort").build().map_err(|e| eyre::eyre!("ort: {e}"))?;
    let mut sess = Session::builder(&env).map_err(|e| eyre::eyre!("ort: {e}"))?
        .with_intra_threads(threads).map_err(|e| eyre::eyre!("ort: {e}"))?
        .commit_from_file(&model).map_err(|e| eyre::eyre!("ort: {e}"))?;
    let input_name = sess.inputs()[0].name().to_string();
    println!("[ort] 模型 {model} 載入 {:.2} ms；輸入 {input_name}；ORT_THREADS={threads}", t.elapsed().as_secs_f64() * 1e3);

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("result".to_owned());
    let mut x = vec![0f32; PIXELS]; // 重複使用同一塊輸入 buffer，不每張配置
    let (mut n, mut n_queue_drop) = (0u64, 0u64);
    let mut lat = Lat::default();
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "image" => {
                let px: &[u8] = (&data).try_into()?;
                eyre::ensure!(px.len() == PIXELS, "影像大小 {} ≠ {PIXELS}", px.len());
                // mnist-12 直接吃 0..255 的 float（bench 實測 /255 與否正確率幾乎一樣）
                for (d, s) in x.iter_mut().zip(px) { *d = *s as f32; }
                let t = Instant::now();
                let outputs = sess.run(ort::inputs![TensorRef::from_array_view(([1usize, 1, 28, 28], &*x))?])?;
                let (_shape, logits) = outputs[0].try_extract_tensor::<f32>()?;
                let (pred, score) = top1(logits);
                let infer_ns = t.elapsed().as_nanos() as u64;
                lat.push(infer_ns);
                let pm = &metadata.parameters;
                let mut p = MetadataParameters::new();
                for k in ["seq", "label", "t_ns"] {
                    p.insert(k.into(), Parameter::Integer(get_integer_param(pm, k).unwrap_or(-1)));
                }
                p.insert("pred".into(), Parameter::Integer(pred as i64));
                p.insert("score".into(), Parameter::Float(score as f64));
                p.insert("infer_us".into(), Parameter::Integer((infer_ns / 1000) as i64));
                p.insert("batch".into(), Parameter::Integer(1));
                p.insert("backend".into(), Parameter::String("ort".into()));
                let logits = logits.to_vec();
                drop(outputs);
                node.send_output(out.clone(), p, logits)?; // 這裡送 logits（未做 softmax）
                n += 1;
                for (_, d) in events.drain_drop_counts() { n_queue_drop += d; }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[ort] 結束：推論 {n} 張，每次呼叫 {}；dora 佇列丟棄 {n_queue_drop}", lat.summary());
    Ok(())
}
```

### src/bin/sink.rs

```rust
//! 下游：每個推論節點各算一份統計——張數、正確率、序號缺口（被丟掉的幀）、端到端延遲（擷取→收到結果）。
use dora_node_api::{DoraNode, Event, Parameter, get_integer_param, get_string_param};
use lesson_17::{Lat, now_ns};
use std::collections::BTreeMap;

#[derive(Default)]
struct Stat { n: u64, correct: u64, gaps: u64, last_seq: i64, e2e: Lat, infer_us: u64, batch_sum: u64 }

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    while let Some(event) = events.recv() {
        let Event::Input { id, metadata, data } = event else {
            if matches!(event, Event::Stop(_)) { break; }
            continue;
        };
        let pm = &metadata.parameters;
        let probs: &[f32] = (&data).try_into()?;
        let (seq, label, t_ns) = (get_integer_param(pm, "seq").unwrap_or(-1),
            get_integer_param(pm, "label").unwrap_or(-1), get_integer_param(pm, "t_ns").unwrap_or(0));
        let pred = get_integer_param(pm, "pred").unwrap_or(-1);
        let score = match pm.get("score") { Some(Parameter::Float(f)) => *f, _ => f64::NAN };
        let s = stats.entry(id.to_string()).or_insert_with(|| Stat { last_seq: -1, ..Default::default() });
        if s.n < 3 {
            println!("[sink] {id} 第 {} 筆：seq={seq} label={label} pred={pred} score={score:.3} backend={} 資料 {} 個 f32",
                s.n + 1, get_string_param(pm, "backend").unwrap_or("?"), probs.len());
        }
        if s.last_seq >= 0 && seq > s.last_seq + 1 { s.gaps += (seq - s.last_seq - 1) as u64; }
        s.last_seq = seq;
        s.n += 1;
        if pred == label { s.correct += 1; }
        s.e2e.push(now_ns().saturating_sub(t_ns as u64));
        s.infer_us += get_integer_param(pm, "infer_us").unwrap_or(0) as u64;
        s.batch_sum += get_integer_param(pm, "batch").unwrap_or(1) as u64;
    }
    for (id, s) in stats.iter_mut() {
        println!("[sink] {id:<14}：{} 張，正確 {}（{:.2}%），序號缺口 {}，平均批次 {:.2}，端到端 {}",
            s.n, s.correct, s.correct as f64 * 100.0 / s.n.max(1) as f64, s.gaps,
            s.batch_sum as f64 / s.n.max(1) as f64, s.e2e.summary());
    }
    Ok(())
}
```

### dataflow.yml

```yaml
# 第 17 篇：假攝影機（MNIST 測試圖，200 張/秒）→ candle 與 ONNX Runtime 兩個推論節點 → sink
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    inputs:
      tick: dora/timer/millis/5
    outputs:
      - image

  - id: infer-candle
    build: cargo build --release --bin infer-candle
    path: target/release/infer-candle
    env:
      MODEL: models/mnist-cnn.safetensors
      BATCH: "1"
      RAYON_NUM_THREADS: "1"
    inputs:
      image:
        source: camera/image
        queue_size: 2        # 推論節點只需要「最近的」幾張，不要讓舊影像排長隊
    outputs:
      - result

  - id: infer-ort
    build: cargo build --release --bin infer-ort
    path: target/release/infer-ort
    env:
      MODEL: models/mnist-12.onnx
      ORT_THREADS: "1"
    inputs:
      image:
        source: camera/image
        queue_size: 2
    outputs:
      - result

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    inputs:
      candle: infer-candle/result
      ort: infer-ort/result
```

### dataflow-slow.yml

`q1`～`age30` 沒寫 `build:`，因為和 `q10` 用同一個執行檔，build 一次就夠。

```yaml
# 第 17 篇：模型太慢、追不上攝影機時的幾種策略。
# 五個 candle 節點都用 SLOW_MS=20（每次呼叫模型後多睡 20 ms，模擬重模型，上限約 50 次呼叫/秒），
# 攝影機一樣 200 張/秒，只有佇列設定、BATCH、MAX_AGE_MS 不同。
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    inputs:
      tick: dora/timer/millis/5
    outputs:
      - image

  - id: q10                    # 預設：queue_size 10、drop_oldest
    build: cargo build --release --bin infer-candle
    path: target/release/infer-candle
    env: { SLOW_MS: "20", RAYON_NUM_THREADS: "1" }
    inputs:
      image: camera/image
    outputs: [result]

  - id: q1                     # 只留最新一張
    path: target/release/infer-candle
    env: { SLOW_MS: "20", RAYON_NUM_THREADS: "1" }
    inputs:
      image: { source: camera/image, queue_size: 1 }
    outputs: [result]

  - id: bp                     # backpressure：不丟，緩衝上限 10×10＝100 筆
    path: target/release/infer-candle
    env: { SLOW_MS: "20", RAYON_NUM_THREADS: "1" }
    inputs:
      image: { source: camera/image, queue_size: 10, queue_policy: backpressure }
    outputs: [result]

  - id: batch8                 # 順手批次，最多 8 張一起推
    path: target/release/infer-candle
    env: { SLOW_MS: "20", RAYON_NUM_THREADS: "1", BATCH: "8" }
    inputs:
      image: { source: camera/image, queue_size: 10 }
    outputs: [result]

  - id: age30                  # 預設佇列，但擷取超過 30 ms 的影像直接丟
    path: target/release/infer-candle
    env: { SLOW_MS: "20", RAYON_NUM_THREADS: "1", MAX_AGE_MS: "30" }
    inputs:
      image: camera/image
    outputs: [result]

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    inputs:
      q10: q10/result
      q1: q1/result
      bp: bp/result
      batch8: batch8/result
      age30: age30/result
```

### run-demo.sh

```bash
#!/usr/bin/env bash
# 一鍵實測：dora build → dora run N 秒。用法：./run-demo.sh [dataflow.yml] [秒數]
set -uo pipefail
YML=${1:-dataflow.yml}; SECS=${2:-15}; TAG=$(basename "$YML" .yml)
cd "$(dirname "$0")"
DORA=${DORA:-dora}
mkdir -p run
$DORA build "$YML" > run/$TAG-build.log 2>&1 || { echo "dora build 失敗，見 run/$TAG-build.log"; exit 1; }
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/$TAG.log
$DORA run "$YML" --stop-after ${SECS}s >> run/$TAG.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/$TAG.log
```

### fetch-data.sh

```bash
#!/usr/bin/env bash
# 下載 MNIST（ossci-datasets 鏡像）與 ONNX Model Zoo 的 mnist-12.onnx，並核對 sha256（本篇實測時的值）。
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p data models
for f in train-images-idx3-ubyte train-labels-idx1-ubyte t10k-images-idx3-ubyte t10k-labels-idx1-ubyte; do
  [ -f data/$f ] || { curl -sSfL -o data/$f.gz https://ossci-datasets.s3.amazonaws.com/mnist/$f.gz && gunzip -f data/$f.gz; }
done
[ -f models/mnist-12.onnx ] || curl -sSfL -o models/mnist-12.onnx \
  https://github.com/onnx/models/raw/main/validated/vision/classification/mnist/model/mnist-12.onnx
sha256sum -c <<'SUM'
0fa7898d509279e482958e8ce81c8e77db3f2f8254e26661ceb7762c4d494ce7  data/t10k-images-idx3-ubyte
ff7bcfd416de33731a308c3f266cc351222c34898ecbeaf847f06e48f7ec33f2  data/t10k-labels-idx1-ubyte
ba891046e6505d7aadcbbe25680a0738ad16aec93bde7f9b65e87a2fc25776db  data/train-images-idx3-ubyte
65a50cbbf4e906d70832878ad85ccda5333a97f0f4c3dd2ef09a8a9eef7101c5  data/train-labels-idx1-ubyte
5c688690f8bacf667d4c2074af5ad0646ca328d7ab03eccf944a65b320171bdd  models/mnist-12.onnx
SUM
```

## 實際執行

```bash
cd examples/lesson-17   # 自 repo 根目錄
export PATH="$HOME/dora/bin:$PATH"   # 依你的 dora CLI 安裝位置調整
export RUSTUP_TOOLCHAIN=1.99.1               # 只有 rustup 沒設預設 toolchain 時才需要
./fetch-data.sh                              # MNIST ＋ mnist-12.onnx，核對 sha256
cargo build --release                        # 第一次會下載 ONNX Runtime（約 107 MB 的 .a）
./target/release/train 3                     # 訓練 → models/mnist-cnn.safetensors（約 4 分鐘）
RAYON_NUM_THREADS=1 ./target/release/bench candle
ORT_THREADS=1 ./target/release/bench ort
./run-demo.sh dataflow.yml 15                # → run/dataflow.log
./run-demo.sh dataflow-slow.yml 15           # → run/dataflow-slow.log
```

編譯時間：改了 `.cargo/config.toml` 的 rustflags 之後整個重新編譯（candle、ort、dora-node-api 全部），8 核虛擬機花了 3 分 08 秒。`.cargo/config.toml` 改好之後最後一次 `cargo build --release` 是 0 個 warning。

### 實測輸出 1：訓練與離線量測（2026-11-17 07:49～07:57，台北時間）

`run/train.log`：

```
[train] 訓練 60000 張、測試 10000 張，參數 46730 個
[train] epoch 1: 平均 loss 0.2131，測試集正確率 97.80%，累計 73.4 s
[train] epoch 2: 平均 loss 0.0668，測試集正確率 97.91%，累計 145.5 s
[train] epoch 3: 平均 loss 0.0492，測試集正確率 98.09%，累計 226.1 s
[train] 已存 models/mnist-cnn.safetensors
```

（權重初始化用的是 candle 的亂數，沒有固定種子，重跑一次的正確率會有些微差異。）

`run/bench.log`：

```
[bench candle] RAYON_NUM_THREADS=1 載入 0.49 ms
[bench candle] 測試集 10000 張正確 9809（98.09%）
[bench candle] batch  1：每次呼叫 p50 0.24 ms / p99 0.36 ms / max 4.92 ms，每張約 0.245 ms（2000 次）
[bench candle] batch  8：每次呼叫 p50 1.64 ms / p99 2.29 ms / max 4.62 ms，每張約 0.205 ms（250 次）
[bench candle] batch 32：每次呼叫 p50 6.41 ms / p99 7.08 ms / max 13.04 ms，每張約 0.201 ms（62 次）
[bench candle] RAYON_NUM_THREADS=4 載入 0.24 ms
[bench candle] 測試集 10000 張正確 9809（98.09%）
[bench candle] batch  1：每次呼叫 p50 0.25 ms / p99 0.46 ms / max 5.89 ms，每張約 0.247 ms（2000 次）
[bench candle] batch  8：每次呼叫 p50 0.85 ms / p99 1.94 ms / max 2.88 ms，每張約 0.106 ms（250 次）
[bench candle] batch 32：每次呼叫 p50 2.85 ms / p99 6.84 ms / max 16.06 ms，每張約 0.089 ms（62 次）
[bench candle] RAYON_NUM_THREADS=8 載入 0.23 ms
[bench candle] 測試集 10000 張正確 9809（98.09%）
[bench candle] batch  1：每次呼叫 p50 0.26 ms / p99 0.70 ms / max 6.14 ms，每張約 0.258 ms（2000 次）
[bench candle] batch  8：每次呼叫 p50 2.07 ms / p99 4.10 ms / max 8.86 ms，每張約 0.259 ms（250 次）
[bench candle] batch 32：每次呼叫 p50 4.98 ms / p99 6.97 ms / max 7.71 ms，每張約 0.156 ms（62 次）
[bench ort] ORT_THREADS=1 載入 24.92 ms
[bench ort] 輸入 Input3 Tensor { ty: Float32, shape: [1, 1, 28, 28], dimension_symbols: SymbolicDimensions(["", "", "", ""]) }
[bench ort] 輸出 Plus214_Output_0 Tensor { ty: Float32, shape: [1, 10], dimension_symbols: SymbolicDimensions(["", ""]) }
[bench ort] 像素 0..255：正確 9891（98.91%），batch 1 每次呼叫 p50 0.03 ms / p99 0.09 ms / max 2.35 ms
[bench ort] 像素 /255：正確 9890（98.90%），batch 1 每次呼叫 p50 0.04 ms / p99 0.08 ms / max 1.70 ms
[bench ort] ORT_THREADS=4 載入 17.65 ms
（輸入／輸出兩行同上，省略）
[bench ort] 像素 0..255：正確 9891（98.91%），batch 1 每次呼叫 p50 0.04 ms / p99 0.07 ms / max 8.73 ms
[bench ort] 像素 /255：正確 9890（98.90%），batch 1 每次呼叫 p50 0.04 ms / p99 0.07 ms / max 0.16 ms
```

「每張約」是 p50 除以批次大小。載入時間：candle 讀 187 KB 的 safetensors 不到 0.5 ms；ort 建 session（含圖最佳化）17～25 ms。

### 實測輸出 2：dataflow.yml（2026-11-17 07:58:34 開始，台北時間）

節錄 `run/dataflow.log`（dora 的 DEBUG 行省略）：

```
07:58:35 stdout infer-candle: [candle] 模型 models/mnist-cnn.safetensors 載入 0.45 ms、暖機 2.83 ms；BATCH=1 RAYON_NUM_THREADS=1 MAX_AGE_MS=0 SLOW_MS=0
07:58:35 stdout infer-ort: [ort] 模型 models/mnist-12.onnx 載入 19.18 ms；輸入 Input3；ORT_THREADS=1
07:58:35 INFO   camera: daemon node is ready
07:58:35 INFO   infer-ort: daemon node is ready
07:58:35 INFO   infer-candle: daemon node is ready
07:58:35 INFO   sink: daemon node is ready
07:58:35 INFO   [dora]: daemon all nodes are ready, starting dataflow
07:58:35 stdout camera: [camera] 載入 10000 張測試圖，MAX_FRAMES=18446744073709551615
07:58:35 stdout sink: [sink] ort 第 1 筆：seq=0 label=7 pred=7 score=1.000 backend=ort 資料 10 個 f32
07:58:35 stdout sink: [sink] ort 第 2 筆：seq=3 label=0 pred=0 score=1.000 backend=ort 資料 10 個 f32
07:58:35 stdout sink: [sink] ort 第 3 筆：seq=4 label=4 pred=4 score=1.000 backend=ort 資料 10 個 f32
07:58:35 stdout sink: [sink] candle 第 1 筆：seq=3 label=0 pred=0 score=0.999 backend=candle 資料 10 個 f32
07:58:35 stdout sink: [sink] candle 第 2 筆：seq=4 label=4 pred=4 score=0.997 backend=candle 資料 10 個 f32
07:58:35 stdout sink: [sink] candle 第 3 筆：seq=5 label=1 pred=1 score=0.998 backend=candle 資料 10 個 f32
2026-11-16T23:58:50.207196Z  INFO dora_daemon: stop-after timeout reached (15s) -> stopping dataflow
07:58:50 stdout camera: [camera] 結束：送出 2885 張
07:58:50 stdout sink: [sink] candle        ：2882 張，正確 2804（97.29%），序號缺口 0，平均批次 1.00，端到端 p50 0.87 ms / p99 2.13 ms / max 6.69 ms
07:58:50 stdout infer-ort: [ort] 結束：推論 2883 張，每次呼叫 p50 0.08 ms / p99 0.13 ms / max 1.43 ms；dora 佇列丟棄 2
07:58:50 stdout sink: [sink] ort           ：2883 張，正確 2832（98.23%），序號缺口 2，平均批次 1.00，端到端 p50 0.56 ms / p99 1.74 ms / max 7.57 ms
07:58:50 stdout infer-candle: [candle] 結束：推論 2882 張／2882 次呼叫（平均每批 1.00 張），每次呼叫 p50 0.38 ms / p99 0.99 ms / max 2.54 ms；過期丟棄 0，dora 佇列丟棄 3
07:58:50 INFO   [dora]: daemon dataflow finished on machine `01a1e6cc-5c9d-7394-9c8d-4367a96a4b21`
dora run 結束碼 0
```

（daemon 自己的 log 用 UTC，`2026-11-16T23:58:50Z` 就是台北時間 11-17 07:58:50。）

讀法：

- **模型載入在 `node is ready` 之前**，和第 1 節說的一樣：dataflow 是等兩個模型都載入好才開始的。
- 15 秒送出 2,885 張（timer 5 ms，理論上 3,000 張；差的部分本篇沒有追）。candle 收到 2,882 張、ort 收到 2,883 張，**少掉的 3 張和 2 張都在一開始**：candle 的第 1 筆是 seq 3，ort 跳過了 seq 1、2，和兩個節點的「dora 佇列丟棄 3／2」對得上。開頭幾張影像幾乎同時到、`queue_size: 2` 放不下，是最可能的原因，但沒有進一步確認。之後兩邊都沒有再掉幀（candle 的「序號缺口 0」是從它收到的第一張 seq 3 開始算的）。
- 正確率 97.29%／98.23% 是**前 2,885 張**的結果，和整個測試集的 98.09%／98.91% 不同是因為樣本不同。
- 端到端（camera 擷取 → sink 收到結果）p50 candle 0.87 ms、ort 0.56 ms；其中推論本身分別是 0.38 ms 和 0.08 ms，其餘是 dora 傳兩次訊息、前後處理和 metadata。節點內的推論時間比 bench 慢（0.38 vs 0.24 ms），可能是同時有 4 個行程在跑，本篇沒有拆開量。

### 實測輸出 3：dataflow-slow.yml，模型追不上攝影機（2026-11-17 07:58:58 開始，台北時間）

```
07:59:14 stdout camera: [camera] 結束：送出 2889 張
07:59:14 stdout sink: [sink] age30         ：696 張，正確 681（97.84%），序號缺口 2181，平均批次 1.00，端到端 p50 48.39 ms / p99 51.71 ms / max 73.67 ms
07:59:14 stdout sink: [sink] batch8        ：2882 張，正確 2804（97.29%），序號缺口 0，平均批次 4.38，端到端 p50 32.93 ms / p99 43.98 ms / max 54.92 ms
07:59:14 stdout sink: [sink] bp            ：698 張，正確 682（97.71%），序號缺口 2084，平均批次 1.00，端到端 p50 518.73 ms / p99 541.81 ms / max 553.27 ms
07:59:14 stdout sink: [sink] q1            ：697 張，正確 680（97.56%），序號缺口 2185，平均批次 1.00，端到端 p50 23.51 ms / p99 29.85 ms / max 53.86 ms
07:59:14 stdout sink: [sink] q10           ：696 張，正確 676（97.13%），序號缺口 2177，平均批次 1.00，端到端 p50 68.60 ms / p99 76.85 ms / max 99.19 ms
07:59:14 stdout bp: [candle] 結束：推論 699 張／699 次呼叫（平均每批 1.00 張），每次呼叫 p50 20.52 ms / p99 21.09 ms / max 25.14 ms；過期丟棄 0，dora 佇列丟棄 2090
07:59:14 stdout q10: [candle] 結束：推論 697 張／697 次呼叫（平均每批 1.00 張），每次呼叫 p50 20.51 ms / p99 24.09 ms / max 32.61 ms；過期丟棄 0，dora 佇列丟棄 2182
07:59:14 stdout q1: [candle] 結束：推論 698 張／698 次呼叫（平均每批 1.00 張），每次呼叫 p50 20.51 ms / p99 21.11 ms / max 32.25 ms；過期丟棄 0，dora 佇列丟棄 2190
07:59:14 stdout age30: [candle] 結束：推論 697 張／697 次呼叫（平均每批 1.00 張），每次呼叫 p50 20.52 ms / p99 21.38 ms / max 42.98 ms；過期丟棄 2184，dora 佇列丟棄 0
07:59:14 stdout batch8: [candle] 結束：推論 2886 張／668 次呼叫（平均每批 4.32 張），每次呼叫 p50 21.34 ms / p99 23.32 ms / max 33.46 ms；過期丟棄 0，dora 佇列丟棄 0
dora run 結束碼 0
```

整理成表（張數是 sink 收到的）：

| 節點 | 策略 | 處理張數 | 丟在哪裡 | 端到端 p50 | p99 |
|---|---|---|---|---|---|
| q1 | `queue_size: 1` | 697 | dora 佇列 2,190 | **23.51 ms** | 29.85 ms |
| age30 | 預設佇列＋`MAX_AGE_MS: 30` | 696 | 節點內過期 2,184、dora 佇列 0 | 48.39 ms | 51.71 ms |
| q10 | 預設（`queue_size` 10） | 696 | dora 佇列 2,182 | 68.60 ms | 76.85 ms |
| bp | `queue_policy: backpressure` | 698 | dora 佇列 2,090 | **518.73 ms** | 541.81 ms |
| batch8 | 順手批次 `BATCH: 8` | **2,882** | 沒有丟（dora 佇列 0） | 32.93 ms | 43.98 ms |

觀察：

- 單張推論的四個節點都只處理了約 697 張（≈ 15 秒 × 每秒 50 次呼叫 − 啟動），**丟幀數量幾乎一樣**；差別全在延遲。`q1` 的延遲最低，就是「只處理最新的那張」；`bp` 雖然名字叫 backpressure，**照樣丟了 2,090 張**（上限一滿就丟最舊的），延遲卻是 0.5 秒——對即時影像是最差的選擇。dora 原始碼在這種情況會用 `tracing::error!` 記一筆「hit hard cap」，但本篇的 log 裡沒有看到這行（推測是節點沒有輸出 tracing log，沒有確認）。
- `age30` 的「dora 佇列丟棄 0」：因為過期的影像直接 `continue`、不推論也不睡，節點把佇列消化得很快，佇列不會滿；丟幀改成發生在節點裡（2,184 張）。延遲 48 ms 比門檻 30 ms 多，是因為檢查年齡是在**推論之前**，再加上 20 ms 推論。
- `q10` 的延遲 68.60 ms 比「10 張 × 20 ms ＝ 200 ms」的直覺估算小很多，本篇沒有查清楚原因（`drain_drop_counts` 的說明提到丟棄也可能發生在 zenoh ingress channel，可能和這有關）。
- `batch8` 平均每批 4.32 張、每次呼叫 21.34 ms，等於每秒處理約 200 張，**追上了攝影機**，延遲 33 ms。但這是因為 `SLOW_MS` 是「每次呼叫固定 20 ms」，對批次特別有利（見第 4 節），真實模型要自己量。另外 `batch8` 推論了 2,886 張，sink 只收到 2,882 張，差的 4 張沒有逐筆追查。

## 常見錯誤

1. **`ort` 預設 feature 拉進 OpenSSL**（實測，`run/openssl-error.log`）：
   ```
   warning: openssl-sys@0.9.118: Could not find directory of OpenSSL installation, and this `-sys` crate cannot proceed without this knowledge. …
   error: failed to run custom build command for `openssl-sys v0.9.118`
   ```
   `ort` 的預設 feature 含 `tls-native`（下載 ONNX Runtime 用 native-tls → `openssl-sys`），需要系統的 `libssl-dev` 和 `pkg-config`。本篇改成 `default-features = false`，自己挑 `std`、`download-binaries`、`copy-dylibs`、`tls-rustls`、`api-24`（見 Cargo.toml）。交叉編譯到 ZYNQ 時也比較不麻煩。
2. **連結 ONNX Runtime 時找不到 `libstdc++`**（實測，`run/libstdcxx-error.log`）：
   ```
   error: linking with `cc` failed: exit status: 1
     = note: rust-lld: error: unable to find library -lstdc++
   ```
   ONNX Runtime 是 C++ 寫的，靜態連結時要 `-lstdc++`。這台機器只有執行期的 `libstdc++.so.6`，沒有開發用的 `libstdc++.so`，而 `apt-get install libstdc++-14-dev` 時 apt 鏡像回 `500 Internal Server Error`。本篇在專案內放一個 symlink（`linkfix/libstdc++.so → /usr/lib/x86_64-linux-gnu/libstdc++.so.6`），再用 `.cargo/config.toml` 的 `rustflags = ["-L", "native=…/linkfix"]` 讓連結器找得到。**正常的機器請直接 `apt install g++`**，不要學這個繞法。注意改 rustflags 會讓整個專案重新編譯（本篇 3 分 08 秒）。
3. **前處理和訓練時不一致**：例如訓練時 `/255`，推論時送 0..255；或把 RGB／BGR、HWC／CHW 弄反。症狀是「不會當掉，但正確率很低」。本篇把 `preprocess()` 放在 `lib.rs` 讓所有程式共用，並且用 `bench` 在離線資料上先量過正確率，才放進 dataflow。ONNX 模型則要查清楚它要的輸入範圍——本篇的 `mnist-12.onnx` 碰巧兩種都對（98.91% vs 98.90%），大多數模型不會這麼寬容。
4. **把模型載入放進事件迴圈**（例如收到第一張影像才 `load`）：第一張的延遲多出整個載入時間；載入失敗時節點已經在收資料。本篇的寫法是載入、暖機都在 `init_from_env()` 之前，失敗就讓 `main` 回傳 `Err`。
5. **ONNX 模型的 batch 維度是固定的**：`mnist-12.onnx` 的輸入是 `[1, 1, 28, 28]`（bench 印出來的 `shape`），送 `(8,1,28,28)` 進去會出錯（本篇沒有實際送，錯誤訊息沒驗證）。想批次就重新匯出成動態 batch。先用 `session.inputs()` 印出形狀再寫程式。
6. **`ort` 的某些錯誤不能直接 `?` 轉 `eyre`**（實測，另開一個測試用的 bin 編譯後刪掉）：
   ```
   error[E0277]: `?` couldn't convert the error: `NonNull<OrtSessionOptions>: Sync` is not satisfied
   error[E0277]: `?` couldn't convert the error: `(dyn std::any::Any + 'static): Send` is not satisfied
   ```
   出錯的是 `.with_intra_threads(1)?` 這種 `SessionBuilder` 設定方法：它們回傳 `ort::Error<SessionBuilder>`（錯誤裡帶著 builder，方便 `recover()`），不符合 `eyre` 要的 `Send + Sync`。同一個測試裡 `ort::init()…build()?`、`Session::builder(&env)?` 沒有報錯。本篇在建 session 的整串呼叫都用 `.map_err(|e| eyre::eyre!("ort: {e}"))` 轉成字串；`session.run()`、`TensorRef::from_array_view()` 則直接 `?`（可以編譯）。
7. **以為 `queue_policy: backpressure` 就不會丟幀**：實測輸出 3 裡 `bp` 照樣丟了 2,090 張，延遲還是最大的。上游比下游快的時候，**一定**會丟，只能選丟在哪裡。
8. **推論執行緒開太多**：bench 裡 `RAYON_NUM_THREADS=8` 在 batch 8 時（2.07 ms）比 1 條（1.64 ms）還慢。dataflow 裡多個節點共用 CPU，推論節點要明確設定執行緒數，不要讓它預設用滿所有核心。

## 小練習

**題目**：做一個「**信心門檻＋標註顯示**」節點，為第 19～20 篇的顯示端預習。新增 `annotate` 節點，同時訂閱 `camera/image` 和 `infer-candle/result`：

1. 用 metadata 的 `seq` 把結果對回原本的影像（兩個輸入的到達順序不保證，要自己暫存最近幾張影像）。
2. `score` 低於 0.9 的結果標成「不確定」，統計不確定的比例，以及不確定的那些裡面答錯的比例。
3. 每 200 張把影像畫成 28 行 ASCII（像素 > 128 印 `#`，其他印 `.`），下面印出 `pred`、`score`、`label`。

提示：

- 影像暫存用 `BTreeMap<i64, Vec<u8>>`（key 是 seq），收到結果就 `remove`；另外要清掉太舊的 key（例如比最新的 seq 小 50 以上），不然推論節點丟幀時，那些永遠等不到結果的影像會一直累積。
- `score` 是 `Parameter::Float`，`dora-node-api` 沒有 `get_float_param`，可以照 `sink.rs` 的寫法 `match pm.get("score")`。
- 預期：信心低的結果答錯的比例應該明顯比較高。把門檻改成 0.5、0.99 各跑一次比較——**數字一定要自己跑出來**，不要用猜的。
- 加分：把 `infer-candle` 換成 `infer-ort`（只改 dataflow.yml 的 `source`），`annotate` 一行都不用改。這就是「推論節點輸出格式要統一」的好處。

## 本篇沒有驗證成功的部分

- **ZYNQ／aarch64**：沒有交叉編譯推論節點，沒有用 qemu 跑，也沒有上板子。第 6 節的內容（candle 可以用 cargo-zigbuild 編、ort 沒有 armv7 預編譯檔、`opt-level` 的影響、Vitis AI）是依文件與 `ort-sys` 原始碼整理的說明，速度、記憶體、glibc 相容性都沒有量。這台機器的 1.99.1 toolchain 目前也沒有安裝 aarch64 target。
- **GPU／其他 execution provider**：沒有 GPU，candle 的 CUDA／Metal 和 ort 的 CUDA／TensorRT／OpenVINO 都沒測。
- **公平的效能比較**：candle 和 ort 用的是不同的模型（不同結構、不同權重），第 2 節的速度差只代表這兩個模型。
- **`candle-onnx`**（用 candle 直接讀 .onnx）、量化（int8）模型、動態 batch 的 ONNX 模型：都沒有做。
- **幾個沒追查的數字**：camera 15 秒送出 2,885 張而不是 3,000 張；開頭 2～3 張被 dora 佇列丟掉的確切原因；`q10` 的延遲為什麼比估算小；`batch8` 推論 2,886 張但 sink 只收到 2,882 張；`bp` 的 hard cap error log 為什麼沒出現。依「同一步卡兩次就停」的原則沒有繼續追。
- **模型載入很慢時 daemon 會不會等**：本篇的模型載入都在 25 ms 以內，沒有測幾十秒的大模型。

收尾：`cargo clean`（`target/` 約 3.5 GB 已刪除）；刪除 ONNX Runtime 下載快取 `~/.cache/ort.pyke.io/`（107 MB，下次 `cargo build` 會重新下載）；刪除 MNIST 訓練集（約 47 MB，`fetch-data.sh` 可再抓）。保留：原始碼、`models/`（mnist-cnn.safetensors 187,496 byte、mnist-12.onnx 26,143 byte）、`data/` 的測試集（約 7.9 MB）、`run/` 的 log。刪除本篇執行時留在 `/dev/shm` 的 zenoh 殘檔（12 KB）；確認沒有殘留的 dora 行程。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 16 篇](lesson-16.md)
- 下一課：[第 18 篇 →](lesson-18.md)
- 本課範例：[examples/lesson-17/](../../examples/lesson-17/)
