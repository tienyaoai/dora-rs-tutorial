# dora-rs 教學 第 20 篇（最終篇）：綜合專題（下）——攝影機 → 辨識 → 顯示：顯示節點、完整管線整合與 ZYNQ 部署收尾

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0），rustc／cargo 1.99.1，`candle-core` 0.11.1（沿用第 17 篇），`png` 0.18.1，`minifb` 0.28.0（選用 feature），ffmpeg 7.1.5（系統內建），cargo-zigbuild 0.24.0＋ziglang 0.17.0，qemu-user 10.0.13 與 `file` 5.46（都是從 Debian 13 的 .deb 用 `dpkg-deb -x` 解到一個本機資料夾，沒有安裝進系統）。驗證日期：2026-11-20（台北時間），只在 x86_64 Linux 虛擬機（8 核、沒有 GPU、**沒有攝影機、沒有 `/dev/fb0`、沒有 ZYNQ 板**；有 Xvfb 虛擬 X 螢幕）上跑。本篇把第 19 篇的 `sink` 換成 **`display` 顯示節點**：把原始畫面放大 2 倍、畫上外框（綠＝對、紅＝錯、黃＝找不到）、上方寫辨識結果、下方寫延遲與幀率，同一張影格可同時送到五種輸出——`mp4`（餵 ffmpeg 壓 H.264）、`png`、`ascii`（終端預覽）、`fb`（Linux framebuffer）、`window`（minifb 真視窗）。`camera`、`detect` 與第 19 篇逐字相同（`diff` 確認）。實測摘要（全部實跑）：（1）合成來源 600 張 → 顯示 600 張、0 丟幀，正確 585/600（97.50%），顯示幀率 30.39 張/秒，「擷取→display 收到」p50 1.70 ms／p99 4.36 ms，「擷取→畫面送出（含寫進 ffmpeg）」p50 3.11 ms／p99 19.84 ms，畫一張 p50 0.55 ms；輸出的 `display.mp4` 用 `ffprobe` 數到 600 幀、640×480、20.0 秒；（2）影片檔來源＋寫到「假的 framebuffer」（一般檔案，800×600、32 bpp）300 張，正確 297/300，擷取→畫面送出 p50 2.51 ms／p99 3.63 ms，再把那個檔案用 ffmpeg 轉回 PNG，畫面正確；（3）`--features window` 在 Xvfb 上真的開出 minifb 視窗跑 300 張（30.53 張/秒），並用 `x11grab` 截圖存證；（4）**交叉編譯到 aarch64 成功**（`cargo zigbuild --profile zynq --target aarch64-unknown-linux-gnu.2.28`，3 分 40 秒），`file` 確認三個都是 `ELF 64-bit LSB pie executable, ARM aarch64`、glibc 符號最高 `GLIBC_2.28`、各約 12 MB；（5）**用 qemu-user 模擬跑完整管線**（官方 aarch64 dora CLI＋三個 aarch64 節點，framebuffer 改 RGB565）：顯示 298/300 張，模型 p50 8.88 ms、擷取→畫面送出 p50 16.70 ms——**這是模擬器的速度，不代表 Cortex-A53**；（6）`ffmpeg -f v4l2 -i /dev/video0` 設定有跑，但沒有裝置，得到預期的錯誤。**沒驗證**：真攝影機、真 `/dev/fb0`、真 ZYNQ 板上的速度與延遲、DMA 影像節點、FPGA 加速、SDL／DRM 顯示。

> **關於日誌：** 本篇引用的 `run/*.log`、`run/note-first-run.txt`、`display.mp4`、PNG 截圖等執行產物**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。

> **本 repo 說明：** 本篇範例依賴 `examples/lesson-19/`（路徑依賴）與 `examples/lesson-17/`（模型、`data/` 測試集；請先執行 `examples/lesson-17/fetch-data.sh`）。`dataflow-file.yml` 需要第 19 篇產生的 `examples/lesson-19/clip.mp4` 與 `clip.labels`（編好第 19 篇 release 後執行 `./make-clip.sh 300`，需要 `ffmpeg`）。`run-aarch64.sh` 需要自備官方 aarch64 版 `dora` 與 sysroot，請以環境變數 `DORA`、`QEMU_LD_PREFIX` 指定；課文用到的 qemu-user 與 `file` 是作者從 .deb 解壓到本機資料夾的，未納入本 repo。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 19 篇](lesson-19.md)
- 下一課：無——第 20 篇是**最終篇**，系列到此結束
- 本課範例：[examples/lesson-20/](../../examples/lesson-20/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [常見錯誤](#常見錯誤)
- [小練習](#小練習)
- [本篇沒有驗證成功的部分](#本篇沒有驗證成功的部分)
- [20 篇總複習／後續學習方向（接到 FPGA CNN 加速）](#20-篇總複習後續學習方向接到-fpga-cnn-加速)
## 重點說明

第 19 篇做了「攝影機 → 辨識 → 結果輸出」的前半段，本篇把最後一段換成**顯示**，整條管線就完整了；接著收尾到 ZYNQ：交叉編譯、怎麼換攝影機、延遲預算怎麼估，最後做 20 篇的總複習。

```
┌─────────────┐ image（mono8, W×H, seq, t_ns） ┌──────────────┐ result（10×f32＋外框＋時間）┌─────────────────────────┐
│ camera      │ ──────────────────────────────▶ │ detect       │ ──────────────────────────▶ │ display                 │
│ synth/ffmpeg│   queue_size 1                  │ 找框＋CNN    │   queue_size 4              │ 配對 seq → 畫框＋字 →   │
└─────────────┘ ──────────────────────────────────────────────────────────────────────────▶ │ mp4/png/ascii/fb/window │
                  image（同一份原始畫面，queue_size 4，先到先等）                             └─────────────────────────┘
```

### 1. 顯示節點的工作：配對 → 畫 → 輸出

`display` 有兩個輸入：`camera/image`（原始灰階畫面）與 `detect/result`（機率＋外框）。兩者是**分開到達**的：camera 同時把畫面送給 detect 和 display，所以 display 通常先收到畫面，約 1～2 ms 後才收到同一張的辨識結果。做法和第 19 篇的 sink 一樣**用 metadata 的 `seq` 配對**：

1. 收到 `image`：複製一份放進最多 8 張的小緩衝（`VecDeque`）。
2. 收到 `result`：把緩衝裡 `seq` 比它舊的畫面丟掉，取出同一個 `seq` 的畫面；找不到就記一筆「找不到對應畫面」。
3. 畫：灰階 → RGB、最近鄰放大 `SCALE` 倍 → 外框 → 上方半透明黑條寫 `#seq PRED d xx.x%  LABEL d OK/NG` → 下方寫 `E2E`（擷取到 display 收到）、`DET`（detect 內部）、`FPS`。
4. 送到 `OUTPUT` 列出的每一種輸出，量「畫圖」與「輸出」各花多久。

另一種設計是讓 detect 把畫面「連同結果」一起轉送，display 就只要一個輸入、不用配對；代價是 detect 每張要多送一次 76.8 KB（dora 會再配一塊共享記憶體）。本篇沿用第 19 篇的配對法，把「辨識」和「顯示」解耦：detect 跟不上時（`queue_size: 1` 丟幀），display 照樣拿得到每一張原始畫面，未來要做「每張都顯示、結果沿用上一筆」也很容易。

**為什麼不用字型庫**：文字用自己寫的 5×7 點陣字（`glyph()`，只收 0～9、用到的英文字母與幾個符號）。好處是**零依賴**：不用 freetype、fontconfig，交叉編譯到 ZYNQ 時不會多出 C 函式庫。缺點是只能寫英數字；要中文就得用 `ab_glyph`／`fontdue` 之類的 crate 加字型檔，本篇沒有做。

### 2. 沒有螢幕時怎麼「驗證顯示」：五種輸出

box 是雲端虛擬機，沒有實體螢幕、沒有攝影機，但「畫面畫得對不對」必須能查證。所以同一張 RGB 影格做成可以疊加的五種輸出（`OUTPUT=mp4,png,ascii` 這樣逗號分隔）：

| `OUTPUT` | 做法 | 本篇怎麼驗證 | 換成真螢幕時 |
|---|---|---|---|
| `mp4` | 開 ffmpeg 子行程，rgb24 影格寫進它的 stdin，壓成 H.264 | `ffprobe` 數幀數、抽幀轉 PNG 目視 | 不用換；當「行車紀錄器」很實用 |
| `png` | 每 `PNG_EVERY` 張存一張 RGB PNG | 直接看圖 | 不用換 |
| `ascii` | 縮成 64×24 字元印到 log，外框格子標 `G`／`R` | 看 `dora run` 的 log | ssh 進板子、沒接螢幕時的快速檢查 |
| `fb` | 寫進 Linux framebuffer（32 bpp BGRX 或 16 bpp RGB565） | 用**一般檔案**假裝 framebuffer，跑完再用 ffmpeg 把它當 rawvideo 轉成 PNG | `FB_DEV=/dev/fb0`：解析度、bpp、stride 改從 `/sys/class/graphics/fb0/` 讀 |
| `window` | minifb 開 X11 視窗（`--features window`） | 在 box 的 Xvfb 虛擬螢幕 `:12` 上跑，`ffmpeg -f x11grab` 截圖 | 桌機／有桌面的板子：`DISPLAY=:0` |

ffmpeg 子行程有兩個要點：（1）跑完要**先關掉它的 stdin 再 `wait()`**，ffmpeg 才知道影片結束、把 mp4 的索引（moov）寫完；直接 kill 會得到打不開的 mp4。（2）餵 ffmpeg 是寫 pipe，pipe 滿了 `write_all` 會阻塞，所以 mp4 輸出的「輸出」時間偶爾會跳到十幾 ms（見實測輸出 1 的 p99）。

### 3. 真螢幕要怎麼換

- **一般 Linux 桌機／筆電（X11）**：`cargo build --release --features window`，`OUTPUT=window`。minifb 0.28 預設同時編 X11 與 Wayland 後端，執行時用 dlopen 載入 `libX11` 等函式庫，所以**編譯時不需要 X11 的 -dev 套件**（本機實測編譯 10.26 秒成功）。從 ssh 跑時要在 dataflow 的 env 設 `DISPLAY`。
- **SDL2**（`sdl2` crate）：可以用硬體加速縮放、處理鍵盤事件，但要系統裝 libSDL2，交叉編譯時 sysroot 裡也要有。本篇**沒有做**。
- **ZYNQ UltraScale+（例如 ZCU104、Kria KV260 的 DisplayPort／HDMI）**：PetaLinux 的顯示走 DRM（`zynqmp-dpsub`），核心開了 `CONFIG_DRM_FBDEV_EMULATION` 時會有 `/dev/fb0`，本篇的 `OUTPUT=fb FB_DEV=/dev/fb0` 直接可用，不需要 X11。注意：
  - 要有寫 `/dev/fb0` 的權限（root 或 `video` 群組）。
  - 文字主控台（fbcon）的游標會蓋掉你的畫面：`echo 0 > /sys/class/graphics/fbcon/cursor_blink`，或乾脆不開 getty。
  - 本篇的 `fb` 是「直接寫進正在顯示的緩衝」，沒有等垂直同步，可能看到畫面撕裂（tearing）。正式產品建議用 DRM/KMS 的 dumb buffer＋page flip（`drm` crate），本篇**沒有做**。
- **ZYNQ-7000（例如 ZedBoard、PYNQ-Z2）**：PS 本身沒有顯示控制器，要在 PL 放 VDMA＋Video Timing Controller＋HDMI 輸出 IP，再配上對應的 Linux 驅動才會出現 `/dev/fb0` 或 DRM 裝置。這是 Vivado 設計的一部分，本篇**沒有做也沒有驗證**。
- **完全不接螢幕**：用 `mp4` 存證，或改成 ffmpeg 推 RTP／RTSP 串流到另一台電腦看（本篇沒有測）。

`fb` 輸出寫入時是**一列一列 `pwrite`**（`FileExt::write_all_at(row, y × stride)`）：framebuffer 每列的 byte 數（stride）常常大於「寬 × 每像素 byte 數」（硬體要求對齊），所以不能把整張畫面一次寫進去。本篇用 800×600 的假 framebuffer、640×480 的畫布，就是在驗證這個「貼到左上角、其餘不動」的行為。

### 4. 完整管線整合：介面不變，節點隨便換

第 19 篇把介面定在 metadata 上，本篇就是在驗證這件事：**`camera`、`detect` 一行都沒改**（直接複製檔案，`diff` 無差異），只把第三個節點從 sink 換成 display，dataflow.yml 改 10 行左右。一條管線、四種組合都跑過：

| dataflow | 來源 | 顯示 | 架構 |
|---|---|---|---|
| `dataflow.yml` | 合成 | mp4＋png＋ascii | x86_64 |
| `dataflow-file.yml` | H.264 影片檔（第 19 篇的 `clip.mp4`） | 假 framebuffer（32 bpp）＋png | x86_64 |
| `dataflow-window.yml` | 合成 | minifb 視窗＋png | x86_64（Xvfb） |
| `dataflow-aarch64.yml` | 合成 | 假 framebuffer（16 bpp）＋png＋ascii | aarch64（qemu 模擬） |
| `dataflow-v4l2.yml` | `/dev/video0`（沒有裝置，預期失敗） | png | x86_64 |

### 5. ZYNQ 部署：交叉編譯（實際做了）

沿用第 15 篇的方法：cargo-zigbuild、目標 `aarch64-unknown-linux-gnu.2.28`（相容 glibc 2.28 以上的 PetaLinux）、`zynq` profile（`opt-level="s"`、fat LTO、`panic="abort"`、strip）。

```bash
rustup target add aarch64-unknown-linux-gnu        # 這台的 1.99.1 本來沒有裝（第 17 篇文末有記）
cargo zigbuild --profile zynq --target aarch64-unknown-linux-gnu.2.28 --bins
```

- 預設 feature **不含** `window`，所以 aarch64 版不會拉進 minifb／X11；ZYNQ 上用 `fb`。
- `lesson-17` 在第 19 篇就已經用 `default-features = false` 關掉 ort（ONNX Runtime），所以整條依賴樹只有 candle＋dora，zig 編得過（`ring` 的 C 程式碼由 zig 編）。
- 產物：三個節點各約 12 MB（x86_64 的一般 release 版各約 39～41 MB；兩者 profile 和架構都不同，不能直接比）。
- dataflow 裡**不寫 `build:`**、`path:` 指向交叉編譯產物（第 15 篇重點 4）。上板子時改成板子上的絕對路徑，自己 `scp` 過去（dora 1.1.0 的 `distribute: scp` 不會幫你傳，第 15 篇查過）。

**在 qemu 上試跑**：沒有板子，用 qemu-user＋binfmt_misc 讓 x86_64 主機能直接執行 aarch64 ELF（`F` 旗標：註冊時就把 qemu 開好，之後換工作目錄也找得到），`QEMU_LD_PREFIX` 指到第 15 篇的 glibc 2.41 sysroot，由**官方 aarch64 dora CLI**（它自己也跑在 qemu 裡）啟動三個 aarch64 節點。功能完全正常，但 qemu 是用 x86 指令一條一條翻譯 ARM 指令，**速度不代表 Cortex-A53**（可能更快也可能更慢），所以實測輸出 5 的延遲只能證明「跑得起來」。

### 6. 把 camera 換成真的影像來源（說明；**本篇沒有攝影機，沒有寫成程式**）

只要新的來源節點送出一樣的 `image`（Arrow UInt8，W×H byte，metadata 帶 `seq`、`t_ns`、`width`、`height`、`encoding="mono8"`、`label=-1`），`detect` 和 `display` 都不用改。三條路，由簡到難：

1. **V4L2，經 ffmpeg**（第 19 篇的 `SOURCE=ffmpeg`，本篇 `dataflow-v4l2.yml`）：USB UVC 攝影機、以及 ZYNQ MPSoC 上經 Xilinx 影像管線（MIPI CSI-2 Rx → 色彩轉換 → Video Frame Buffer Write）接出來的攝影機，在 Linux 上通常都是 `/dev/videoN`。最省事，但 PetaLinux 的 rootfs 預設不一定有 ffmpeg，要自己加套件；另外多一個行程、多一次 pipe 複製。本篇只驗證了「沒有裝置時會正確報錯」。
2. **V4L2，用 `v4l` crate 直接讀**：`Device::new(0)` → `set_format`（例如 640×480 YUYV）→ `MmapStream` 拿到驅動的 mmap 緩衝 → YUYV 每 2 byte 取第 1 個（Y）就是灰階 → `send_output_raw`。少一個行程。本篇**沒有加這個 crate、沒有編譯**（沒有裝置可測，依規定不擴大範圍）。
3. **第 16 篇的 DMA 節點**：PL 端把影像（例如已經轉好灰階、甚至縮好尺寸）用 AXI4-Stream → AXI DMA S2MM 寫進 u-dma-buf；節點等 UIO 中斷 → `sync_for_cpu` 讓 cache 失效 → 從 mmap 讀一張 → `send_output_raw`。要改的只有兩點：輸出改成 `image`＋上面那組 metadata；`t_ns` 在**收到中斷的那一刻**取，才算得到最接近「擷取完成」的端到端延遲。好處是 PL 可以先做二值化、找框、甚至整個 CNN，CPU 幾乎不用碰像素（見最後一段）。

另外兩個上板子時要注意的地方（第 19 篇也提過）：

- **節奏**：本篇 camera 用 dora timer 每 33 ms 拉一張，這是為了播放影片檔。真攝影機／DMA 自己有幀率，應該改成「阻塞等下一張 → 送出」，不要 timer，否則驅動緩衝裡的舊畫面會越積越多。**沒有實作。**
- **跨機器的時間戳**：`t_ns` 用的是 `SystemTime`。整條管線在同一塊板子上就沒問題；若像第 14、15 篇那樣拆到兩台機器（例如 camera 在板子、display 在 PC），兩台時鐘要先用 chrony／PTP 同步，否則端到端延遲會算錯。

### 7. 板上的延遲預算怎麼估

**延遲預算**＝把「從光打到感光元件，到結果出現在螢幕上」拆成幾段，每段給一個上限，加起來要小於需求；同時**每一段的「每張處理時間」都要小於幀間隔**（30 fps 是 33.3 ms），不然就會開始丟幀（第 8、18、19 篇）。本篇的 display 已經把可以在軟體裡量的幾段分開印出來，上板子後**直接跑 `dataflow-aarch64.yml` 看 log** 就能把下表的「板上實測」欄填起來。

| 段落 | 怎麼量／怎麼估 | x86_64 實測（本篇，p50） | qemu 模擬（本篇，p50，**不代表實機**） | ZYNQ 上的估算方法 |
|---|---|---|---|---|
| ① 曝光＋感光元件讀出 | 軟體量不到；約 1 個幀間隔 | —（合成畫面） | — | 30 fps ≈ 33 ms；看感光元件規格 |
| ② 驅動／DMA 搬進 DDR | DMA 中斷時間 − 幀結束時間 | — | — | 資料量 ÷ 有效頻寬：320×240 灰階 76.8 KB、640×480 YUYV 614 KB；要在板上量 |
| ③ camera → detect 收到（dora） | detect 的 `t_in_ns − t_ns` | 第 19 篇約 0.6 ms（合成 0.62、影片 0.57） | — | 板上實測 |
| ④ 前處理（找框＋裁切） | detect 的「前處理」 | 0.05 ms | 0.56 ms | 板上實測；PL 可代勞 |
| ⑤ CNN 推論 | detect 的「模型」 | 0.50 ms | 8.88 ms | 板上實測；或用 MAC 數 ÷ 加速器吞吐量估（下一段） |
| ⑥ detect → display 收到 | display「擷取→display 收到」扣掉上面三段 | 「擷取→display 收到」共 1.70 ms | 共 11.93 ms | 板上實測 |
| ⑦ 畫圖 | display「畫圖」 | 0.55 ms | 1.95 ms | 板上實測；640×480 RGB 約 0.9 MB 的記憶體寫入 |
| ⑧ 輸出（寫 fb／ffmpeg） | display「輸出」 | 0.67 ms（mp4）、0.51 ms（假 fb） | 2.72 ms（假 fb） | 板上實測 |
| ⑨ 等垂直同步＋面板反應 | 軟體量不到 | — | — | 60 Hz 螢幕最多再 16.7 ms，面板再數 ms |

讀法：

- ③～⑧ 是軟體可以量、也可以調的部分。本篇在 x86 上「擷取→畫面送出」p50 只有 2.5～3.1 ms，遠小於 ①＋⑨ 這兩段「物理延遲」（加起來約 35～50 ms）。**也就是說，在這個小模型上，真正的瓶頸是攝影機與螢幕，不是 dora 也不是 CNN。**
- 換成 ZYNQ 的 Cortex-A53（1.2～1.5 GHz 級）時，⑤ 會變慢多少要實測。若 ⑤ 超過幀間隔（33 ms），detect 的 `queue_size: 1` 會丟幀，顯示仍然流暢但結果只更新一部分；這時就是把 ⑤ 搬到 FPGA 的時候。
- p99 比 p50 重要：本篇 x86 上 mp4 輸出的 p99 有 17.30 ms（ffmpeg pipe 阻塞），只看 p50 會以為很快。預算要用 p99 或 max 抓。

**CNN 的運算量**（從第 17 篇的網路結構算出，可以用來估 FPGA 加速器的時間）：

| 層 | 輸出 | 每個輸出的乘加 | 乘加次數（MAC） |
|---|---|---|---|
| conv1 5×5，1→16 | 24×24×16 | 25 | 230,400 |
| conv2 5×5，16→32 | 8×8×32 | 400 | 819,200 |
| fc1 512→64 | 64 | 512 | 32,768 |
| fc2 64→10 | 10 | 64 | 640 |
| **合計** | | | **1,083,008（約 1.08 M MAC）** |

（參數數：416＋12,832＋32,832＋650＝46,730，和第 17 篇 `train` 印出的相同，可以當作這張表算對了的檢查。）

套公式：**時間 ≈ MAC 數 ÷（每個時脈能做幾個 MAC × 時脈頻率）**。例如假設 PL 上的加速器每個時脈做 64 個 MAC、跑 200 MHz：1,083,008 ÷ 64 ≈ 16,922 個時脈 ≈ 0.085 ms。**這只是算式示範，不是實測**：真正的時間還要加上權重與影像的搬運（DMA）、層與層之間的等待、PS↔PL 的同步，通常會是這個理想值的好幾倍；而且 x86 上 candle 只花 0.5 ms，所以**這個 MNIST 小模型其實不需要 FPGA**。FPGA 加速的價值要到更大的模型（例如 YOLO 類偵測網路動輒數十億 MAC）或更嚴的功耗限制時才明顯。

## 完整範例

```
examples/lesson-20/
├── Cargo.toml
├── src/
│   ├── lib.rs              # RGB 畫布、畫框、5×7 點陣字、ASCII 預覽、framebuffer 格式轉換、RGB PNG
│   └── bin/
│       ├── camera.rs       # 與第 19 篇逐字相同
│       ├── detect.rs       # 與第 19 篇逐字相同
│       └── display.rs      # 本篇主角：顯示節點
├── dataflow.yml            # 合成 → detect → display（mp4＋png＋ascii）
├── dataflow-file.yml       # 影片檔 → detect → display（假 framebuffer 32 bpp＋png）
├── dataflow-window.yml     # 合成 → detect → display（minifb 視窗＋png）
├── dataflow-aarch64.yml    # aarch64 交叉編譯產物（假 framebuffer 16 bpp＋png＋ascii）
├── dataflow-v4l2.yml       # 真攝影機設定（這台沒有 /dev/video0）
├── run-demo.sh             # x86_64：dora build → dora run
└── run-aarch64.sh          # aarch64：官方 aarch64 dora CLI（qemu）→ dora run
```

依賴：第 19 篇的 `lesson-19`（路徑依賴）→ 第 17 篇的 `lesson-17`（模型與 MNIST 讀檔）。模型檔 `../lesson-17/models/mnist-cnn.safetensors`、測試資料 `../lesson-17/data/`、影片檔 `../lesson-19/clip.mp4` 都直接引用，沒有複製。

### Cargo.toml

```toml
[package]
name = "lesson-20"
version = "0.1.0"
edition = "2024"

[features]
default = []
# 真的開視窗（X11）：cargo build --release --features window。預設不開，交叉編譯到 ZYNQ 時不需要 X11。
window = ["dep:minifb"]

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
png = "0.18"
# detect.rs 直接 `use candle_core::{Device, Module}`：路徑依賴的套件不會把它的依賴「借」給你，要自己列（常見錯誤 1）
candle-core = "0.11"
# 沿用第 19 篇的程式庫（合成來源、找框、裁切、統計），它再沿用第 17 篇的 CNN。
lesson-19 = { path = "../lesson-19" }
minifb = { version = "0.28", optional = true }

# 第 15 篇的 ZYNQ 小檔案設定：cargo zigbuild --profile zynq --target aarch64-unknown-linux-gnu.2.28
[profile.zynq]
inherits = "release"
opt-level = "s"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
```

### src/lib.rs

```rust
//! 第 20 篇共用程式：RGB 畫布、畫框、5×7 點陣字、ASCII 預覽、輸出成 RGB PNG／framebuffer 格式。
//! 只用標準函式庫＋png crate，不依賴任何圖形環境，所以 x86_64、aarch64（ZYNQ）都能直接編。
pub use lesson_19::{BBox, env_or, now_ns, pct, summary_us};

/// RGB888 畫布（每像素 3 byte，列優先）
pub struct Canvas { pub w: usize, pub h: usize, pub px: Vec<u8> }

pub type Rgb = [u8; 3];
pub const GREEN: Rgb = [40, 220, 60];
pub const RED: Rgb = [235, 50, 40];
pub const YELLOW: Rgb = [250, 210, 40];
pub const WHITE: Rgb = [245, 245, 245];

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self { Self { w, h, px: vec![0; w * h * 3] } }

    /// 灰階影像（gw×gh）用最近鄰放大 scale 倍畫進畫布（畫布大小必須是 gw*scale × gh*scale）
    pub fn blit_gray(&mut self, g: &[u8], gw: usize, gh: usize, scale: usize) {
        debug_assert_eq!((self.w, self.h), (gw * scale, gh * scale));
        for y in 0..self.h {
            let src = &g[(y / scale) * gw..(y / scale + 1) * gw];
            let dst = &mut self.px[y * self.w * 3..(y + 1) * self.w * 3];
            for (x, d) in dst.chunks_exact_mut(3).enumerate() {
                let v = src[x / scale];
                d.copy_from_slice(&[v, v, v]);
            }
        }
    }
    #[inline]
    pub fn put(&mut self, x: isize, y: isize, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }
    /// 實心矩形；alpha 0..=255（255 = 不透明），用來畫半透明的文字底色
    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: Rgb, alpha: u16) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                let i = (yy * self.w + xx) * 3;
                for k in 0..3 {
                    self.px[i + k] = ((self.px[i + k] as u16 * (255 - alpha) + c[k] as u16 * alpha) / 255) as u8;
                }
            }
        }
    }
    /// 空心外框，線寬 t
    pub fn rect(&mut self, b: BBox, t: usize, c: Rgb) {
        let (x0, y0) = (b.x as isize, b.y as isize);
        let (x1, y1) = ((b.x + b.w) as isize - 1, (b.y + b.h) as isize - 1);
        for d in 0..t as isize {
            for x in x0 - d..=x1 + d { self.put(x, y0 - d, c); self.put(x, y1 + d, c); }
            for y in y0 - d..=y1 + d { self.put(x0 - d, y, c); self.put(x1 + d, y, c); }
        }
    }
    /// 用 5×7 點陣字寫字，每個點放大成 s×s；回傳寫完後的 x
    pub fn text(&mut self, x: usize, y: usize, s: usize, c: Rgb, txt: &str) -> usize {
        let mut cx = x;
        for ch in txt.chars() {
            let g = glyph(ch.to_ascii_uppercase());
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) != 0 {
                        for dy in 0..s { for dx in 0..s {
                            self.put((cx + col * s + dx) as isize, (y + row * s + dy) as isize, c);
                        }}
                    }
                }
            }
            cx += 6 * s;
        }
        cx
    }
    /// 終端機 ASCII 預覽：縮成 cols×rows，用亮度挑字元
    pub fn ascii(&self, cols: usize, rows: usize) -> String {
        const RAMP: &[u8] = b" .:-=+*#%@";
        let mut s = String::with_capacity((cols + 1) * rows);
        for r in 0..rows {
            for c in 0..cols {
                // 一格涵蓋畫布上一塊區域：區域裡只要有外框顏色（純綠／純紅）就標 G／R（線很細，只取樣中心點會漏掉）
                let (x0, x1, y0, y1) = (c * self.w / cols, (c + 1) * self.w / cols, r * self.h / rows, (r + 1) * self.h / rows);
                let mut mark = None;
                for y in y0..y1 { for x in x0..x1 {
                    let i = (y * self.w + x) * 3;
                    let (pr, pg) = (self.px[i], self.px[i + 1]);
                    if pg > 180 && pr < 100 { mark = Some('G'); }
                    if pr > 180 && pg < 100 { mark = Some('R'); }
                }}
                if let Some(m) = mark { s.push(m); continue; }
                let i = (((y0 + y1) / 2) * self.w + (x0 + x1) / 2) * 3;
                let p = &self.px[i..i + 3];
                let l = (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8;
                s.push(RAMP[(l as usize * (RAMP.len() - 1)) / 255] as char);
            }
            s.push('\n');
        }
        s
    }
    /// 轉成 framebuffer 常見格式寫進 dst 的一列（bpp=32：B,G,R,X；bpp=16：RGB565 little-endian）
    pub fn row_to_fb(&self, y: usize, bpp: usize, dst: &mut [u8]) {
        let src = &self.px[y * self.w * 3..(y + 1) * self.w * 3];
        match bpp {
            32 => for (d, p) in dst.chunks_exact_mut(4).zip(src.chunks_exact(3)) { d.copy_from_slice(&[p[2], p[1], p[0], 0xff]); },
            16 => for (d, p) in dst.chunks_exact_mut(2).zip(src.chunks_exact(3)) {
                let v = ((p[0] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[2] as u16 >> 3);
                d.copy_from_slice(&v.to_le_bytes());
            },
            _ => {}
        }
    }
    pub fn save_png(&self, path: &str) -> eyre::Result<()> {
        let f = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(f, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&self.px)?;
        Ok(())
    }
}

/// 5×7 點陣字（每列 5 bit，最高位在左）。只收這個專題會用到的字；不認得的字畫成實心方塊。
pub fn glyph(c: char) -> [u8; 7] {
    let b = |r: [&str; 7]| r.map(|s| u8::from_str_radix(s, 2).unwrap());
    match c {
        '0' => b(["01110","10001","10011","10101","11001","10001","01110"]),
        '1' => b(["00100","01100","00100","00100","00100","00100","01110"]),
        '2' => b(["01110","10001","00001","00010","00100","01000","11111"]),
        '3' => b(["11111","00010","00100","00010","00001","10001","01110"]),
        '4' => b(["00010","00110","01010","10010","11111","00010","00010"]),
        '5' => b(["11111","10000","11110","00001","00001","10001","01110"]),
        '6' => b(["00110","01000","10000","11110","10001","10001","01110"]),
        '7' => b(["11111","00001","00010","00100","01000","01000","01000"]),
        '8' => b(["01110","10001","10001","01110","10001","10001","01110"]),
        '9' => b(["01110","10001","10001","01111","00001","00010","01100"]),
        'A' => b(["01110","10001","10001","11111","10001","10001","10001"]),
        'B' => b(["11110","10001","10001","11110","10001","10001","11110"]),
        'C' => b(["01110","10001","10000","10000","10000","10001","01110"]),
        'D' => b(["11100","10010","10001","10001","10001","10010","11100"]),
        'E' => b(["11111","10000","10000","11110","10000","10000","11111"]),
        'F' => b(["11111","10000","10000","11110","10000","10000","10000"]),
        'G' => b(["01110","10001","10000","10111","10001","10001","01111"]),
        'I' => b(["01110","00100","00100","00100","00100","00100","01110"]),
        'K' => b(["10001","10010","10100","11000","10100","10010","10001"]),
        'L' => b(["10000","10000","10000","10000","10000","10000","11111"]),
        'M' => b(["10001","11011","10101","10101","10001","10001","10001"]),
        'N' => b(["10001","10001","11001","10101","10011","10001","10001"]),
        'O' => b(["01110","10001","10001","10001","10001","10001","01110"]),
        'P' => b(["11110","10001","10001","11110","10000","10000","10000"]),
        'Q' => b(["01110","10001","10001","10001","10101","10010","01101"]),
        'R' => b(["11110","10001","10001","11110","10100","10010","10001"]),
        'S' => b(["01111","10000","10000","01110","00001","00001","11110"]),
        'T' => b(["11111","00100","00100","00100","00100","00100","00100"]),
        'U' => b(["10001","10001","10001","10001","10001","10001","01110"]),
        'X' => b(["10001","10001","01010","00100","01010","10001","10001"]),
        '.' => b(["00000","00000","00000","00000","00000","01100","01100"]),
        ':' => b(["00000","01100","01100","00000","01100","01100","00000"]),
        '%' => b(["11000","11001","00010","00100","01000","10011","00011"]),
        '#' => b(["01010","01010","11111","01010","11111","01010","01010"]),
        '/' => b(["00000","00001","00010","00100","01000","10000","00000"]),
        '=' => b(["00000","00000","11111","00000","11111","00000","00000"]),
        '-' => b(["00000","00000","00000","11111","00000","00000","00000"]),
        ' ' => [0; 7],
        _ => [0x1f; 7],
    }
}
```

### src/bin/display.rs

```rust
//! 顯示節點（取代第 19 篇的 sink）：把「原始畫面＋外框＋辨識結果＋延遲」畫成一張 RGB 影格，送到一個或多個輸出。
//! 輸入：image（camera/image，原始灰階畫面）、result（detect/result，10 個機率＋外框 metadata）。
//! 兩者用 metadata 的 seq 配對：image 先到就先放在小緩衝裡，result 到了再找同一個 seq 的畫面來畫。
//! env OUTPUT（逗號分隔，可多選）：
//!   mp4    ：開 ffmpeg 子行程，把 rgb24 影格從 stdin 餵進去，壓成 MP4_PATH（H.264）
//!   png    ：每 PNG_EVERY 張存一張 PNG 到 RESULT_DIR
//!   ascii  ：每 ASCII_EVERY 張在 log 印一張 ASCII_COLS×ASCII_ROWS 的終端預覽
//!   fb     ：寫到 Linux framebuffer（FB_DEV，例如 /dev/fb0；ZYNQ 接 DisplayPort／HDMI 時常見）。
//!            是 /dev/fb* 時從 sysfs 讀解析度、bpp、stride；否則（測試用一般檔案）用 FB_W、FB_H、FB_BPP
//!   window ：minifb 視窗（要 cargo build --features window，需要 X11）
//! 其他：SCALE（放大倍數，預設 2）、FONT（字放大倍數，預設 2）、RESULT_DIR（⚠️不要叫 OUT_DIR，見常見錯誤 2）、WARMUP（前幾筆不計入統計）
use dora_node_api::{DoraNode, Event, Parameter, get_integer_param};
use lesson_20::{BBox, Canvas, GREEN, RED, WHITE, YELLOW, env_or, now_ns, summary_us};
use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Instant;

struct Fb { file: std::fs::File, w: usize, h: usize, bpp: usize, stride: usize, row: Vec<u8> }

impl Fb {
    fn open(dev: &str) -> eyre::Result<Self> {
        let (w, h, bpp, stride) = if let Some(n) = dev.strip_prefix("/dev/fb") {
            // 真的 framebuffer：解析度、每像素位元數、每列 byte 數都從 sysfs 讀
            let rd = |f: &str| std::fs::read_to_string(format!("/sys/class/graphics/fb{n}/{f}"));
            let vs = rd("virtual_size")?; let (a, b) = vs.trim().split_once(',').ok_or_else(|| eyre::eyre!("virtual_size 格式不對"))?;
            let bpp: usize = rd("bits_per_pixel")?.trim().parse()?;
            let stride: usize = rd("stride")?.trim().parse()?;
            (a.parse()?, b.parse()?, bpp, stride)
        } else {
            let (w, h, bpp): (usize, usize, usize) = (env_or("FB_W", 800), env_or("FB_H", 600), env_or("FB_BPP", 32));
            (w, h, bpp, w * bpp / 8)
        };
        eyre::ensure!(bpp == 32 || bpp == 16, "只支援 32 或 16 bpp，這個裝置是 {bpp}");
        let file = std::fs::OpenOptions::new().write(true).create(!dev.starts_with("/dev/")).open(dev)?;
        if !dev.starts_with("/dev/") { file.set_len((stride * h) as u64)?; } // 測試用檔案：先撐到整個畫面的大小
        println!("[display] framebuffer {dev}：{w}x{h} {bpp}bpp stride {stride}");
        Ok(Self { file, w, h, bpp, stride, row: vec![] })
    }
    /// 把畫布貼到左上角（超出的部分裁掉），一列一列 pwrite
    fn show(&mut self, c: &Canvas) -> eyre::Result<()> {
        let cols = c.w.min(self.w);
        self.row.resize(c.w * self.bpp / 8, 0);
        for y in 0..c.h.min(self.h) {
            c.row_to_fb(y, self.bpp, &mut self.row);
            self.file.write_all_at(&self.row[..cols * self.bpp / 8], (y * self.stride) as u64)?;
        }
        Ok(())
    }
}

enum Out {
    Mp4 { child: Child, stdin: Option<ChildStdin>, path: String },
    Png { dir: String, every: i64, saved: u64 },
    Ascii { every: i64, cols: usize, rows: usize },
    Fb(Fb),
    #[cfg(feature = "window")]
    Window { win: minifb::Window, buf: Vec<u32> },
}

fn main() -> eyre::Result<()> {
    let scale: usize = env_or("SCALE", 2);
    let font: usize = env_or("FONT", 2);
    let warmup: u64 = env_or("WARMUP", 30);
    let out_dir: String = env_or("RESULT_DIR", "run/out".to_string());
    let fps_hint: u32 = env_or("FPS", 30);
    let outputs: String = env_or("OUTPUT", "png".to_string());
    std::fs::create_dir_all(&out_dir)?;

    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut outs: Vec<Out> = vec![];
    let mut canvas: Option<Canvas> = None;          // 第一張畫面到了才知道大小
    let mut frames: VecDeque<(i64, usize, usize, Vec<u8>)> = VecDeque::new();
    let (mut shown, mut missing, mut correct, mut labeled, mut wrong_shown) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut e2e_in, mut e2e_shown, mut render, mut emit) = (vec![], vec![], vec![], vec![]);
    let (mut t_first, mut t_last) = (0u64, 0u64);
    let mut last_shown_ns = 0u64;
    let mut fps = 0.0f64;
    let (mut drop_img, mut drop_res, mut first_seq, mut last_seq, mut gaps) = (0u64, 0u64, -1i64, -1i64, 0u64);

    while let Some(ev) = events.recv() {
        let Event::Input { id, metadata, data } = ev else {
            if matches!(ev, Event::Stop(_)) { break; }
            continue;
        };
        let arrive = now_ns();
        for (k, d) in events.drain_drop_counts() { if k.as_str() == "result" { drop_res += d } else { drop_img += d } }
        let pm = &metadata.parameters;
        let gi = |k: &str| get_integer_param(pm, k).unwrap_or(-1);
        match id.as_str() {
            "image" => {
                let px: &[u8] = (&data).try_into()?;
                frames.push_back((gi("seq"), gi("width") as usize, gi("height") as usize, px.to_vec()));
                while frames.len() > 8 { frames.pop_front(); }
            }
            "result" => {
                let seq = gi("seq");
                if first_seq < 0 { first_seq = seq; }
                if last_seq >= 0 && seq > last_seq + 1 { gaps += (seq - last_seq - 1) as u64; }
                last_seq = seq;
                // 找同一個 seq 的畫面；比它舊的畫面不會再用到，一起丟掉
                while frames.front().is_some_and(|f| f.0 < seq) { frames.pop_front(); }
                let Some(f) = frames.pop_front().filter(|f| f.0 == seq) else { missing += 1; continue; };
                let (_, w, h, gray) = f;

                // ---- 第一次：依畫面大小建立畫布與輸出 ----
                if canvas.is_none() {
                    let (cw, ch) = (w * scale, h * scale);
                    canvas = Some(Canvas::new(cw, ch));
                    for o in outputs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        outs.push(match o {
                            "mp4" => {
                                let path: String = env_or("MP4_PATH", format!("{out_dir}/display.mp4"));
                                let mut child = Command::new("ffmpeg")
                                    .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
                                           "-s", &format!("{cw}x{ch}"), "-r", &fps_hint.to_string(), "-i", "-",
                                           "-c:v", "libx264", "-preset", "veryfast", "-crf", &env_or("MP4_CRF", 28).to_string(), "-pix_fmt", "yuv420p", &path])
                                    .stdin(Stdio::piped()).spawn()?;
                                let stdin = child.stdin.take();
                                Out::Mp4 { child, stdin, path }
                            }
                            "png" => Out::Png { dir: out_dir.clone(), every: env_or("PNG_EVERY", 100), saved: 0 },
                            "ascii" => Out::Ascii { every: env_or("ASCII_EVERY", 150), cols: env_or("ASCII_COLS", 64), rows: env_or("ASCII_ROWS", 24) },
                            "fb" => Out::Fb(Fb::open(&env_or("FB_DEV", "/dev/fb0".to_string()))?),
                            #[cfg(feature = "window")]
                            "window" => {
                                let mut win = minifb::Window::new("dora-rs lesson 20", cw, ch, minifb::WindowOptions::default())?;
                                win.set_target_fps(0); // 不讓 minifb 自己睡覺限速，節奏由 camera 決定
                                Out::Window { win, buf: vec![0; cw * ch] }
                            }
                            other => eyre::bail!("不認得的 OUTPUT={other}（window 要用 --features window 編譯）"),
                        });
                    }
                    println!("[display] 畫布 {cw}x{ch}（原始 {w}x{h} ×{scale}），輸出：{outputs}");
                }
                let c = canvas.as_mut().unwrap();

                // ---- 畫：原圖 → 外框 → 上方結果列 → 下方延遲列 ----
                let t0 = Instant::now();
                let (label, pred, found) = (gi("label"), gi("pred"), gi("found") == 1);
                let score = match pm.get("score") { Some(Parameter::Float(v)) => *v, _ => 0.0 };
                if label >= 0 { labeled += 1; if pred == label { correct += 1; } }
                let ok = label < 0 || pred == label;        // 真攝影機沒有答案（label = -1）時一律畫綠框
                let color = if !found { YELLOW } else if ok { GREEN } else { RED };
                c.blit_gray(&gray, w, h, scale);
                if found {
                    let b = BBox { x: gi("bx") as usize * scale, y: gi("by") as usize * scale, w: gi("bw") as usize * scale, h: gi("bh") as usize * scale };
                    c.rect(b, 2, color);
                }
                let bar = 7 * font + 6;
                c.fill(0, 0, c.w, bar, [0, 0, 0], 170);
                let head = if found { format!("#{seq} PRED {pred} {:.1}%", score * 100.0) } else { format!("#{seq} NO DIGIT") };
                let x = c.text(4, 3, font, color, &head);
                if label >= 0 { c.text(x + 6 * font, 3, font, WHITE, &format!("LABEL {label} {}", if ok { "OK" } else { "NG" })); }
                let e2e_ms = (arrive as i64 - gi("t_ns")) as f64 / 1e6;
                let det_ms = (gi("t_out_ns") - gi("t_in_ns")) as f64 / 1e6;
                c.fill(0, c.h - bar, c.w, bar, [0, 0, 0], 170);
                c.text(4, c.h - bar + 3, font, WHITE, &format!("E2E {e2e_ms:.2}MS DET {det_ms:.2}MS FPS {fps:.1}"));
                let t_render = t0.elapsed().as_nanos() as u64;

                // ---- 送到各個輸出 ----
                let t1 = Instant::now();
                for o in outs.iter_mut() {
                    match o {
                        Out::Mp4 { stdin, .. } => { if let Some(s) = stdin { s.write_all(&c.px)?; } }
                        Out::Png { dir, every, saved } => if seq % *every == 0 {
                            c.save_png(&format!("{dir}/frame_{seq:05}_label{label}_pred{pred}.png"))?; *saved += 1;
                        },
                        Out::Ascii { every, cols, rows } => if seq % *every == 0 {
                            println!("[display] ASCII 預覽 seq={seq}（{head}；G=綠框、R=紅框）\n{}", c.ascii(*cols, *rows));
                        },
                        Out::Fb(fb) => fb.show(c)?,
                        #[cfg(feature = "window")]
                        Out::Window { win, buf } => {
                            for (d, p) in buf.iter_mut().zip(c.px.chunks_exact(3)) {
                                *d = (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
                            }
                            if !win.is_open() { println!("[display] 視窗被關掉"); break; }
                            win.update_with_buffer(buf, c.w, c.h)?;
                        }
                    }
                }
                let done = now_ns();
                shown += 1;
                if found && !ok { wrong_shown += 1; }
                // 顯示幀率：用相鄰兩次「顯示完成」的間隔做指數平均
                if last_shown_ns > 0 {
                    let inst = 1e9 / (done - last_shown_ns).max(1) as f64;
                    fps = if fps == 0.0 { inst } else { fps * 0.9 + inst * 0.1 };
                }
                last_shown_ns = done;
                if shown == 1 { t_first = done; }
                t_last = done;
                if shown > warmup {
                    e2e_in.push((arrive as i64 - gi("t_ns")) as u64 / 1000);
                    e2e_shown.push((done as i64 - gi("t_ns")) as u64 / 1000);
                    render.push(t_render / 1000);
                    emit.push(t1.elapsed().as_nanos() as u64 / 1000);
                }
            }
            _ => {}
        }
    }

    // ---- 收尾：關掉 ffmpeg 的 stdin，等它把 mp4 寫完 ----
    for o in outs.iter_mut() {
        match o {
            Out::Mp4 { child, stdin, path } => {
                drop(stdin.take());
                let st = child.wait()?;
                let size = std::fs::metadata(&*path).map(|m| m.len()).unwrap_or(0);
                println!("[display] ffmpeg 結束（{st}），{path} {size} byte");
            }
            Out::Png { saved, .. } => println!("[display] 存了 {saved} 張 PNG"),
            _ => {}
        }
    }
    println!("[display] result 序號 {first_seq}..={last_seq}，缺口 {gaps}；dora 佇列丟棄 result {drop_res}、image {drop_img}");
    let secs = (t_last.saturating_sub(t_first)) as f64 / 1e9;
    println!("[display] 顯示 {shown} 張（找不到對應畫面 {missing}），正確 {correct}/{labeled} = {:.2}%，紅框 {wrong_shown} 張；平均顯示幀率 {:.2} 張/秒",
        correct as f64 * 100.0 / labeled.max(1) as f64, if secs > 0.0 { (shown - 1) as f64 / secs } else { 0.0 });
    println!("[display] 延遲（前 {warmup} 筆不計，共 {} 筆）擷取→display 收到 {}", e2e_in.len(), summary_us(&mut e2e_in));
    println!("[display]   擷取→畫面送出 {}；畫圖 {}；輸出 {}", summary_us(&mut e2e_shown), summary_us(&mut render), summary_us(&mut emit));
    Ok(())
}
```

### src/bin/camera.rs 與 src/bin/detect.rs（與第 19 篇相同）

這兩個檔案是直接從第 19 篇複製的（`diff ../lesson-19/src/bin/camera.rs src/bin/camera.rs` 與 detect 都沒有輸出），說明請看第 19 篇。為了讓本篇自成一份完整範例，全文附在下面。

<details>
<summary>src/bin/camera.rs</summary>

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

</details>

<details>
<summary>src/bin/detect.rs</summary>

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

</details>

### dataflow.yml

```yaml
# 第 20 篇：camera（合成 320×240，約 30 張/秒）→ detect（找框＋candle CNN）→ display（畫框＋字，輸出 mp4／PNG／ASCII）
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: synth
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "600"          # 約 20 秒
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
        queue_size: 1        # 只辨識最新的一張
    outputs:
      - result

  - id: display
    build: cargo build --release --bin display
    path: target/release/display
    env:
      OUTPUT: mp4,png,ascii
      RESULT_DIR: run/out-synth
      SCALE: "2"             # 320×240 → 640×480
      PNG_EVERY: "100"
      ASCII_EVERY: "300"
    inputs:
      image:
        source: camera/image
        queue_size: 4        # 原始畫面先到，等 result 來配對
      result:
        source: detect/result
        queue_size: 4        # 顯示只要跟上最新的；追不上就丟舊結果（下面統計會看到「找不到對應畫面」）
```

### dataflow-file.yml

```yaml
# 第 20 篇：camera（ffmpeg 解碼第 19 篇的 clip.mp4）→ detect → display（寫到 framebuffer＋PNG）。
# 沒有 /dev/fb0，用一般檔案 run/fake-fb0.raw 假裝是 800×600、32 bpp 的 framebuffer；板子上把 FB_DEV 改成 /dev/fb0 即可。
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: ffmpeg
      FFMPEG_INPUT: "-i ../lesson-19/clip.mp4"
      LABELS: ../lesson-19/clip.labels
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
        queue_size: 1        # 只辨識最新的一張
    outputs:
      - result

  - id: display
    build: cargo build --release --bin display
    path: target/release/display
    env:
      OUTPUT: fb,png
      RESULT_DIR: run/out-file
      FB_DEV: run/fake-fb0.raw
      FB_W: "800"
      FB_H: "600"
      FB_BPP: "32"
      SCALE: "2"             # 320×240 → 640×480
      PNG_EVERY: "100"
    inputs:
      image:
        source: camera/image
        queue_size: 4        # 原始畫面先到，等 result 來配對
      result:
        source: detect/result
        queue_size: 4        # 顯示只要跟上最新的；追不上就丟舊結果（下面統計會看到「找不到對應畫面」）
```

### dataflow-window.yml

```yaml
# 第 20 篇：camera（合成）→ detect → display（minifb 真視窗＋PNG）。要有 X11（這台是 Xvfb 虛擬螢幕 :12）。
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: synth
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "300"          # 約 10 秒
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
        queue_size: 1        # 只辨識最新的一張
    outputs:
      - result

  - id: display
    build: cargo build --release --features window --bin display
    path: target/release/display
    env:
      OUTPUT: window,png
      DISPLAY: ":12"         # X11 顯示器；真螢幕通常是 ":0"（從 ssh 跑時一定要設）
      RESULT_DIR: run/out-window
      SCALE: "2"             # 320×240 → 640×480
      PNG_EVERY: "100"
    inputs:
      image:
        source: camera/image
        queue_size: 4        # 原始畫面先到，等 result 來配對
      result:
        source: detect/result
        queue_size: 4        # 顯示只要跟上最新的；追不上就丟舊結果（下面統計會看到「找不到對應畫面」）
```

### dataflow-aarch64.yml

```yaml
# 第 20 篇：直接用交叉編譯好的 aarch64 執行檔（zynq profile）。沒有 build:（第 15 篇：有 build: 會在目標機上跑 cargo）。
# 這台沒有板子：用 qemu-user＋binfmt_misc 模擬，速度不代表 Cortex-A53。
# 上板子時：把 path 改成板子上的絕對路徑，camera 換成 V4L2／DMA，display 的 FB_DEV 改成 /dev/fb0。
nodes:
  - id: camera
    path: target/aarch64-unknown-linux-gnu/zynq/camera
    env:
      SOURCE: synth
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "300"
    inputs:
      tick: dora/timer/millis/33
    outputs:
      - image

  - id: detect
    path: target/aarch64-unknown-linux-gnu/zynq/detect
    env:
      MODEL: ../lesson-17/models/mnist-cnn.safetensors
      RAYON_NUM_THREADS: "1"
    inputs:
      image:
        source: camera/image
        queue_size: 1
    outputs:
      - result

  - id: display
    path: target/aarch64-unknown-linux-gnu/zynq/display
    env:
      OUTPUT: fb,png,ascii
      RESULT_DIR: run/out-aarch64
      FB_DEV: run/fake-fb0-aarch64.raw
      FB_W: "800"
      FB_H: "600"
      FB_BPP: "16"           # 這次試 RGB565（很多小 LCD／舊的 fbdev 是 16 bpp）
      SCALE: "2"
      PNG_EVERY: "100"
      ASCII_EVERY: "150"
      ASCII_COLS: "48"
      ASCII_ROWS: "16"
    inputs:
      image:
        source: camera/image
        queue_size: 4
      result:
        source: detect/result
        queue_size: 4
```

### dataflow-v4l2.yml

```yaml
# 第 20 篇：真攝影機版（V4L2，經 ffmpeg）→ detect → display。這台沒有 /dev/video0，預期會失敗。
nodes:
  - id: camera
    build: cargo build --release --bin camera
    path: target/release/camera
    env:
      SOURCE: ffmpeg
      FFMPEG_INPUT: "-f v4l2 -video_size 640x480 -framerate 30 -i /dev/video0"
      WIDTH: "320"
      HEIGHT: "240"
      FRAMES: "100000"
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
        queue_size: 1        # 只辨識最新的一張
    outputs:
      - result

  - id: display
    build: cargo build --release --bin display
    path: target/release/display
    env:
      OUTPUT: png
      RESULT_DIR: run/out-v4l2
      SCALE: "2"             # 320×240 → 640×480
      PNG_EVERY: "100"
    inputs:
      image:
        source: camera/image
        queue_size: 4        # 原始畫面先到，等 result 來配對
      result:
        source: detect/result
        queue_size: 4        # 顯示只要跟上最新的；追不上就丟舊結果（下面統計會看到「找不到對應畫面」）
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

### run-aarch64.sh

```bash
#!/usr/bin/env bash
# 用 qemu-user 模擬跑 aarch64 版：官方 aarch64 dora CLI＋aarch64 節點（需要先 binfmt 註冊 qemu-aarch64，見第 15 篇 setup-qemu.sh）
set -uo pipefail
cd "$(dirname "$0")"
export QEMU_LD_PREFIX=${QEMU_LD_PREFIX:?請設定 QEMU_LD_PREFIX：模擬板子 rootfs 的 aarch64 glibc sysroot（官方 aarch64 CLI 要 glibc ≥ 2.34）}
DORA=${DORA:?請設定 DORA：官方 aarch64 dora CLI 的路徑}
mkdir -p run
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/dataflow-aarch64.log
$DORA run dataflow-aarch64.yml --stop-after ${1:-90}s >> run/dataflow-aarch64.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/dataflow-aarch64.log
```

## 實際執行

```bash
cd examples/lesson-20   # 自 repo 根目錄
export PATH="$HOME/dora/bin:$PATH"   # 依你的 dora CLI 安裝位置調整

# 1) x86_64：合成來源 → mp4＋png＋ascii
./run-demo.sh dataflow.yml 60
ffprobe -v error -count_frames -select_streams v:0 \
  -show_entries stream=codec_name,width,height,nb_read_frames,r_frame_rate -show_entries format=duration \
  -of default=nw=1 run/out-synth/display.mp4
ffmpeg -i run/out-synth/display.mp4 -vf "select='not(mod(n\,15))',scale=160:120,tile=8x5" -frames:v 1 run/out-synth/tile-every15.png
ffmpeg -i run/out-synth/display.mp4 -vf "select='eq(n\,50)'" -frames:v 1 run/out-synth/mp4-frame50.png

# 2) x86_64：影片檔來源 → 假 framebuffer＋png；再把 framebuffer 的內容轉回 PNG 檢查
./run-demo.sh dataflow-file.yml 60
ffmpeg -f rawvideo -pix_fmt bgr0 -s 800x600 -i run/fake-fb0.raw -frames:v 1 run/out-file/fake-fb0-last.png

# 3) x86_64：真視窗（這台是 Xvfb :12；6 秒後截一張圖）
cargo build --release --features window --bin display     # 先單獨編一次確認 minifb 編得過
(sleep 6; ffmpeg -f x11grab -video_size 1920x1200 -i :12 -frames:v 1 run/xvfb-screenshot.png) &
./run-demo.sh dataflow-window.yml 60

# 4) 交叉編譯到 aarch64（cargo-zigbuild；zig 來自 pip 的 ziglang）
rustup target add aarch64-unknown-linux-gnu
export PATH=$PATH:$(python3 -c "import ziglang,os;print(os.path.dirname(ziglang.__file__))")
cargo zigbuild --profile zynq --target aarch64-unknown-linux-gnu.2.28 --bins
file target/aarch64-unknown-linux-gnu/zynq/{camera,detect,display}
readelf -V target/aarch64-unknown-linux-gnu/zynq/display | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1

# 5) qemu-user 模擬：binfmt 註冊 qemu-aarch64（需要 root），再用官方 aarch64 dora CLI 跑
Q=/usr/bin/qemu-aarch64   # 依你的 qemu-user 位置調整
[ -e /proc/sys/fs/binfmt_misc/register ] || sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc
sudo sh -c "echo ':qemu-aarch64:M::\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\xb7\x00:\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff:$Q:F' > /proc/sys/fs/binfmt_misc/register"
export QEMU_LD_PREFIX=$HOME/sysroot/aarch64-glibc2.41            # 依你的 sysroot 位置調整
export DORA=$HOME/aarch64/dora-cli-aarch64-unknown-linux-gnu/dora   # 官方 aarch64 CLI
./run-aarch64.sh 90
ffmpeg -f rawvideo -pix_fmt rgb565le -s 800x600 -i run/fake-fb0-aarch64.raw -frames:v 1 run/out-aarch64/fake-fb0-aarch64-last.png
echo -1 | sudo tee /proc/sys/fs/binfmt_misc/qemu-aarch64   # 用完取消註冊

# 6) 真攝影機設定（預期失敗）
./run-demo.sh dataflow-v4l2.yml 20
```

這台機器的 qemu-user 與 `file` 是本篇開始時重新下載的：第 15 篇用過的 qemu 已經不在系統上（`/usr/bin/qemu*` 不存在、binfmt_misc 沒有掛載），系統也沒有 `file` 指令。做法是從 deb.debian.org 下載 Debian 13 的 `qemu-user_10.0.13+ds-0+deb13u1_amd64.deb`、`file_5.46-5`、`libmagic1t64_5.46-5`、`libmagic-mgc_5.46-5`，用 `dpkg-deb -x` 解到一個本機資料夾（不裝進系統；該資料夾未納入本 repo）；`file` 要這樣叫：`LD_LIBRARY_PATH=$R/usr/lib/x86_64-linux-gnu $R/usr/bin/file -m $R/usr/lib/file/magic.mgc …`。

### 實測輸出 1：合成來源 `dataflow.yml`（2026-11-20 08:00:40 開始，台北時間）

```
開始 2026-11-20 08:00:40 CST
08:00:41 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 3.07 ms；THRESHOLD=128 SLOW_MS=0
08:00:41 stdout camera: [camera] SOURCE=synth 320x240 FRAMES=600 zero_copy_threshold=4096
08:00:41 stdout display: [display] 畫布 640x480（原始 320x240 ×2），輸出：mp4,png,ascii
08:01:01 stdout camera: [camera] 結束：送出 600 張
08:01:01 stdout detect: [detect] 結束：處理 600 張，找到數字 600 張；前處理 p50 0.05 ms / p99 0.08 ms / max 0.14 ms；模型 p50 0.50 ms / p99 0.92 ms / max 10.57 ms；dora 佇列丟棄 0
08:01:01 stdout display: [display] ffmpeg 結束（exit status: 0），run/out-synth/display.mp4 5310930 byte
08:01:01 stdout display: [display] 存了 6 張 PNG
08:01:01 stdout display: [display] result 序號 0..=599，缺口 0；dora 佇列丟棄 result 0、image 0
08:01:01 stdout display: [display] 顯示 600 張（找不到對應畫面 0），正確 585/600 = 97.50%，紅框 15 張；平均顯示幀率 30.39 張/秒
08:01:01 stdout display: [display] 延遲（前 30 筆不計，共 570 筆）擷取→display 收到 p50 1.70 / p99 4.36 / max 15.84 ms
08:01:01 stdout display: [display]   擷取→畫面送出 p50 3.11 / p99 19.84 / max 35.92 ms；畫圖 p50 0.55 / p99 0.81 / max 1.85 ms；輸出 p50 0.67 / p99 17.30 / max 23.81 ms
dora run 結束碼 0
```

`display.mp4` 的檢查（`run/ffprobe-synth.txt`）：

```
codec_name=h264
width=640
height=480
r_frame_rate=30/1
nb_read_frames=600
duration=20.000000
```

600 張全部進了 mp4，幀率 30、長度剛好 20 秒。抽出第 300 張 PNG（`run/out-synth/frame_00300_label6_pred6.png`）看：上方綠字 `#300 PRED 6 100.0%  LABEL 6 OK`、數字外面一個綠框、下方白字 `E2E 1.76MS DET 0.54MS FPS 30.4`。從 mp4 每 15 張抽一張拼成 8×5 的總覽（`run/out-synth/tile-every15.png`），40 格裡只有第 4 格（第 45～59 張那個「8」）是紅框；抽出第 50 張（`run/out-synth/mp4-frame50.png`）是紅字 `#50 PRED 3 45.3%  LABEL 8 NG`＋紅框，和第 19 篇「seq 45～59 全錯、score 約 0.45」一致。

log 裡的 ASCII 預覽（seq=300，時間戳前綴已拿掉；第一列的 `G` 是上方的綠色字，`%` 列是上下的文字條）：

```
[display] ASCII 預覽 seq=300（#300 PRED 6 100.0%；G=綠框、R=紅框）
GGGGG GGGGG GG GGGGGGG  %  %  %                                 
  . .    .  . . .    . .   . . ... .  . . .    .. .. .. . . .. .
...      .  ..  . . . . .     ... .   .. .... ....... . ....... 
       .    .         . .. .    .... . ......  .  . . .. ...... 
.   .          .. .... . ..    .     .. .... .. .  ....... .....
   ..    ....     GGGGGGGGGGGGGGG..  .  .   ..    ..  . ........
  .  .    .  ..   GG.  .*@#.... G............ . . .......... ...
        .. ..   . GG..:%%=.  . .G . ..  .    .....  ............
     . ........ . GG.+%+ ..  . .G.... ..... . ........  .. .:...
.  .  . .   .  . .GG%@#. . ..-:.G  .. .... .. ... ..............
 .    ...    .. ..GG@*...*%@@@@@G. ....   .. .......... .......:
  .  .       ...  GG@%*%@@@@@@#:G .... ............:. ....:...::
. .  .. ....  ... GG..+%@@@#: ..G. .  ..  .. ..... :.....:....::
 .....   . . .  ..GGGGGGGGGGGGGGG. .. ....... .... .......:....:
 ..      .  .  .  ..... . ...... ......  ....................:..
..   . ...  . .  . .....  .. . ........ ...  ...:...............
.  .    ..   ...   ............. ... .. ..........:..........:..
.   ..... ...... . .. ......  .................:.:.:.:.........:
   ...... ..   .. .....  .........  ...................:.......:
... .  ..   ...  ... .  ... ... ......... :......::.:.:.:......:
.  ..  .. ..... ................:. ..:........:......:::....:...
 ..... ........ ..... .......................:.:.::..:.:...:....
 .......... ....................:.........:.....::...:.....:::..
%  %      %%  %%% %  %     %%%                                  
```

怎麼讀：

- **0 丟幀、和第 19 篇一致**：顯示 600 張、序號缺口 0、兩個輸入的 dora 佇列丟棄都是 0；正確率 585/600（97.50%）和第 19 篇合成來源完全相同，表示換掉 sink 沒有影響前面兩個節點。
- **「擷取→display 收到」p50 1.70 ms**：和第 19 篇 sink 的端到端 p50 1.69 ms 幾乎一樣——display 收到結果之前的路徑本來就沒變。
- **顯示本身的成本**：畫一張 640×480 p50 0.55 ms；寫進 ffmpeg p50 0.67 ms，但 **p99 17.30 ms、max 23.81 ms**——ffmpeg 壓縮跟不上的瞬間 pipe 會滿、`write_all` 阻塞。所以「擷取→畫面送出」p50 3.11 ms、p99 19.84 ms。即使如此，平均顯示幀率仍有 30.39 張/秒，沒有因此丟幀（result 佇列 4 格夠用）。
- **雜訊很大**：同一個設定在 07:58:39 那次（log 已被覆蓋，數字取自當時的終端輸出）是「擷取→畫面送出」p50 2.99 ms／p99 6.67 ms、輸出 p99 3.18 ms。共用虛擬機上 p99 每次差好幾倍（第 18 篇），只能看量級。

### 實測輸出 2：影片檔來源＋假 framebuffer `dataflow-file.yml`（2026-11-20 08:01:22 開始，台北時間）

```
開始 2026-11-20 08:01:22 CST
08:01:23 stdout camera: [camera] ffmpeg 來源：-i ../lesson-19/clip.mp4（答案 300 行）
08:01:23 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 3.14 ms；THRESHOLD=128 SLOW_MS=0
08:01:23 stdout camera: [camera] SOURCE=ffmpeg 320x240 FRAMES=100000 zero_copy_threshold=4096
08:01:23 stdout display: [display] framebuffer run/fake-fb0.raw：800x600 32bpp stride 3200
08:01:23 stdout display: [display] 畫布 640x480（原始 320x240 ×2），輸出：fb,png
08:01:33 stdout camera: [camera] 影片讀完
08:01:33 stdout camera: [camera] 結束：送出 300 張
08:01:33 stdout detect: [detect] 結束：處理 300 張，找到數字 300 張；前處理 p50 0.05 ms / p99 0.07 ms / max 0.07 ms；模型 p50 0.42 ms / p99 0.89 ms / max 1.04 ms；dora 佇列丟棄 0
08:01:33 stdout display: [display] 存了 3 張 PNG
08:01:33 stdout display: [display] result 序號 0..=299，缺口 0；dora 佇列丟棄 result 0、image 0
08:01:33 stdout display: [display] 顯示 300 張（找不到對應畫面 0），正確 297/300 = 99.00%，紅框 3 張；平均顯示幀率 30.37 張/秒
08:01:33 stdout display: [display] 延遲（前 30 筆不計，共 270 筆）擷取→display 收到 p50 1.42 / p99 2.42 / max 2.47 ms
08:01:33 stdout display: [display]   擷取→畫面送出 p50 2.51 / p99 3.63 / max 23.50 ms；畫圖 p50 0.58 / p99 0.68 / max 0.73 ms；輸出 p50 0.51 / p99 0.88 / max 20.48 ms
dora run 結束碼 0
```

- 正確 297/300（99.00%），和第 19 篇影片檔來源相同。
- framebuffer 檔案是 800×600×4 = 1,920,000 byte（`ls -l` 實測），用 `ffmpeg -f rawvideo -pix_fmt bgr0 -s 800x600` 轉回 PNG（`run/out-file/fake-fb0-last.png`）：左上 640×480 是最後一張（`#299 PRED 6 100.0%  LABEL 6 OK`、綠框），右邊 160 px 與下方 120 px 是沒寫到的黑色——證明「依 stride 逐列寫入、只蓋左上角」是對的，色彩順序（B,G,R,X）也對。
- 寫 framebuffer 比寫 ffmpeg 穩：輸出 p50 0.51 ms、p99 0.88 ms；「擷取→畫面送出」p50 2.51 ms、p99 3.63 ms（max 23.50 ms 只有零星一次）。

### 實測輸出 3：minifb 真視窗 `dataflow-window.yml`（2026-11-20 08:02:01 開始，台北時間）

先單獨編譯（`run/build-window.log`）：`Compiling minifb v0.28.0`、`x11-dl v2.21.0`、`wayland-client v0.29.5`…，`Finished release profile [optimized] target(s) in 10.26s`。

```
開始 2026-11-20 08:02:01 CST
08:02:02 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 2.81 ms；THRESHOLD=128 SLOW_MS=0
08:02:03 stdout camera: [camera] SOURCE=synth 320x240 FRAMES=300 zero_copy_threshold=4096
08:02:03 stdout display: [display] 畫布 640x480（原始 320x240 ×2），輸出：window,png
08:02:13 stdout camera: [camera] 結束：送出 300 張
08:02:13 stdout detect: [detect] 結束：處理 300 張，找到數字 300 張；前處理 p50 0.05 ms / p99 0.07 ms / max 0.15 ms；模型 p50 0.42 ms / p99 1.00 ms / max 3.70 ms；dora 佇列丟棄 0
08:02:13 stdout display: [display] 存了 3 張 PNG
08:02:13 stdout display: [display] result 序號 0..=299，缺口 0；dora 佇列丟棄 result 0、image 0
08:02:13 stdout display: [display] 顯示 300 張（找不到對應畫面 0），正確 285/300 = 95.00%，紅框 15 張；平均顯示幀率 30.53 張/秒
08:02:13 stdout display: [display] 延遲（前 30 筆不計，共 270 筆）擷取→display 收到 p50 1.50 / p99 3.16 / max 19.22 ms
08:02:13 stdout display: [display]   擷取→畫面送出 p50 3.04 / p99 19.10 / max 31.60 ms；畫圖 p50 0.58 / p99 0.76 / max 3.99 ms；輸出 p50 0.86 / p99 10.34 / max 19.48 ms
dora run 結束碼 0
```

第 6 秒的 Xvfb 截圖（`run/xvfb-screenshot-small.png`，縮成 960×600）：桌面正中央一個標題列寫著 `dora-rs lesson 20` 的視窗，裡面是 `#122 PRED 7 100.0%  LABEL 7 OK`、綠框框住「7」，下方 `E2E 1.02MS DET 0.39MS FPS 30.6`；工作列上也有這個視窗。正確 285/300 是因為只跑了前 300 張（含錯的那 15 張「8」），和第 19 篇「合成來源前 300 張正確 285 張」相同。**這是 Xvfb 虛擬螢幕，不是實體螢幕**；實體螢幕的垂直同步、合成器延遲都沒有量到。

### 實測輸出 4：交叉編譯到 aarch64（2026-11-20 08:02～08:06，台北時間）

`run/build-aarch64.log` 的結尾：

```
   Compiling lesson-17 v0.1.0 (.../examples/lesson-17)
   Compiling lesson-19 v0.1.0 (.../examples/lesson-19)
   Compiling lesson-20 v0.1.0 (.../examples/lesson-20)
    Finished `zynq` profile [optimized] target(s) in 3m 40s

real	3m40.762s
user	11m27.956s
sys	0m54.783s
```

產物檢查（`run/aarch64-check.txt`）：

```
target/aarch64-unknown-linux-gnu/zynq/camera:  ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV), dynamically linked, interpreter /lib/ld-linux-aarch64.so.1, for GNU/Linux 2.0.0, stripped
target/aarch64-unknown-linux-gnu/zynq/detect:  ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV), dynamically linked, interpreter /lib/ld-linux-aarch64.so.1, for GNU/Linux 2.0.0, stripped
target/aarch64-unknown-linux-gnu/zynq/display: ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV), dynamically linked, interpreter /lib/ld-linux-aarch64.so.1, for GNU/Linux 2.0.0, stripped
-rwxr-xr-x 2 box box 11968664 Nov 20 08:06 target/aarch64-unknown-linux-gnu/zynq/camera
-rwxr-xr-x 2 box box 12730200 Nov 20 08:06 target/aarch64-unknown-linux-gnu/zynq/detect
-rwxr-xr-x 2 box box 11933936 Nov 20 08:06 target/aarch64-unknown-linux-gnu/zynq/display
-rwxr-xr-x 2 box box 39158024 Nov 20 08:00 target/release/camera
-rwxr-xr-x 2 box box 41328944 Nov 20 08:00 target/release/detect
-rwxr-xr-x 2 box box 40021336 Nov 20 08:01 target/release/display
camera: GLIBC_2.28
detect: GLIBC_2.28
display: GLIBC_2.28
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libpthread.so.0]
 0x0000000000000001 (NEEDED)             Shared library: [libdl.so.2]
 0x0000000000000001 (NEEDED)             Shared library: [librt.so.1]
```

- 三個都是 **ARM aarch64** 的 PIE 執行檔，interpreter 是板子上的 `/lib/ld-linux-aarch64.so.1`，已 strip。
- glibc 符號最高 `GLIBC_2.28`：glibc 2.28 以上的 PetaLinux rootfs 都能跑。
- 只依賴 glibc 的基本函式庫（libm、libc、libpthread、libdl、librt），沒有 X11、沒有 libstdc++、沒有 ONNX Runtime。
- 大小：aarch64 `zynq` profile 各 11.9～12.7 MB；x86_64 一般 release 各 39.2～41.3 MB（profile 不同，不能直接比；第 15 篇同一支程式 25.0 → 11.9 MB 的比較才是同架構）。

### 實測輸出 5：qemu-user 模擬跑 aarch64 管線 `dataflow-aarch64.yml`（2026-11-20 08:06:34 開始，台北時間）

binfmt 註冊後 `cat /proc/sys/fs/binfmt_misc/qemu-aarch64` 顯示 `enabled`、`interpreter <解壓目錄>/usr/bin/qemu-aarch64`、`flags: F`；`QEMU_LD_PREFIX=… dora --version`（aarch64 版）印出 `dora-cli 1.1.0`。daemon 啟動的確實是 aarch64 執行檔：

```
08:06:34 INFO   camera: spawner spawning `.../examples/lesson-20/target/aarch64-unknown-linux-gnu/zynq/camera` in `.../examples/lesson-20`
08:06:34 INFO   detect: spawner spawning `.../examples/lesson-20/target/aarch64-unknown-linux-gnu/zynq/detect` in `.../examples/lesson-20`
08:06:34 INFO   display: spawner spawning `.../examples/lesson-20/target/aarch64-unknown-linux-gnu/zynq/display` in `.../examples/lesson-20`
```

```
開始 2026-11-20 08:06:34 CST
08:06:34 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 51.68 ms；THRESHOLD=128 SLOW_MS=0
08:06:35 stdout camera: [camera] SOURCE=synth 320x240 FRAMES=300 zero_copy_threshold=4096
08:06:35 stdout display: [display] framebuffer run/fake-fb0-aarch64.raw：800x600 16bpp stride 1600
08:06:35 stdout display: [display] 畫布 640x480（原始 320x240 ×2），輸出：fb,png,ascii
08:06:45 stdout camera: [camera] 結束：送出 300 張
08:06:45 stdout detect: [detect] 結束：處理 299 張，找到數字 299 張；前處理 p50 0.56 ms / p99 1.06 ms / max 1.15 ms；模型 p50 8.88 ms / p99 12.99 ms / max 24.25 ms；dora 佇列丟棄 1
08:06:45 stdout display: [display] 存了 3 張 PNG
08:06:45 stdout display: [display] result 序號 0..=299，缺口 2；dora 佇列丟棄 result 1、image 0
08:06:45 stdout display: [display] 顯示 298 張（找不到對應畫面 0），正確 283/298 = 94.97%，紅框 15 張；平均顯示幀率 30.71 張/秒
08:06:45 stdout display: [display] 延遲（前 30 筆不計，共 268 筆）擷取→display 收到 p50 11.93 / p99 52.41 / max 87.79 ms
08:06:45 stdout display: [display]   擷取→畫面送出 p50 16.70 / p99 84.37 / max 120.00 ms；畫圖 p50 1.95 / p99 2.72 / max 3.74 ms；輸出 p50 2.72 / p99 5.29 / max 106.16 ms
dora run 結束碼 0
```

ASCII 預覽（seq=150，ASCII_COLS=48、ASCII_ROWS=16）：

```
[display] ASCII 預覽 seq=150（#150 PRED 1 100.0%；G=綠框、R=紅框）
GGGGGGGGGG GGGGGG% %%%                          
      .       ..  .    ...    .    .... ... ....
      . .       ...  . . . .... .. . ...... .. .
 .  ..   ..  .. .  . ..  .   . .... . ..........
     . . .....  ..  ..   ..  .................. 
.  ..   . ............     ... ............. ...
..   .   .....   ......  ...... .... . ..:......
  .   ..  .   ... . .......  ... .. ... ......:.
. ... . ..GGGGGGGGG...... .. . ... ...........:.
 ...   .. G ...#@+G.... . .... ........:..:.:...
.   . .   G .#@+ .G. .. .. ..........:...:.:....
  ....  ..G.@#....G .... ..........:.....:..:...
  .. ... .G@+ ..  G...................:.........
  . . ....GGGGGGGGG....................:..:..:..
..  .. . ..  .      .. ..:..........:.:.::..:::.
     %  %        % %   %                        

08:06:45 stdout camera: [camera] 結束：送出 300 張
08:06:45 stdout detect: [detect] 結束：處理 299 張，找到數字 299 張；前處理 p50 0.56 ms / p99 1.06 ms / max 1.15 ms；模型 p50 8.88 ms / p99 12.99 ms / max 24.25 ms；dora 佇列丟棄 1
08:06:45 DEBUG  camera: daemon handling node stop with exit status Success (restart: false, restart_count: 0)
08:06:45 INFO   camera: daemon camera finished successfully
[display] 存了 3 張 PNG
[display] result 序號 0..=299，缺口 2；dora 佇列丟棄 result 1、image 0
[display] 顯示 298 張（找不到對應畫面 0），正確 283/298 = 94.97%，紅框 15 張；平均顯示幀率 30.71 張/秒
[display] 延遲（前 30 筆不計，共 268 筆）擷取→display 收到 p50 11.93 / p99 52.41 / max 87.79 ms
[display]   擷取→畫面送出 p50 16.70 / p99 84.37 / max 120.00 ms；畫圖 p50 1.95 / p99 2.72 / max 3.74 ms；輸出 p50 2.72 / p99 5.29 / max 106.16 ms
08:06:45 DEBUG  detect: daemon handling node stop with exit status Success (restart: false, restart_count: 0)
08:06:45 INFO   detect: daemon detect finished successfully
08:06:45 DEBUG  display: daemon handling node stop with exit status Success (restart: false, restart_count: 0)
08:06:45 INFO   display: daemon display finished successfully
08:06:45 INFO   [dora]: daemon dataflow finished on machine `01a1f646-c13f-7140-8b37-9f0583f434c7`
2026-11-20T00:06:45.577299Z  INFO run_inner: dora_daemon: exiting daemon because all required dataflows are finished health_check_interval_duration=None self.daemon_id=DaemonId { machine_id: None, uuid: 01a1f646-c13f-7140-8b37-9f0583f434c7 }
dora run 結束碼 0
```

16 bpp 假 framebuffer 是 800×600×2 = 960,000 byte，用 `-pix_fmt rgb565le` 轉回 PNG（`run/out-aarch64/fake-fb0-aarch64-last.png`）：`#299 PRED 6 100.0%  LABEL 6 OK`、綠框、下方 `E2E 13.85MS DET 9.56MS FPS 30.3`，顏色正確，證明 RGB565 的位元排列也對。

怎麼讀：

- **功能**：aarch64 版三個節點＋aarch64 dora CLI 跑完 300 張，結束碼 0，辨識結果和 x86 版一致（錯的仍是那 15 張「8」：紅框 15 張），PNG、framebuffer、ASCII 都正確。這是本篇能給的「上板子之前」最強的證據。
- **速度（不代表實機）**：模型 p50 8.88 ms（x86 0.50 ms）、前處理 0.56 ms（x86 0.05 ms）、畫圖 1.95 ms（x86 0.55 ms），「擷取→畫面送出」p50 16.70 ms、p99 84.37 ms。qemu 的翻譯成本因指令種類差很多，**不能拿來推算 Cortex-A53 的速度**。
- **丟了 2 張**：detect 的佇列丟了 1 張（`queue_size: 1`，模擬下偶爾慢），display 的 result 佇列也丟了 1 張（序號缺口 2）。display 有一次輸出花了 106.16 ms，推測是那段期間 result 堆超過 4 格；**沒有追查**是哪一張、為什麼那次特別慢。

### 實測輸出 6：真攝影機設定 `dataflow-v4l2.yml`（2026-11-20 08:07:15 開始，台北時間）——預期失敗

這台 `ls /dev/video* /dev/fb*` 兩個都是 `No such file or directory`。

```
開始 2026-11-20 08:07:15 CST
08:07:16 stdout camera: [camera] ffmpeg 來源：-f v4l2 -video_size 640x480 -framerate 30 -i /dev/video0（答案 0 行）
08:07:16 stdout detect: [detect] 模型 ../lesson-17/models/mnist-cnn.safetensors 載入＋暖機 3.26 ms；THRESHOLD=128 SLOW_MS=0
08:07:16 stdout camera: [camera] SOURCE=ffmpeg 320x240 FRAMES=100000 zero_copy_threshold=4096
08:07:16 stdout camera: [video4linux2,v4l2 @ 0x55ef9f280140] Cannot open video device /dev/video0: No such file or directory
08:07:16 stdout camera: [in#0 @ 0x55ef9f27fe80] Error opening input: No such file or directory
08:07:16 stdout camera: Error opening input file /dev/video0.
08:07:16 stdout camera: Error opening input files: No such file or directory
08:07:16 stdout camera: Error: ffmpeg 一張都沒讀到，來源打不開？（看上面 ffmpeg 的錯誤訊息）
08:07:16 stdout camera: 
08:07:16 stdout camera: Location:
08:07:16 stdout detect: [detect] 結束：處理 0 張，找到數字 0 張；前處理 —；模型 —；dora 佇列丟棄 0
08:07:16 stdout camera:     src/bin/camera.rs:60:29
08:07:16 stdout display: [display] result 序號 -1..=-1，缺口 0；dora 佇列丟棄 result 0、image 0
08:07:16 stdout display: [display] 顯示 0 張（找不到對應畫面 0），正確 0/0 = 0.00%，紅框 0 張；平均顯示幀率 0.00 張/秒
08:07:16 stdout display: [display] 延遲（前 30 筆不計，共 0 筆）擷取→display 收到 p50 0.00 / p99 0.00 / max 0.00 ms
08:07:16 stdout display: [display]   擷取→畫面送出 p50 0.00 / p99 0.00 / max 0.00 ms；畫圖 p50 0.00 / p99 0.00 / max 0.00 ms；輸出 p50 0.00 / p99 0.00 / max 0.00 ms
Dataflow failed:
Node `camera` failed: exited with code 1 with stderr output:
dora run 結束碼 1
```

和第 19 篇一樣，只證明「打不開時 camera 會報錯、dataflow 會失敗（結束碼 1）」；這時 detect、display 都正常結束（處理 0 張），display 印的延遲全是 0，因為沒有資料。**真攝影機的畫面與延遲完全沒有測到。**

## 常見錯誤

1. **`unresolved import candle_core`（E0432）與 `no method named forward`（E0599）**：第一次編譯時 `detect.rs` 失敗（`run/build-error-1.txt`）。`detect.rs` 直接 `use candle_core::{Device, Module}`，但本篇的 Cargo.toml 只列了 `lesson-19`。**路徑依賴的套件不會把它自己的依賴「借」給你**，要在自己的 `[dependencies]` 再列一次 `candle-core = "0.11"`（版本跟 lesson-17 一樣，cargo 會共用同一份編譯結果）。`forward` 找不到也是同一個原因：`Module` trait 不在範圍內。
2. **又踩了第 19 篇的 `OUT_DIR` 坑**：display 第一版用 `OUT_DIR` 當輸出資料夾的環境變數，結果每次 `dora run` 前的 build，camera 要 1 分 13 秒、display 又 1 分 09 秒（都在重編 `ring`、`rustls`、`zenoh`…，數字取自當時的 log，後來被覆蓋）。原因：dora 把節點的 `env:` 也套到 `build:` 指令上，而 `ring` 的 build script 宣告了 `cargo:rerun-if-env-changed=OUT_DIR`（第 19 篇 `run/ring-rerun-if-env.txt`）。改名成 `RESULT_DIR` 後，第一次 build 還要重編一次（`run/dataflow-build.log`：camera 1m 12s），之後三個節點都是 0.19～0.21 秒。**教訓再說一次：節點的 env 不要用 `OUT_DIR`、`CC`、`CFLAGS`、`TARGET` 這類名字。**
3. **ASCII 預覽看不到外框**：第一版每格只取中心點的顏色，640 px 寬縮成 64 格，每格 10 px，2 px 粗的框線幾乎都被跳過，預覽裡只看得到數字。改成「格子裡只要有任何一個框線色像素就標 `G`／`R`」才看得到。縮圖時細線會消失，是所有降取樣（包括 CNN 的 pooling）都要注意的事。
4. **framebuffer 轉回的 PNG 右邊和下面是白色**：第一次用 `-pix_fmt bgra` 轉，沒寫到的區域是全 0，alpha 也是 0，看圖軟體把透明顯示成白色，看起來像「寫壞了」。framebuffer 的第 4 個 byte 通常不是 alpha（是填充），轉換時要用 `bgr0`。
5. **mp4 打不開**：如果 display 被強制結束（例如 `dora run --stop-after` 時間設得比影片短、或節點 panic），ffmpeg 沒收到 EOF，mp4 的 moov 索引沒寫，檔案就打不開。本篇在事件迴圈結束後先 `drop(stdin)` 再 `wait()`。這一條是程式設計時的考量，**沒有實際重現「打不開」的情況**。
6. **aarch64 執行檔跑不起來（`Exec format error`）**：沒有註冊 binfmt 就直接執行 aarch64 ELF 會這樣；在真的板子上則是把 x86 的檔案傳上去了。用 `file` 檢查。本篇沒有刻意重現這個錯誤（第 15 篇常見錯誤 4 也只是說明、沒有實測）。

## 小練習

1. **加「不確定」狀態**：score < 0.6 時改畫黃框、上方寫 `PRED ? (3 45%)`，並另外統計「不確定」張數。看看那 15 張「8」會不會全部變成不確定。
2. **只錄有問題的片段**：mp4 輸出改成只有 NG 時才寫入，前後各多留 15 張（提示：用一個環形緩衝留住最近 15 張的畫布）。這就是「事件錄影」，在產品上很常用。
3. **讓 framebuffer 不撕裂**（進階，本篇沒有做）：開一個高度是兩倍的虛擬 framebuffer（`FBIOPUT_VSCREENINFO` 把 `yres_virtual` 設成 `2 × yres`），輪流畫上下兩半，再用 `FBIOPAN_DISPLAY` 切換、`FBIO_WAITFORVSYNC` 等垂直同步。需要 `libc::ioctl`。
4. **上板子填預算表**：有 ZYNQ 板子時，把 aarch64 執行檔、`dataflow-aarch64.yml`、模型與 MNIST 測試資料 `scp` 過去（路徑改成板子上的絕對路徑），跑一次，把重點說明 7 的表格「板上實測」欄填起來，再和 x86 比較哪一段變慢最多。

## 本篇沒有驗證成功的部分

- **真的 ZYNQ 板**：沒有。交叉編譯產物只在 qemu-user 上跑過；Cortex-A53／A9 上的速度、記憶體、溫度、實際 PetaLinux rootfs（glibc 版本、有沒有 ffmpeg、`/dev/fb0` 的格式）都沒有驗證。重點說明 7 表格中「ZYNQ 上的估算方法」那一欄都是方法，不是數字。
- **ZYNQ-7000（armv7）版**：本篇只編了 aarch64，沒有編 `armv7-unknown-linux-gnueabihf`（第 15 篇有編過別的程式；官方 dora 也沒有 armv7 CLI）。
- **真的 framebuffer**：box 沒有 `/dev/fb0`。`Fb::open` 讀 sysfs（`virtual_size`、`bits_per_pixel`、`stride`）的那條路徑**從來沒有執行過**；只驗證了「寫一般檔案、依 FB_W／FB_H／FB_BPP 換算 stride」的路徑。fbcon 游標、權限、tearing 都沒有實測。
- **實體螢幕與 Wayland**：`window` 只在 Xvfb（X11）上跑過；沒有在 Wayland、沒有在真的螢幕上跑，所以垂直同步與合成器的延遲沒有量到。
- **SDL2、DRM/KMS（page flip）、RTP／RTSP 串流**：只在重點說明 3 提到，沒有寫程式。
- **真攝影機、`v4l` crate、DMA 影像節點、用攝影機節奏取代 timer**：都只有說明（重點說明 6），沒有寫成程式、沒有編譯。`dataflow-v4l2.yml` 只得到「打不開裝置」的錯誤。
- **FPGA CNN 加速**：重點說明 7 的「0.085 ms」是算式示範，不是任何實作或實測。
- **第一次跑 `dataflow.yml` 只顯示了 589 張**（07:53，正確 574/589，找不到對應畫面 0；那時還沒有加丟幀與序號缺口的計數，log 已被後來的執行覆蓋，數字記在 `run/note-first-run.txt`）。之後加了計數重跑兩次都是 600 張、0 丟棄。那 11 張是被 display 的佇列丟掉、還是別的原因，**沒有查出來**。
- **qemu 執行時 display 丟的那 1 張 result**：原因只是推測（實測輸出 5），沒有追查。
- **中文字**：點陣字只有英數字與少數符號，沒有做中文顯示。

收尾：對 `lesson-20` 執行 `cargo clean`，刪除 `target/`（`du` 清除前量到 3.1 GB：`release` 1.7 GB、`aarch64-unknown-linux-gnu` 1.1 GB、zigbuild 的主機端 build script `zynq` 329 MB；`cargo clean` 回報移除 9,612 個檔案、共 3.2 GiB，交叉編譯的 target 也在同一個資料夾裡一起清掉）；刪除本篇執行時留在 `/dev/shm` 的 zenoh 殘檔 `1356930607.zenoh`（12 KB，時間戳 08:07，在本篇 `dora run` 期間；開始前 `/dev/shm` 是空的，見 `run/shm-before.txt`，只刪這一個）；確認沒有殘留的 dora、ffmpeg、qemu-aarch64 或 lesson-20 行程（`pgrep` 找不到）。另外：取消本篇註冊的 binfmt `qemu-aarch64`（`echo -1 > …/qemu-aarch64`），並卸載本篇掛上的 `binfmt_misc`（開始前沒有掛載）。保留：原始碼、各 yml 與腳本、`run/` 裡的 log、`display.mp4`（5.3 MB）、PNG、截圖、兩個假 framebuffer 檔（1.9 MB 與 0.96 MB），以及 dora 自己寫在 `out/` 的 session 紀錄（`du`：`run/` 11 MB、`out/` 168 KB）。

## 20 篇總複習／後續學習方向（接到 FPGA CNN 加速）

### 這 20 篇學了什麼

| 階段 | 篇 | 主題 | 在這個專題裡用到的地方 |
|---|---|---|---|
| 入門 | 1 | dora-rs 是什麼、跟 ROS 2 差在哪 | 為什麼選 dataflow：節點各自獨立、介面清楚 |
| | 2 | 安裝 CLI、跑官方範例 | `dora` CLI（`dora run`、`dora build`、`dora validate`） |
| | 3 | 逐行讀懂 dataflow.yml | 每一份 yml：`build`、`path`、`env`、`inputs`、`outputs` |
| | 4 | 第一個 Rust 節點：`DoraNode` 與事件迴圈 | 三個節點都是 `while let Some(ev) = events.recv()` |
| | 5 | Arrow 資料與 metadata | `image` 是 UInt8、`result` 是 Float32；`seq`、`t_ns`、外框都放 metadata |
| | 6 | `dora up/start/logs/down` 生命週期 | 板子上長時間跑時用 `dora up`＋`dora start`，不用 `dora run` |
| 管線 | 7 | 扇入、扇出 | camera 的 `image` 扇出給 detect 和 display；display 扇入兩個輸入 |
| | 8 | 計時器與背壓 | camera 用 `dora/timer/millis/33`；佇列滿了就丟舊的 |
| | 9 | 錯誤處理與重啟 | 來源打不開時 camera 回傳錯誤、dataflow 失敗，而不是假裝成功 |
| | 10 | 零拷貝大資料 | `send_output_raw` 直接寫進共享記憶體；76.8 KB > 4096 門檻 |
| | 11 | Operator vs Node | 本專題全部用 Node（各自一個行程，當掉不會拖累別人） |
| | 12 | 混用 Python 節點 | 想快速試別的模型時，detect 可以先用 Python 寫 |
| | 13 | 測試與除錯 | 畫圖、找框、點陣字都是純函式，可以不開 daemon 做單元測試（本篇沒寫測試，見下方建議） |
| 部署 | 14 | 分散式：coordinator 與 daemon | camera＋detect 在板子、display 在 PC 的拆法 |
| | 15 | 交叉編譯 aarch64 | 本篇實測輸出 4、5 |
| | 16 | 自訂硬體節點：UART、共享記憶體、FPGA DMA | 重點說明 6 的第 3 條路 |
| AI 與效能 | 17 | candle／ort 推論節點 | detect 的模型 |
| | 18 | 效能調校：先量再改 | `t_ns` 時間戳、p50/p99、`queue_size: 1` |
| 專題 | 19 | 綜合專題（上）：來源、辨識、量測 | camera、detect |
| | 20 | 綜合專題（下）：顯示、整合、部署 | display、aarch64、延遲預算 |

如果只能記三件事：

1. **介面定在資料與 metadata 上**：節點只看 `image`／`result` 長什麼樣，不管上游是合成、影片、攝影機還是 FPGA DMA。本篇換掉整個輸出端，前面兩個節點一行都沒改。
2. **先量再改**：每個環節都帶時間戳，用 p50／p99 說話。本篇的量測告訴我們：這個小模型在 x86 上整條軟體管線只要約 3 ms，真正的大頭是攝影機與螢幕。
3. **「最新的一張」比「每一張」重要**：辨識節點 `queue_size: 1`；跟不上時丟幀，延遲不會越積越大。

### 後續學習方向：從這條管線走到 FPGA 上的 CNN 加速

把 detect 換成 FPGA 加速，**dora 這一側幾乎不用動**：新的 detect 節點一樣收 `image`、送 `result`，只是中間從「呼叫 candle」變成「把資料交給 PL、等它算完」。建議的順序（每一步都可以獨立驗證、獨立量測）：

1. **先在 PS 上跑通、量出基準**：把本篇的 aarch64 版放上真的板子，填好重點說明 7 的預算表。沒有基準，就無法證明 FPGA 版比較快。
2. **把模型改成整數（量化）**：FPGA 上做 int8 乘加比 float32 省非常多資源。先在 PC 上把第 17 篇的模型量化成 int8（例如 PyTorch／ONNX 的量化工具），**用同一組 MNIST 測試資料比較正確率掉多少**，再寫一個「用整數算」的 Rust 參考實作當作黃金模型（golden model）。之後 FPGA 算出來的每一層結果，都拿它來逐位元比對。
3. **用 HLS 寫一個卷積層**：Vitis HLS 可以用 C++ 寫 conv 5×5，綜合成 RTL，在 C 模擬（csim）與 C／RTL 協同模擬（cosim）裡和黃金模型比對。重點概念：迴圈展開（unroll）＝並行的乘加器數量、管線化（pipeline，II=1）、line buffer（影像只讀一次）、權重放 BRAM。重點說明 7 的「每個時脈幾個 MAC」就是在這一步決定的。
4. **接上 AXI**：控制暫存器用 AXI4-Lite（`s_axilite`），資料用 AXI4-Stream 或 AXI4 master（`m_axi`）。在 Vivado 用 Block Design 把它和 AXI DMA、ZYNQ PS 接起來，產生 bitstream 與 `.xsa`，再給 PetaLinux 做 device tree（第 16 篇的 UIO、u-dma-buf 就在這裡用上）。
5. **寫 FPGA 版的 detect 節點**：沿用第 16 篇 `dma-node` 的寫法——MM2S 把 28×28 的裁切圖（或整張畫面）送進 PL、S2MM 收回 10 個分數、UIO 等中斷、注意 cache 一致性。metadata 照本篇的格式送出，display 不用改，就能在畫面上直接看到「FPGA 算的」結果與延遲。
6. **整條搬進 PL**：最後可以把找框、裁切也做進 PL（本篇的 `locate` 就是一個很適合做成硬體的「掃描一遍、記最大最小座標」電路），CPU 只負責顯示與管理。

另外兩條值得了解的路：

- **Vitis AI＋DPU**（ZYNQ MPSoC／Kria）：Xilinx 現成的 CNN 加速 IP，把訓練好的模型用工具鏈量化、編譯就能跑，不用自己寫 HLS。適合「要快速做出產品」；自己寫 HLS 適合「要學懂硬體在做什麼」或模型很特殊時。（第 17 篇提過，本系列沒有實測。）
- **hls4ml**：把小型神經網路直接轉成 HLS 程式碼的開源工具，很適合像本系列這種 MNIST 等級的小模型練手。（本系列沒有實測。）

建議的學習材料方向：Vitis HLS 使用者指南（UG1399）、AXI DMA 產品指南（PG021，第 16 篇已引用）、Xilinx 的 Vitis-Tutorials（GitHub）、PetaLinux 工具參考（UG1144）。另外，第 13 篇的測試方法可以馬上用在本篇：`glyph`、`Canvas::rect`、`row_to_fb` 都是純函式，很適合先補上單元測試，這也是之後「FPGA 結果 vs 黃金模型」比對的雛形。

這個系列到這裡結束。從第 1 篇的 `dora run` 到本篇在 aarch64 上跑完整條「攝影機 → 辨識 → 顯示」，各篇的範例程式都是在 box 上實際編譯、執行過的；沒跑成的地方，每篇的「本篇沒有驗證成功的部分」都有誠實記下來，那些就是上了真板子之後最先要補驗證的清單。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 19 篇](lesson-19.md)
- 下一課：無——第 20 篇是**最終篇**，系列到此結束
- 本課範例：[examples/lesson-20/](../../examples/lesson-20/)
