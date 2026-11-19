# dora-rs 教學 第 16 篇：自訂硬體節點——序列埠、共享記憶體與 FPGA DMA（概念＋模擬）

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0），rustc 1.99.1，`serialport` 4.10.1（關掉預設 feature），`memmap2` 0.9.12。驗證日期：2026-11-16（台北時間），只在 x86_64 Linux 虛擬機上跑。**這台機器沒有實體序列埠、沒有 ZYNQ 板、沒有 FPGA**：UART 用 Rust 寫的 `openpty` 模擬裝置代替（機器上也沒有 socat），DMA 暫存器、DMA buffer、PL 中斷都是用一般檔案、`mmap` 和 FIFO 模擬的，**不是真的硬體**。實測：四個節點加上模擬器一起跑 12 秒，`dora run` 結束碼 0。UART 節點收到 401 行，算錯檢查碼的 12 行都丟掉了；模擬拔線之後，節點用指數退避自動重連 1 次。共享記憶體節點送出 1132 格（67.9 MB），內容錯 0 格，序號缺口 0。DMA 節點送出 446 塊，格式錯 0；故意漏掉的 11 次中斷都觸發逾時並重設成功。背壓實驗裡，三種策略（丟最新、覆蓋最舊、讓上游停下來）都看得到預期的計數。**沒驗證**：真的 UART／ZYNQ／UIO／u-dma-buf／`/dev/mem`；socat；dora 往下游送資料時的零拷貝——用 `/proc/self/maps` 檢查，接收端拿到的資料在**匿名記憶體**，不在 `/dev/shm`（見文末）。

> **關於日誌：** 本篇引用的 `run/*.log` 等完整執行日誌**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。範例可依「怎麼跑／執行」小節自行重現。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 15 篇](lesson-15.md)
- 下一課：[第 17 篇 →](lesson-17.md)
- 本課範例：[examples/lesson-16/](../../examples/lesson-16/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [常見錯誤](#常見錯誤)
- [小練習](#小練習)
- [本篇沒有驗證成功的部分](#本篇沒有驗證成功的部分)
## 重點說明

第 15 篇把節點交叉編譯到 ZYNQ 上了。真正上板子之後，節點幾乎都要碰硬體：讀 UART 感測器、跟廠商的程式交換大量資料、把 FPGA（PL 端）用 DMA 寫進 DDR 的資料送進 dataflow。這一篇講怎麼把這類節點寫穩。我們用模擬器把每一條錯誤路徑都實際跑過一次。

### 1. 硬體節點的設計原則

dora 節點的主迴圈是 `events.recv()`。硬體 I/O 的麻煩是**會阻塞**（等 UART 下一個 byte、等 FPGA 中斷），而且**會出錯**（線被拔掉、DMA 卡住）。這一篇的三個節點都照同一套規則寫：

| 原則 | 為什麼 | 本篇做法 |
|---|---|---|
| **I/O 執行緒與 dora 事件迴圈分開** | 主迴圈卡在 `read()` 上，就收不到 `Stop`，daemon 只能等寬限期結束後強制砍掉節點；dora 的 timer 事件也會堆積 | 會阻塞的 I/O（UART 讀取、等中斷）一律放到自己的執行緒；主迴圈只處理 dora 事件 |
| **用 channel 交接，不共用可變狀態** | 兩條執行緒各做各的，不用鎖 | 資料走**有界** `sync_channel`；狀態和錯誤走另一條**無界** channel（訊息很少，而且不能被背壓丟掉）；計數器用 `Arc<AtomicU64>` |
| **每個等待都要有逾時** | 硬體不回應時，執行緒要能醒來檢查 `stop`，也要能判斷「裝置失聯」 | 序列埠 `read` 100 ms、靜默看門狗 500 ms、`poll` 等中斷 200 ms、等 DMA reset 完成 100 ms |
| **重連／重設要自動** | 現場的 USB 線會鬆、FPGA 會卡 | UART 用指數退避（100 ms 起跳、加倍、上限 2 s）；DMA 中斷逾時就軟體重設再重新開始 |
| **背壓要明確選一種** | 硬體不會等你。消費端慢下來時，資料一定得丟在某個地方，要自己決定丟在哪 | UART：channel 滿了**丟最新的**並計數；共享記憶體環形緩衝：寫者**覆蓋最舊的**，讀者偵測到後跳過；DMA：只有兩塊 buffer（ping-pong），沒還回來就**讓上游停下來** |
| **錯誤要回報，不要只印出來** | 下游和維運人員都要知道感測器現在是什麼狀態 | 每個節點都多一個 `status` 輸出，送出連線、斷線、逾時、重設等事件；結束時印出所有計數器 |
| **能恢復的錯誤和致命錯誤分開處理** | 重試沒有意義的錯誤（例如設定寫錯）應該讓節點直接失敗，daemon 才會照第 9 篇的 `restart_policy` 處理 | 開檔、`mmap` 失敗時 `main` 直接回傳 `Err`；斷線、逾時只記錄下來然後重試 |

背壓這件事和第 8 篇不同。第 8 篇講的是 **dora 節點之間**的背壓（`queue_size`、`backpressure` 輸入），這一篇講的是**硬體和節點之間**的背壓：dora 管不到這一段，要自己處理。

### 2. 序列埠節點（`serialport` crate）

- `serialport::new(path, baud).timeout(100ms).open()`。在 Linux 上它會設 raw mode、用 `TIOCEXCL` 加 `flock` 獨占這個埠。**斷線後一定要讓 port 被 drop**（也就是關掉 fd），之後才開得起來。
- `Cargo.toml` 要寫 `default-features = false`。預設的 `libudev` feature 只有在列舉 USB 埠（`available_ports()`）時才用得到，卻要求系統裝好 `pkg-config` 和 `libudev-dev`。這台機器開著預設 feature 編譯，會失敗在 `libudev-sys`（見「常見錯誤」）；ZYNQ 交叉編譯時更麻煩。
- 讀取：逾時會回傳 `ErrorKind::TimedOut`，這是正常狀況，繼續讀就好。收到的 bytes 自己累積，再用 `\n` 切成一行一行。**開埠後的第一個換行之前可能只是半行**，要丟掉；緩衝超過 1 KB 還沒看到換行，就當成雜訊或鮑率設錯，直接清空。
- 協定：本篇用 NMEA 風格的文字行 `$T,<seq>,<毫攝氏度>*<XOR 檢查碼>`。二進位協定常用 COBS／SLIP 切 frame，再加 CRC。
- **沒有實體裝置時**：用 socat 開一對互連的 pty，例如 `socat -d -d pty,raw,echo=0,link=/tmp/ttyV0 pty,raw,echo=0,link=/tmp/ttyV1`，一端給節點、一端給測試程式。這台機器沒有 socat（本篇沒有實測這條指令），所以本篇的 `fake-hw` 直接呼叫 `openpty(3)` 開 pty，再用 symlink 掛成 `/tmp/lesson16/ttyFAKE0`，效果和 socat 的 `link=` 一樣。「拔線」就是把 master 端關掉，再開一個新的 pts。
- ZYNQ 上常見的裝置名稱：PS UART 是 `/dev/ttyPS0`（通常是 console）、`/dev/ttyPS1`；PL 裡的 AXI UART Lite 是 `/dev/ttyUL0`；USB 轉序列是 `/dev/ttyUSB0`、`/dev/ttyACM0`。使用者要在 `dialout` 群組裡才有權限；可以用 udev 規則固定一個 symlink，USB 埠換了位置，名稱也不會變。

### 3. 共享記憶體（`/dev/shm` ＋ `memmap2` 環形緩衝）

適用情境：資料來源**不是 dora 節點**，例如廠商的 C++ SDK、另一個使用者帳號底下的服務、不能改的舊程式，但它每秒要交出幾十 MB 的資料。如果來源能改寫成 dora 節點，直接改成節點最好，就不需要這一層。

- `/dev/shm/xxx` 是 tmpfs 上的檔案，兩邊都用 `MmapRaw::map_raw` 映射同一個檔案，就是共享記憶體（POSIX `shm_open` 底層也是這樣）。
- 本篇的版面是**單一寫者的 seqlock 環形緩衝**：header 放 `magic`（最後才寫，讀者看到 magic 才算初始化完成）、格數、每格大小、`write_seq`（`AtomicU64`）。每一格開頭放 `slot_seq`。寫者先把 `slot_seq` 設成「寫入中」，寫完資料再把它設成這一格的 seq（Release）。讀者讀之前、讀之後各看一次 `slot_seq`，兩次都等於自己要的 seq 才算有效。
- 讀者落後超過一圈（`write_seq - next > 格數`），代表舊格子已經被蓋掉，直接跳到最舊的有效格並計入 `overrun`。這就是「覆蓋最舊的」背壓。
- **和 dora 自己的共享記憶體是什麼關係**：dora 1.1.0 的資料平面是 zenoh。大於 4096 byte（`ZERO_COPY_THRESHOLD`）的輸出，`send_output_raw` 會從 zenoh 的 SHM pool 配一塊 sample，讓你直接寫進去（第 10 篇）。外部行程沒辦法直接寫 dora 的 pool，所以邊界上一定會有**一次複製**：外部 ring → `send_output_raw` closure 給的 `dst`。本篇把複製寫在 closure 裡，所以只複製這一次，沒有中間的 `Vec`。
  照原始碼註解，同一台機器上的下游應該直接讀那塊 SHM（零拷貝）。但本篇用 `/proc/self/maps` 檢查 monitor 收到的指標，結果在**匿名記憶體**（heap 或執行緒 arena），不是 `/dev/shm/*.zenoh`。也就是說，在這台機器上下游拿到的是複本，原因還沒查清楚（見文末）。
- 讀 ring 不會阻塞（只是讀記憶體），所以 `shm-node` **沒有另開執行緒**，每個 5 ms 的 tick 把新的格子讀完就好。不是每個硬體節點都要開執行緒，只有會**阻塞**的 I/O 才需要。
- 記憶體順序：CPU 和 CPU 之間的 cache 由硬體維持一致，x86 和 ARM 都只需要正確的 Acquire／Release，**不需要 flush cache**（這一點和 DMA 不同）。資料本體是一般的非原子複製，在 Rust 的記憶體模型裡技術上屬於 data race。這是 seqlock 的已知取捨，所以讀完一定要再驗一次 seq。正式產品可以考慮用 `ptr::read_volatile`，或專門的 seqlock crate。

### 4. FPGA DMA 概念（ZYNQ AXI DMA）

ZYNQ 分成 PS（ARM 處理器、DDR 控制器）和 PL（FPGA）兩部分。PL 產生的高速資料（ADC、相機、自訂演算法的結果）通常用 AXI4-Stream 接到 Xilinx 的 **AXI DMA** IP（PG021）。DMA 的 **S2MM** 通道（Stream to Memory-Mapped）把串流資料寫進 DDR，寫完後發出中斷。

本篇用的是 S2MM **direct register mode**（沒有 scatter-gather）的暫存器：

| 偏移 | 名稱 | 用途 |
|---|---|---|
| 0x30 | S2MM_DMACR | bit0 RS（run/stop）、bit2 Reset、bit12 IOC_IrqEn（傳完要不要發中斷）、bit14 Err_IrqEn |
| 0x34 | S2MM_DMASR | bit0 Halted、bit1 Idle、bit4～6 內部／Slave／Decode 錯誤、bit12 IOC_Irq、bit14 Err_Irq（中斷位元要**寫 1 清除**，W1C） |
| 0x48 / 0x4C | S2MM_DA / DA_MSB | 目的地的**實體位址**（位址寬度超過 32 位元時才有 MSB） |
| 0x58 | S2MM_LENGTH | 要傳幾個 byte；**寫入這個暫存器就是開始傳輸**，傳完之後讀回來是實際傳了幾個 byte |

一次傳輸的流程（`dma-node.rs` 的 `Hw` 就照這個順序）：寫 Reset 並等它自己清掉 → 寫 DMACR = RS | IOC_IrqEn | Err_IrqEn → 寫 DA → 寫 LENGTH（開始）→ 等中斷 → 讀 DMASR 檢查錯誤 → 對 IOC_Irq 寫 1 清除 → **讓 CPU 端的 cache 失效** → 讀資料。

Linux 上從 user space 碰這些東西有幾種方法：

1. **`/dev/mem` ＋ `mmap`**：用 root 權限把實體位址（例如 ZYNQ-7000 的 Vivado 預設 AXI DMA 位址 `0x40400000`）直接映射進來。開檔時要加 `O_SYNC`，映射才不會經過 cache。這種方法收不到中斷，而且寫錯位址可能把整台機器弄當，只適合拿來快速驗證。
2. **UIO（`generic-uio`）**：在 device tree 裡把 DMA 節點的 `compatible` 改成 `generic-uio`，開機參數加 `uio_pdrv_genirq.of_id=generic-uio`。之後 `/dev/uio0` 可以 `mmap`（offset 0 就是暫存器 map0），`read()` 會阻塞到中斷發生，回傳一個 u32 的累計中斷次數。每次等中斷前要先 `write(1u32)` 重新開啟中斷（irqcontrol）。等的時候用 `poll(2)` 就能設逾時。
3. **u-dma-buf**（第三方核心模組）：配置一塊**實體上連續**的 DMA buffer，`/dev/udmabuf0` 可以 `mmap`，實體位址從 `/sys/class/u-dma-buf/udmabuf0/phys_addr` 讀，cache 同步透過 `sync_offset`、`sync_size`、`sync_direction`、`sync_for_cpu` 這幾個 sysfs 檔案觸發。一般 `malloc` 出來的記憶體在實體上不連續，位址也會被換出，**不能交給 DMA**。
4. **核心驅動**（`xilinx_dma` ＋ dmaengine，或自己寫 char driver／IIO）：正式產品的做法，user space 只看到 `read()` 或 `mmap` 介面。本篇不談。

**cache coherency**（最常踩的坑）：DMA 直接寫 DDR，不經過 CPU 的 cache。CPU 如果還留著那段位址的舊 cache line，就會讀到舊資料。

- ZYNQ-7000 的 **HP0～HP3** 埠**不維持 cache 一致**：CPU 讀之前要讓 cache 失效（invalidate），CPU 寫完要交給 DMA 讀之前要 flush。用 u-dma-buf 的 `sync_for_cpu`，或者用 `O_SYNC` 建立不經過 cache 的映射（比較慢）。
- **ACP** 埠和 ZYNQ UltraScale+ 的 **HPC0／HPC1**（經過 CCI-400）**可以維持一致**，但 device tree 要加 `dma-coherent`，PL 端的 AxCACHE／AxPROT 也要設對，才真的生效。

本篇的 `dma-node` 一律呼叫 `sync_for_cpu()`，在一致的埠上可以拿掉。資料 header 裡有 magic，cache 沒處理好時多半會被這個檢查抓到（「buffer 的 header 不對」）。

**PL 中斷**：PL 的中斷線接到 PS 的 `IRQ_F2P`，進到 GIC，device tree 的 `interrupts = <0 29 4>` 指定中斷號，UIO 再把它變成 `read()` 可以等的事件。

**本篇的模擬對照**（再說一次：不是真的硬體）：

| 真的 | 本篇模擬（`fake-hw`） | 差異 |
|---|---|---|
| `/dev/uio0` map0（AXI DMA 暫存器） | `/tmp/lesson16/axi_dma_regs`（4 KB 一般檔案，`mmap`） | 一般記憶體沒有「寫入就觸發動作」和 W1C 的行為。所以模擬器**看到 LENGTH≠0 就接下這次傳輸，並把 LENGTH 歸零**；節點在 `DMA_SIM=1` 時直接把 IOC 位元清成 0，而不是寫 1 清除 |
| u-dma-buf `/dev/udmabuf0` ＋ sysfs | `/tmp/lesson16/udmabuf0`（1 MB）＋ `/tmp/lesson16/sys/{phys_addr,size,sync_*}` | 實體位址 `0x3f000000` 是假的；`sync_for_cpu` 只是把字串寫進普通檔案，沒有真的讓 cache 失效 |
| `/dev/uio0` 的 `read()` 中斷 | FIFO `/tmp/lesson16/uio0`，模擬器每完成一塊就寫入 u32 累計次數 | 模擬時跳過 `write(1)`（不然寫進 FIFO 的資料會被自己讀回來） |
| PL 的 ADC ＋ AXI-Stream | 模擬器寫入正弦波 i16 樣本，前面加 24 byte header（magic、frame、樣本數、時間戳記）。這個 header 是本篇自訂的格式 | 每 40 次傳輸故意不完成、不發中斷，模擬 PL 沒送出 tlast、DMA 卡住的狀況 |

## 完整範例

位置：[examples/lesson-16/](../../examples/lesson-16/)

```
examples/lesson-16/
├── Cargo.toml
├── dataflow.yml                # 正常設定：uart / shm / dma → monitor
├── dataflow-backpressure.yml   # 故意讓消費端變慢，觀察三種背壓策略
├── run-demo.sh                 # 背景啟動 fake-hw → dora run 12 s → 收掉 fake-hw
├── run/                        # 實測 log
└── src/
    ├── lib.rs                  # UART 協定、shm ring 版面、AXI DMA 暫存器表
    └── bin/
        ├── fake-hw.rs          # ⚠️ 模擬硬體（不是 dora 節點）
        ├── uart-node.rs        # 序列埠節點：I/O 執行緒 + 重連 + 丟最新
        ├── shm-node.rs         # 共享記憶體節點：seqlock ring → send_output_raw
        ├── dma-node.rs         # AXI DMA S2MM 節點：UIO 中斷 + ping-pong + 重設
        └── monitor.rs          # 下游：驗證內容、序號缺口、延遲、資料位址落在哪個 mapping
```

### Cargo.toml

```toml
[package]
name = "lesson-16"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
libc = "0.2"
memmap2 = "0.9"
# 不要預設的 libudev（只有列舉 USB 序列埠時才需要；ZYNQ rootfs 常沒有 libudev-dev）
serialport = { version = "4.10", default-features = false }
```

### src/lib.rs

```rust
//! 第 16 篇共用程式：UART 文字協定、/dev/shm 環形緩衝的版面、AXI DMA 暫存器表、路徑設定。
//! 注意：本專案的「硬體」全部是 fake-hw 模擬出來的（pty、/dev/shm 檔案、一般檔案 mmap、FIFO），
//! 不是真的 UART／FPGA。程式結構刻意寫成換成真裝置時只改路徑與少數標了 SIM 的地方。
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 模擬硬體的檔案都放這裡（可用環境變數 LESSON16_DIR 改）。
pub fn sim_dir() -> PathBuf {
    std::env::var_os("LESSON16_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/lesson16"))
}

/// 牆上時鐘（ns），用來量「裝置寫入 → monitor 收到」的延遲（同一台機器才有意義）。
pub fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

// ───────────────────────── UART 協定 ─────────────────────────
/// 一行一筆：`$T,<seq>,<毫攝氏度>*<XOR 檢查碼兩位十六進位>\r\n`（NMEA 風格）。
pub mod proto {
    pub fn xor_cs(body: &[u8]) -> u8 {
        body.iter().fold(0u8, |a, b| a ^ b)
    }

    pub fn encode(seq: u32, milli_c: i32) -> String {
        let body = format!("T,{seq},{milli_c}");
        format!("${body}*{:02X}\r\n", xor_cs(body.as_bytes()))
    }

    #[derive(Debug, PartialEq)]
    pub enum ParseError {
        Framing,  // 沒有 $ 或 *，通常是開機／重連時接到半行
        Checksum, // 線路雜訊
        Field,    // 格式對但欄位不是數字
    }

    /// 不含行尾 \r\n 的一行。
    pub fn parse(line: &[u8]) -> Result<(u32, i32), ParseError> {
        let line = line.strip_prefix(b"$").ok_or(ParseError::Framing)?;
        let star = line.iter().rposition(|&b| b == b'*').ok_or(ParseError::Framing)?;
        let (body, cs) = (&line[..star], &line[star + 1..]);
        let cs = std::str::from_utf8(cs).ok().and_then(|s| u8::from_str_radix(s, 16).ok());
        if cs != Some(xor_cs(body)) {
            return Err(ParseError::Checksum);
        }
        let s = std::str::from_utf8(body).map_err(|_| ParseError::Field)?;
        let mut it = s.split(',');
        match (it.next(), it.next(), it.next(), it.next()) {
            (Some("T"), Some(seq), Some(v), None) => Ok((
                seq.parse().map_err(|_| ParseError::Field)?,
                v.parse().map_err(|_| ParseError::Field)?,
            )),
            _ => Err(ParseError::Field),
        }
    }
}

// ───────────────────── /dev/shm 環形緩衝 ─────────────────────
/// 單一寫者、多讀者的 seqlock 環形緩衝。檔案版面：
/// ```text
/// 0x00 magic u64 | 0x08 n_slots u32 | 0x0C slot_size u32 | 0x10 write_seq AtomicU64 | …填到 64
/// 每個 slot（slot_size bytes）：0x00 slot_seq AtomicU64 | 0x08 payload
/// payload：0x00 frame_seq u64 | 0x08 t_ns u64 | 0x10 len u32 | 0x14 pad | 0x18 資料…
/// ```
pub mod ring {
    use memmap2::MmapRaw;
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering, fence};

    pub const MAGIC: u64 = 0x4C31_3652_494E_4701; // "L16RING" + 版本 1
    pub const HDR: usize = 64;
    pub const SLOT_HDR: usize = 8;
    pub const PAYLOAD_HDR: usize = 24;
    const WRITING: u64 = u64::MAX;

    pub struct Ring {
        map: MmapRaw,
        pub n_slots: usize,
        pub slot_size: usize,
    }

    impl Ring {
        /// 寫者（模擬的驅動／相機程式）建立並清空。
        pub fn create(path: &Path, n_slots: usize, slot_size: usize) -> std::io::Result<Ring> {
            let f = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
            f.set_len((HDR + n_slots * slot_size) as u64)?;
            let map = MmapRaw::map_raw(&f)?;
            let r = Ring { map, n_slots, slot_size };
            unsafe {
                let p = r.map.as_mut_ptr();
                (p.add(0x08) as *mut u32).write(n_slots as u32);
                (p.add(0x0C) as *mut u32).write(slot_size as u32);
            }
            r.write_seq().store(0, Ordering::Relaxed);
            fence(Ordering::Release);
            // magic 最後寫：讀者看到 magic 才代表版面已經初始化好
            unsafe { (r.map.as_mut_ptr() as *const AtomicU64).as_ref().unwrap() }.store(MAGIC, Ordering::Release);
            Ok(r)
        }

        /// 讀者（dora 節點）附掛。檔案不在或還沒初始化 → Err，呼叫端稍後重試。
        pub fn open(path: &Path) -> std::io::Result<Ring> {
            let f = OpenOptions::new().read(true).write(true).open(path)?;
            if f.metadata()?.len() < HDR as u64 {
                return Err(std::io::Error::other("ring 太小（還在建立中？）"));
            }
            let map = MmapRaw::map_raw(&f)?;
            let p = map.as_ptr();
            let magic = unsafe { (p as *const AtomicU64).as_ref().unwrap() }.load(Ordering::Acquire);
            if magic != MAGIC {
                return Err(std::io::Error::other(format!("magic 不對：{magic:#x}")));
            }
            let (n_slots, slot_size) = unsafe {
                ((p.add(0x08) as *const u32).read() as usize, (p.add(0x0C) as *const u32).read() as usize)
            };
            if map.len() < HDR + n_slots * slot_size {
                return Err(std::io::Error::other("檔案長度和 header 不符"));
            }
            Ok(Ring { map, n_slots, slot_size })
        }

        fn atomic(&self, off: usize) -> &AtomicU64 {
            // SAFETY: off 是 8 的倍數、在 mapping 內；mmap 起點按頁對齊。
            unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU64) }
        }
        pub fn write_seq(&self) -> &AtomicU64 {
            self.atomic(0x10)
        }
        fn slot_off(&self, seq: u64) -> usize {
            HDR + (seq as usize % self.n_slots) * self.slot_size
        }
        pub fn max_payload(&self) -> usize {
            self.slot_size - SLOT_HDR
        }

        /// 寫者：寫入第 seq 筆（seq 從 0 遞增）。`fill` 拿到整個 payload 區、回傳實際用掉幾 byte。
        pub fn push(&self, seq: u64, fill: impl FnOnce(&mut [u8])) {
            let off = self.slot_off(seq);
            let slot_seq = self.atomic(off);
            slot_seq.store(WRITING, Ordering::Relaxed);
            fence(Ordering::Release);
            let buf = unsafe {
                std::slice::from_raw_parts_mut(self.map.as_mut_ptr().add(off + SLOT_HDR), self.max_payload())
            };
            fill(buf);
            slot_seq.store(seq, Ordering::Release);
            self.write_seq().store(seq + 1, Ordering::Release);
        }

        /// 讀者：把第 seq 筆交給 `f`（f 通常把資料複製進 dora 的輸出 sample）。
        /// 回傳 false 代表這格在讀的過程中被寫者蓋掉了（讀者太慢），f 的結果要丟掉。
        pub fn read(&self, seq: u64, f: impl FnOnce(&[u8])) -> bool {
            let off = self.slot_off(seq);
            let slot_seq = self.atomic(off);
            if slot_seq.load(Ordering::Acquire) != seq {
                return false;
            }
            let buf = unsafe { std::slice::from_raw_parts(self.map.as_ptr().add(off + SLOT_HDR), self.max_payload()) };
            f(buf);
            fence(Ordering::Acquire);
            slot_seq.load(Ordering::Relaxed) == seq
        }
    }
}

// ─────────────────── Xilinx AXI DMA（PG021）S2MM ───────────────────
/// 只列本篇用到的 S2MM（Stream to Memory-Mapped，也就是 PL → DDR）direct register mode 暫存器。
pub mod axidma {
    pub const S2MM_DMACR: usize = 0x30;
    pub const S2MM_DMASR: usize = 0x34;
    pub const S2MM_DA: usize = 0x48;
    pub const S2MM_DA_MSB: usize = 0x4C;
    pub const S2MM_LENGTH: usize = 0x58;

    pub const CR_RS: u32 = 1 << 0;
    pub const CR_RESET: u32 = 1 << 2;
    pub const CR_IOC_IRQ_EN: u32 = 1 << 12;
    pub const CR_ERR_IRQ_EN: u32 = 1 << 14;

    pub const SR_HALTED: u32 = 1 << 0;
    pub const SR_IDLE: u32 = 1 << 1;
    pub const SR_DMA_INT_ERR: u32 = 1 << 4;
    pub const SR_DMA_SLV_ERR: u32 = 1 << 5;
    pub const SR_DMA_DEC_ERR: u32 = 1 << 6;
    pub const SR_IOC_IRQ: u32 = 1 << 12;
    pub const SR_ERR_IRQ: u32 = 1 << 14;
    pub const SR_ERRORS: u32 = SR_DMA_INT_ERR | SR_DMA_SLV_ERR | SR_DMA_DEC_ERR | SR_ERR_IRQ;

    /// PL 串流進來的每一塊資料前面放的 header（這是本篇自訂的 FPGA 格式，不是 Xilinx 規定）。
    /// 0x00 magic u32 | 0x04 frame u32 | 0x08 n_samples u32 | 0x0C pad | 0x10 t_ns u64 | 0x18 i16 樣本…
    pub const FRAME_MAGIC: u32 = 0xADC0_FEED;
    pub const FRAME_HDR: usize = 24;

    /// MMIO 暫存器視窗：一律 volatile 存取，一次 32 位元，不能用 memcpy／一般 load。
    pub struct Regs {
        map: memmap2::MmapRaw,
    }

    impl Regs {
        pub fn new(map: memmap2::MmapRaw) -> Self {
            assert!(map.len() >= 0x100);
            Regs { map }
        }
        pub fn read(&self, off: usize) -> u32 {
            unsafe { std::ptr::read_volatile(self.map.as_ptr().add(off) as *const u32) }
        }
        pub fn write(&self, off: usize, v: u32) {
            unsafe { std::ptr::write_volatile(self.map.as_mut_ptr().add(off) as *mut u32, v) }
        }
    }
}
```

### src/bin/uart-node.rs

```rust
//! 序列埠（UART）硬體節點。
//! 執行緒分工：
//!   I/O 執行緒：開埠、阻塞讀取（有逾時）、切行、驗檢查碼、斷線重連（指數退避）。
//!   主執行緒（dora 事件迴圈）：只處理 dora 事件；每個 tick 把 channel 裡累積的讀值一次送出。
//! 兩邊只靠有界 channel（背壓：滿了丟最新的一筆並計數）和 Arc<Stats> 溝通。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_16::proto::{self, ParseError};
use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Stats {
    lines: AtomicU64,
    framing: AtomicU64,
    checksum: AtomicU64,
    dropped: AtomicU64, // 背壓：channel 滿了丟掉的
    reconnects: AtomicU64,
}

enum Msg {
    Reading { seq: u32, milli_c: i32 },
    Status(String), // 錯誤／狀態：走另一條無界 channel，不會被背壓丟掉
}

struct Cfg {
    path: String,
    baud: u32,
    read_timeout: Duration, // 單次 read 最多等多久（讓執行緒能定期檢查 stop）
    silence: Duration,      // 多久沒收到完整一行就當作裝置失聯、主動重連
}

fn io_thread(cfg: Cfg, data: SyncSender<Msg>, status: mpsc::Sender<Msg>, st: Arc<Stats>, stop: Arc<AtomicBool>) {
    let mut backoff = Duration::from_millis(100);
    let mut first = true;
    while !stop.load(Relaxed) {
        let mut port = match serialport::new(&cfg.path, cfg.baud).timeout(cfg.read_timeout).open() {
            Ok(p) => p,
            Err(e) => {
                let _ = status.send(Msg::Status(format!("開啟 {} 失敗：{e}；{backoff:?} 後重試", cfg.path)));
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(2));
                continue;
            }
        };
        if !first {
            st.reconnects.fetch_add(1, Relaxed);
        }
        first = false;
        backoff = Duration::from_millis(100);
        let _ = status.send(Msg::Status(format!("已連線 {} @ {} baud", cfg.path, cfg.baud)));
        let _ = port.clear(serialport::ClearBuffer::Input); // 丟掉開埠前殘留的舊資料
        let (mut acc, mut buf) = (Vec::with_capacity(256), [0u8; 256]);
        let mut synced = false; // 第一個 '\n' 之前可能是半行，丟掉
        let mut last_good = Instant::now();
        let reason = loop {
            if stop.load(Relaxed) {
                break "stop".to_string();
            }
            if last_good.elapsed() > cfg.silence {
                break format!("{:?} 沒有收到有效資料", cfg.silence);
            }
            let n = match port.read(&mut buf) {
                Ok(0) => break "讀到 EOF（裝置關閉）".into(),
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::TimedOut => continue,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => break format!("讀取錯誤：{e}"),
            };
            acc.extend_from_slice(&buf[..n]);
            while let Some(nl) = acc.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = acc.drain(..=nl).collect();
                if !synced {
                    synced = true;
                    if !line.starts_with(b"$") {
                        continue;
                    }
                }
                let line = line.strip_suffix(b"\r\n").or(line.strip_suffix(b"\n")).unwrap_or(&line);
                match proto::parse(line) {
                    Ok((seq, milli_c)) => {
                        st.lines.fetch_add(1, Relaxed);
                        last_good = Instant::now();
                        if let Err(TrySendError::Full(_)) = data.try_send(Msg::Reading { seq, milli_c }) {
                            st.dropped.fetch_add(1, Relaxed);
                        }
                    }
                    Err(ParseError::Checksum) => _ = st.checksum.fetch_add(1, Relaxed),
                    Err(_) => _ = st.framing.fetch_add(1, Relaxed),
                }
            }
            if acc.len() > 1024 {
                acc.clear(); // 一直沒有換行：雜訊或鮑率設錯，別讓緩衝無限長大
                st.framing.fetch_add(1, Relaxed);
            }
        };
        if reason != "stop" {
            let _ = status.send(Msg::Status(format!("斷線：{reason}；重新連線")));
        }
        // port 在這裡 drop → close(fd)，釋放 TIOCEXCL／flock，重連才開得起來
    }
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let cfg = Cfg {
        path: std::env::var("SERIAL_PORT").unwrap_or_else(|_| lesson_16::sim_dir().join("ttyFAKE0").display().to_string()),
        baud: std::env::var("BAUD").ok().and_then(|b| b.parse().ok()).unwrap_or(115_200),
        read_timeout: Duration::from_millis(100),
        silence: Duration::from_millis(500),
    };
    let cap: usize = std::env::var("QUEUE").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
    let (data_tx, data_rx): (SyncSender<Msg>, Receiver<Msg>) = mpsc::sync_channel(cap);
    let (status_tx, status_rx) = mpsc::channel();
    let st = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let io = {
        let (st, stop) = (st.clone(), stop.clone());
        std::thread::Builder::new().name("uart-io".into()).spawn(move || io_thread(cfg, data_tx, status_tx, st, stop))?
    };
    let (out_temp, out_status) = (DataId::from("temp".to_owned()), DataId::from("status".to_owned()));
    let mut sent = 0u64;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                for m in status_rx.try_iter() {
                    if let Msg::Status(s) = m {
                        eprintln!("[uart] {s}");
                        node.send_output(out_status.clone(), MetadataParameters::new(), s.as_str())?;
                    }
                }
                let (mut seqs, mut vals) = (Vec::new(), Vec::new());
                for m in data_rx.try_iter() {
                    if let Msg::Reading { seq, milli_c } = m {
                        seqs.push(seq);
                        vals.push(milli_c);
                    }
                }
                if !vals.is_empty() {
                    let mut p = MetadataParameters::new();
                    p.insert("first_seq".into(), Parameter::Integer(seqs[0] as i64));
                    p.insert("last_seq".into(), Parameter::Integer(*seqs.last().unwrap() as i64));
                    sent += vals.len() as u64;
                    node.send_output(out_temp.clone(), p, vals)?; // 一個 tick 一批，Arrow Int32 陣列
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    stop.store(true, Relaxed);
    let _ = io.join(); // read 逾時 100 ms，最多等這麼久
    eprintln!(
        "[uart] 結束：有效 {} 行、送出 {sent} 筆、檢查碼錯 {}、半行/格式錯 {}、背壓丟棄 {}、重連 {} 次",
        st.lines.load(Relaxed), st.checksum.load(Relaxed), st.framing.load(Relaxed),
        st.dropped.load(Relaxed), st.reconnects.load(Relaxed)
    );
    Ok(())
}
```

### src/bin/shm-node.rs

```rust
//! 共享記憶體節點：把「別的行程」寫進 /dev/shm 環形緩衝的大塊資料接進 dora。
//! 讀 ring 不會阻塞（只是讀記憶體），所以不另開執行緒：每個 5 ms tick 把新的格子讀完。
//! 每一格只複製一次：ring（外部 shm）→ send_output_raw 給的 dora sample（dora 的 zenoh shm）。
//! 之後 dora 把那塊 shm 交給下游，下游讀到的就是同一塊記憶體（零拷貝）。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_16::ring::{PAYLOAD_HDR, Ring};
use std::path::Path;
use std::time::{Duration, Instant};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let path = std::env::var("RING").unwrap_or_else(|_| "/dev/shm/lesson16-ring".into());
    eprintln!("[shm] zero_copy_threshold = {} bytes", node.zero_copy_threshold());
    let out = DataId::from("frame".to_owned());
    let out_status = DataId::from("status".to_owned());
    let mut ring: Option<Ring> = None;
    let mut last_try = Instant::now() - Duration::from_secs(1);
    let (mut next, mut sent, mut overrun, mut torn) = (0u64, 0u64, 0u64, 0u64);

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                if ring.is_none() {
                    if last_try.elapsed() < Duration::from_millis(100) {
                        continue;
                    }
                    last_try = Instant::now();
                    match Ring::open(Path::new(&path)) {
                        Ok(r) => {
                            next = r.write_seq().load(std::sync::atomic::Ordering::Acquire); // 從最新的開始，不追舊帳
                            let s = format!("附掛 {path}：{} 格 × {} bytes，從 seq={next} 開始", r.n_slots, r.slot_size);
                            eprintln!("[shm] {s}");
                            node.send_output(out_status.clone(), MetadataParameters::new(), s.as_str())?;
                            ring = Some(r);
                        }
                        Err(e) => {
                            eprintln!("[shm] 還不能附掛 {path}：{e}");
                            continue;
                        }
                    }
                }
                let r = ring.as_ref().unwrap();
                let w = r.write_seq().load(std::sync::atomic::Ordering::Acquire);
                if w < next {
                    // 寫者重啟（seq 歸零）：重新同步
                    next = w;
                }
                if w - next > r.n_slots as u64 {
                    overrun += w - next - r.n_slots as u64; // 讀太慢，舊格子已被蓋掉
                    next = w - r.n_slots as u64;
                }
                while next < w {
                    let seq = next;
                    next += 1;
                    // 先看 header 拿長度與時間戳
                    let mut meta = (0u64, 0usize);
                    if !r.read(seq, |p| {
                        meta = (u64::from_le_bytes(p[8..16].try_into().unwrap()),
                                u32::from_le_bytes(p[16..20].try_into().unwrap()) as usize)
                    }) {
                        torn += 1;
                        continue;
                    }
                    let (t_ns, len) = meta;
                    let mut ok = true;
                    let mut p = MetadataParameters::new();
                    p.insert("seq".into(), Parameter::Integer(seq as i64));
                    p.insert("t_ns".into(), Parameter::Integer(t_ns as i64));
                    // send_output_raw 的 closure 拿到的 dst 就是 dora 的 shm sample：只有這一次複製
                    node.send_output_raw(out.clone(), p, len, |dst| {
                        ok = r.read(seq, |src| dst.copy_from_slice(&src[PAYLOAD_HDR..PAYLOAD_HDR + len]));
                    })?;
                    // 注意：send_output_raw 一旦呼叫就送出了。若複製途中被蓋掉（ok=false），
                    // 下游會收到一筆壞資料——所以 monitor 會驗內容；更嚴謹的做法見本篇「常見錯誤」。
                    if ok { sent += 1 } else { torn += 1 }
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[shm] 結束：送出 {sent} 格，讀太慢被覆蓋 {overrun} 格，讀到一半被改 {torn} 格");
    Ok(())
}
```

### src/bin/dma-node.rs

```rust
//! ZYNQ PL → DDR（AXI DMA S2MM，direct register mode）硬體節點的「形狀」。
//! ⚠️ 本篇在 x86 虛擬機上跑，暫存器／buffer／中斷全由 fake-hw 用檔案模擬（設 DMA_SIM=1）。
//! 換到真板子時要改的只有路徑（環境變數）和標了 SIM 的兩處。
//!
//!   I/O 執行緒：設定 DMA、等中斷（poll 有逾時）、檢查錯誤、ack、cache 同步，
//!               用 ping-pong 兩塊 buffer：一塊給 DMA 寫、一塊給 dora 讀。
//!   主執行緒：收到「第 h 塊好了」→ send_output_raw 從 DMA buffer 直接複製進 dora sample
//!             → 把第 h 塊還給 I/O 執行緒（credit）。主執行緒太慢時 I/O 執行緒拿不到空 buffer，
//!             DMA 就暫停（背壓往上游傳；真系統上 PL 端 FIFO 會滿、要在 FPGA 那邊計數 overflow）。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_16::axidma::*;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

const XFER: usize = 32 * 1024; // 每次 DMA 的 byte 數（= FRAME_HDR + 16372 個 i16 樣本）
const HALF: usize = 64 * 1024; // ping-pong 兩塊的間距

struct Ready { half: usize, frame: u32, irq_count: u32, len: usize }

#[derive(Default)]
struct Stats { frames: AtomicU64, timeouts: AtomicU64, errors: AtomicU64, stalls: AtomicU64, resets: AtomicU64 }

struct Hw {
    regs: Regs,
    buf: Arc<memmap2::MmapRaw>,
    phys: u64,
    uio: File,
    sysfs: PathBuf,
    sim: bool,
}

impl Hw {
    fn open() -> eyre::Result<(Hw, Arc<memmap2::MmapRaw>)> {
        let d = lesson_16::sim_dir();
        let sim = std::env::var("DMA_SIM").is_ok_and(|v| v == "1");
        // 真板子（UIO）：DMA_REGS=/dev/uio0（map0 = AXI DMA 暫存器，mmap offset 0）
        //   或 /dev/mem 加 offset=0x40400000（ZYNQ-7000 Vivado 預設位址）、O_SYNC 才是不快取的映射。
        // 真板子（u-dma-buf）：DMA_BUF=/dev/udmabuf0，SYSFS=/sys/class/u-dma-buf/udmabuf0
        let env = |k: &str, def: PathBuf| std::env::var_os(k).map(PathBuf::from).unwrap_or(def);
        let regs_path = env("DMA_REGS", d.join("axi_dma_regs"));
        let buf_path = env("DMA_BUF", d.join("udmabuf0"));
        let uio_path = env("DMA_UIO", d.join("uio0"));
        let sysfs = env("DMA_SYSFS", d.join("sys"));

        let f = OpenOptions::new().read(true).write(true).open(&regs_path)?;
        let regs = Regs::new(memmap2::MmapOptions::new().len(4096).map_raw(&f)?);
        let f = OpenOptions::new().read(true).write(true).open(&buf_path)?;
        let buf = Arc::new(memmap2::MmapRaw::map_raw(&f)?);
        let phys_s = fs::read_to_string(sysfs.join("phys_addr"))?;
        let phys = u64::from_str_radix(phys_s.trim().trim_start_matches("0x"), 16)?;
        let uio = OpenOptions::new().read(true).write(true).open(&uio_path)?;
        eprintln!("[dma] regs={} buf={} ({} bytes, phys={phys:#x}) irq={} sim={sim}",
                  regs_path.display(), buf_path.display(), buf.len(), uio_path.display());
        Ok((Hw { regs, buf: buf.clone(), phys, uio, sysfs, sim }, buf))
    }

    /// 軟體重設 DMA，等 Reset bit 自己清掉（有逾時）。
    fn reset(&self) -> eyre::Result<()> {
        self.regs.write(S2MM_DMACR, CR_RESET);
        for _ in 0..100 {
            if self.regs.read(S2MM_DMACR) & CR_RESET == 0 {
                self.regs.write(S2MM_DMACR, CR_RS | CR_IOC_IRQ_EN | CR_ERR_IRQ_EN);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        eyre::bail!("DMA reset 100 ms 內沒有完成（DMASR={:#x}）", self.regs.read(S2MM_DMASR))
    }

    /// 寫 1 清除（W1C）狀態位元。SIM：一般記憶體沒有 W1C 行為，只好直接把 bit 清成 0。
    fn ack(&self, bits: u32) {
        if self.sim {
            self.regs.write(S2MM_DMASR, self.regs.read(S2MM_DMASR) & !bits);
        } else {
            self.regs.write(S2MM_DMASR, bits);
        }
    }

    /// 開始一次 S2MM：先寫目的位址，最後寫 LENGTH（寫 LENGTH 就是「開始」）。
    fn start(&self, half: usize) {
        let da = self.phys + (half * HALF) as u64;
        self.regs.write(S2MM_DA, da as u32);
        self.regs.write(S2MM_DA_MSB, (da >> 32) as u32); // 只有 >32 位元位址寬度的設定才有這個暫存器
        self.regs.write(S2MM_LENGTH, XFER as u32);
    }

    /// UIO 的中斷：每次 read 前寫 1 重新致能（irqcontrol），read 回傳 u32 累計次數。用 poll 做逾時。
    fn wait_irq(&mut self, timeout_ms: i32) -> std::io::Result<Option<u32>> {
        if !self.sim {
            self.uio.write_all(&1u32.to_ne_bytes())?; // SIM：FIFO 寫進去會被自己讀回來，所以略過
        }
        let mut pfd = libc::pollfd { fd: self.uio.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        match unsafe { libc::poll(&mut pfd, 1, timeout_ms) } {
            0 => Ok(None),
            n if n < 0 => Err(std::io::Error::last_os_error()),
            _ => {
                let mut b = [0u8; 4];
                self.uio.read_exact(&mut b)?;
                Ok(Some(u32::from_ne_bytes(b)))
            }
        }
    }

    /// CPU 要讀 DMA 寫好的資料前：把這段的 cache 作廢（non-coherent 埠，例如 ZYNQ-7000 的 HP0～3）。
    /// u-dma-buf 的做法是寫 sysfs；在 coherent 埠（ACP、ZynqMP 的 HPC＋CCI）上可以省略。
    fn sync_for_cpu(&self, off: usize, len: usize) -> std::io::Result<()> {
        fs::write(self.sysfs.join("sync_offset"), off.to_string())?;
        fs::write(self.sysfs.join("sync_size"), len.to_string())?;
        fs::write(self.sysfs.join("sync_direction"), "2")?; // 2 = DMA_FROM_DEVICE
        fs::write(self.sysfs.join("sync_for_cpu"), "1")
    }
}

fn io_thread(mut hw: Hw, ready: Sender<Ready>, credit: Receiver<usize>, status: Sender<String>,
             st: Arc<Stats>, stop: Arc<AtomicBool>) {
    let mut free = [true, true];
    let mut cur: Option<usize> = None;
    if let Err(e) = hw.reset() {
        let _ = status.send(format!("初始化失敗：{e}"));
        return;
    }
    while !stop.load(Relaxed) {
        for h in credit.try_iter() { free[h] = true; }
        if cur.is_none() {
            match free.iter().position(|&f| f) {
                Some(h) => { free[h] = false; hw.start(h); cur = Some(h); }
                None => {
                    // 兩塊都在 dora 手上：等主執行緒還 buffer（背壓）
                    st.stalls.fetch_add(1, Relaxed);
                    if let Ok(h) = credit.recv_timeout(Duration::from_millis(50)) { free[h] = true; }
                    continue;
                }
            }
        }
        let irq = match hw.wait_irq(200) {
            Ok(Some(n)) => n,
            Ok(None) => {
                st.timeouts.fetch_add(1, Relaxed);
                let _ = status.send(format!("中斷逾時 200 ms（DMASR={:#x}），重設 DMA", hw.regs.read(S2MM_DMASR)));
                free[cur.take().unwrap()] = true; // 那次傳輸作廢，buffer 收回
                if let Err(e) = hw.reset() { let _ = status.send(format!("重設失敗：{e}")); return; }
                st.resets.fetch_add(1, Relaxed);
                continue;
            }
            Err(e) => { let _ = status.send(format!("讀中斷失敗：{e}")); return; }
        };
        let sr = hw.regs.read(S2MM_DMASR);
        if sr & SR_ERRORS != 0 {
            st.errors.fetch_add(1, Relaxed);
            let _ = status.send(format!("DMA 錯誤 DMASR={sr:#x}，重設"));
            free[cur.take().unwrap()] = true;
            let _ = hw.reset();
            st.resets.fetch_add(1, Relaxed);
            continue;
        }
        if sr & SR_IOC_IRQ == 0 { continue; } // 不是完成中斷（共用中斷線時會發生）
        hw.ack(SR_IOC_IRQ);
        let h = cur.take().unwrap();
        // ping-pong：馬上讓 DMA 寫另一塊，CPU 這邊同時處理剛寫好的這塊
        if let Some(n) = free.iter().position(|&f| f) { free[n] = false; hw.start(n); cur = Some(n); }
        if let Err(e) = hw.sync_for_cpu(h * HALF, XFER) { let _ = status.send(format!("cache 同步失敗：{e}")); }
        let p = unsafe { std::slice::from_raw_parts(hw.buf.as_ptr().add(h * HALF), FRAME_HDR) };
        let magic = u32::from_le_bytes(p[0..4].try_into().unwrap());
        let frame = u32::from_le_bytes(p[4..8].try_into().unwrap());
        let n = u32::from_le_bytes(p[8..12].try_into().unwrap()) as usize;
        if magic != FRAME_MAGIC || FRAME_HDR + 2 * n > XFER {
            st.errors.fetch_add(1, Relaxed);
            let _ = status.send(format!("buffer {h} 的 header 不對（magic={magic:#x}）——cache 沒作廢？"));
            free[h] = true;
            continue;
        }
        st.frames.fetch_add(1, Relaxed);
        if ready.send(Ready { half: h, frame, irq_count: irq, len: FRAME_HDR + 2 * n }).is_err() { return; }
    }
    hw.regs.write(S2MM_DMACR, 0); // 停 DMA：RS=0
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let (out, out_status) = (DataId::from("frame".to_owned()), DataId::from("status".to_owned()));
    let (hw, buf) = Hw::open()?;
    let (ready_tx, ready_rx) = mpsc::channel::<Ready>(); // 最多 2 筆在途（只有兩塊 buffer），不必有界
    let (credit_tx, credit_rx) = mpsc::channel::<usize>();
    let (status_tx, status_rx) = mpsc::channel::<String>();
    let st = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let io = {
        let (st, stop) = (st.clone(), stop.clone());
        std::thread::Builder::new().name("dma-io".into())
            .spawn(move || io_thread(hw, ready_tx, credit_rx, status_tx, st, stop))?
    };
    let mut sent = 0u64;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                for s in status_rx.try_iter() {
                    eprintln!("[dma] {s}");
                    node.send_output(out_status.clone(), MetadataParameters::new(), s.as_str())?;
                }
                for r in ready_rx.try_iter() {
                    let mut p = MetadataParameters::new();
                    p.insert("frame".into(), Parameter::Integer(r.frame as i64));
                    p.insert("irq".into(), Parameter::Integer(r.irq_count as i64));
                    let src = unsafe { std::slice::from_raw_parts(buf.as_ptr().add(r.half * HALF), r.len) };
                    node.send_output_raw(out.clone(), p, r.len, |dst| dst.copy_from_slice(src))?;
                    let _ = credit_tx.send(r.half); // 複製完就能還：下游拿到的是 dora 的 shm，不是 DMA buffer
                    sent += 1;
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    stop.store(true, Relaxed);
    let _ = io.join();
    eprintln!("[dma] 結束：完成 {} 塊、送出 {sent} 塊、中斷逾時 {}、DMA 錯誤 {}、重設 {}、等 buffer {} 次",
        st.frames.load(Relaxed), st.timeouts.load(Relaxed), st.errors.load(Relaxed),
        st.resets.load(Relaxed), st.stalls.load(Relaxed));
    Ok(())
}
```

### src/bin/monitor.rs

```rust
//! 下游：驗證三個硬體節點送來的資料（內容、序號缺口、延遲），結束時印總表。
use dora_node_api::{DoraNode, Event, get_integer_param};
use lesson_16::{axidma::{FRAME_HDR, FRAME_MAGIC}, now_ns};

#[derive(Default)]
struct Lat(Vec<u64>);
impl Lat {
    fn summary(&mut self) -> String {
        if self.0.is_empty() { return "—".into(); }
        self.0.sort_unstable();
        let q = |f: f64| self.0[((self.0.len() - 1) as f64 * f) as usize] as f64 / 1e6;
        format!("p50 {:.2} ms / p99 {:.2} ms / max {:.2} ms", q(0.5), q(0.99), q(1.0))
    }
}

/// 這個位址落在哪個 mapping？讀 /proc/self/maps——用來確認收到的資料是不是直接指向 /dev/shm（零拷貝）。
fn mapping_of(addr: usize) -> String {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    for l in maps.lines() {
        let mut f = l.split_whitespace();
        let (Some(range), Some(perm)) = (f.next(), f.next()) else { continue };
        let Some((a, b)) = range.split_once('-') else { continue };
        let (a, b) = (usize::from_str_radix(a, 16).unwrap_or(0), usize::from_str_radix(b, 16).unwrap_or(0));
        if (a..b).contains(&addr) {
            let name = f.nth(3).unwrap_or("[匿名：heap 或執行緒 arena]");
            return format!("{perm} {name}");
        }
    }
    "找不到".into()
}

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let (mut uart_n, mut uart_batches, mut uart_gaps, mut uart_last) = (0u64, 0u64, 0u64, 0i64);
    let (mut shm_n, mut shm_bad, mut shm_gaps, mut shm_last, mut shm_bytes) = (0u64, 0u64, 0u64, -1i64, 0usize);
    let (mut dma_n, mut dma_bad, mut dma_gaps, mut dma_last, mut dma_bytes) = (0u64, 0u64, 0u64, 0i64, 0usize);
    let (mut shm_lat, mut dma_lat) = (Lat::default(), Lat::default());
    let mut statuses = 0u64;

    while let Some(event) = events.recv() {
        let Event::Input { id, metadata, data } = event else {
            if matches!(event, Event::Stop(_)) { break; }
            continue;
        };
        let pm = &metadata.parameters;
        match id.as_str() {
            "uart_temp" => {
                let v: &[i32] = (&data).try_into()?;
                let first = get_integer_param(pm, "first_seq").unwrap_or(-1);
                let last = get_integer_param(pm, "last_seq").unwrap_or(-1);
                if uart_last > 0 && first > uart_last + 1 {
                    uart_gaps += (first - uart_last - 1) as u64; // 批次之間
                    println!("[monitor] uart 缺口：上一批到 seq {uart_last}，這一批從 {first} 開始");
                }
                uart_gaps += (last - first + 1) as u64 - v.len() as u64; // 批次之內（例如檢查碼錯被丟掉的行）
                if uart_batches % 20 == 0 {
                    println!("[monitor] uart 批次 seq {first}..={last}（{} 筆）第一筆 {:.3} °C", v.len(), v[0] as f64 / 1000.0);
                }
                uart_last = last;
                uart_batches += 1;
                uart_n += v.len() as u64;
            }
            "shm_frame" => {
                let b: &[u8] = (&data).try_into()?;
                let seq = get_integer_param(pm, "seq").unwrap_or(-1);
                let t = get_integer_param(pm, "t_ns").unwrap_or(0) as u64;
                shm_lat.0.push(now_ns().saturating_sub(t));
                if b.iter().any(|&x| x != seq as u8) { shm_bad += 1; } // 內容整塊應該都是 seq 的低 8 位
                if shm_last >= 0 && seq > shm_last + 1 { shm_gaps += (seq - shm_last - 1) as u64; }
                if shm_n % 200 == 0 {
                    println!("[monitor] shm seq={seq} {} bytes，資料位址 {:p} → {}", b.len(), b.as_ptr(), mapping_of(b.as_ptr() as usize));
                }
                shm_last = seq;
                shm_n += 1;
                shm_bytes += b.len();
            }
            "dma_frame" => {
                let b: &[u8] = (&data).try_into()?;
                let frame = get_integer_param(pm, "frame").unwrap_or(-1);
                let magic = u32::from_le_bytes(b[0..4].try_into()?);
                let n = u32::from_le_bytes(b[8..12].try_into()?) as usize;
                let t = u64::from_le_bytes(b[16..24].try_into()?);
                dma_lat.0.push(now_ns().saturating_sub(t));
                if magic != FRAME_MAGIC || b.len() != FRAME_HDR + 2 * n { dma_bad += 1; }
                if dma_last > 0 && frame > dma_last + 1 { dma_gaps += (frame - dma_last - 1) as u64; }
                if dma_n % 100 == 0 {
                    let s0 = i16::from_le_bytes(b[FRAME_HDR..FRAME_HDR + 2].try_into()?);
                    println!("[monitor] dma frame={frame} irq={} {n} 樣本，第一個樣本 {s0}，資料位址 {:p} → {}",
                             get_integer_param(pm, "irq").unwrap_or(-1), b.as_ptr(), mapping_of(b.as_ptr() as usize));
                }
                dma_last = frame;
                dma_n += 1;
                dma_bytes += b.len();
            }
            s if s.ends_with("_status") => {
                let msg: &str = (&data).try_into()?;
                println!("[monitor] 狀態 {s}: {msg}");
                statuses += 1;
            }
            _ => {}
        }
    }
    eprintln!("[monitor] UART：{uart_n} 筆／{uart_batches} 批，序號缺口 {uart_gaps}");
    eprintln!("[monitor] SHM ：{shm_n} 格（{:.1} MB），內容錯 {shm_bad}，序號缺口 {shm_gaps}，延遲 {}",
              shm_bytes as f64 / 1e6, shm_lat.summary());
    eprintln!("[monitor] DMA ：{dma_n} 塊（{:.1} MB），格式錯 {dma_bad}，frame 缺口 {dma_gaps}，延遲 {}",
              dma_bytes as f64 / 1e6, dma_lat.summary());
    eprintln!("[monitor] 狀態訊息 {statuses} 則");
    Ok(())
}
```

### src/bin/fake-hw.rs（模擬硬體）

```rust
//! ⚠️ 模擬硬體，不是 dora 節點、也不是真的裝置。同時模擬三樣東西：
//!   1. UART 感測器：openpty 開一對虛擬終端機，從 master 端每 20 ms 送一行溫度，
//!      slave 端用 symlink 掛成 $LESSON16_DIR/ttyFAKE0（等同 socat 的 pty,link=…）。
//!      會故意送壞檢查碼，並在 --unplug-at 秒時「拔線」1.5 秒（關掉 master、換一個新的 pts）。
//!   2. 另一個行程（例如相機驅動）寫入 /dev/shm/lesson16-ring 的環形緩衝，100 fps × 60 KB。
//!   3. ZYNQ PL 端的 AXI DMA S2MM＋ADC：暫存器是一個 4 KB 檔案（mmap）、DMA buffer 是 1 MB 檔案
//!      （假裝是 udmabuf）、PL 中斷是一個 FIFO（假裝是 /dev/uio0，read 讀到 u32 中斷次數）。
//!      每 --lose-every 次傳輸故意「掉一次中斷」，讓節點走逾時→重設的路徑。
//! 用法：fake-hw [--seconds N] [--unplug-at S] [--lose-every K]
use lesson_16::{axidma::*, now_ns, proto, ring::*, sim_dir};
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd, AsRawFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering, fence};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const PHYS_BASE: u64 = 0x3F00_0000; // 假的實體位址（udmabuf 的 phys_addr）
const DMA_BUF_SIZE: usize = 1 << 20;

fn arg(name: &str, default: f64) -> f64 {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() -> std::io::Result<()> {
    let dir = sim_dir();
    fs::create_dir_all(dir.join("sys"))?;
    let seconds = arg("--seconds", 30.0);
    let unplug_at = arg("--unplug-at", 4.0);
    let lose_every = arg("--lose-every", 40.0) as u32;
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();

    let hs = [
        { let (s, d) = (stop.clone(), dir.clone()); thread::spawn(move || uart(&d, s, t0, unplug_at)) },
        { let s = stop.clone(); thread::spawn(move || shm(s)) },
        { let (s, d) = (stop.clone(), dir.clone()); thread::spawn(move || pl(&d, s, lose_every)) },
    ];
    eprintln!("[fake-hw] 啟動：dir={} 執行 {seconds}s，UART 在 {unplug_at}s 拔線，DMA 每 {lose_every} 次掉一次中斷", dir.display());
    thread::sleep(Duration::from_secs_f64(seconds));
    stop.store(true, Ordering::Relaxed);
    for h in hs {
        let _ = h.join();
    }
    let _ = fs::remove_file("/dev/shm/lesson16-ring");
    eprintln!("[fake-hw] 結束");
    Ok(())
}

/// 開一對 pty，回傳 (master, slave 的路徑)。slave 先設成 raw，避免沒人開時 echo 回 master。
fn open_pty() -> std::io::Result<(File, String)> {
    let (mut m, mut s) = (0, 0);
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    unsafe { libc::cfmakeraw(&mut t) };
    if unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), &t, std::ptr::null()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut name = [0 as libc::c_char; 64];
    unsafe { libc::ttyname_r(s, name.as_mut_ptr(), name.len()) };
    let name = unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
    unsafe {
        libc::close(s);
        libc::fcntl(m, libc::F_SETFL, libc::O_NONBLOCK); // 沒人讀時寫滿就丟，不卡住模擬器
    }
    Ok((unsafe { File::from(OwnedFd::from_raw_fd(m)) }, name))
}

fn relink(target: &str, link: &Path) {
    let tmp = link.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp).unwrap();
    fs::rename(&tmp, link).unwrap(); // rename 是原子的：讀者不會看到「連結不存在」的空窗
}

fn uart(dir: &Path, stop: Arc<AtomicBool>, t0: Instant, unplug_at: f64) {
    let link = dir.join("ttyFAKE0");
    let (mut master, mut name) = open_pty().unwrap();
    relink(&name, &link);
    eprintln!("[fake-hw] UART：{} -> {name}", link.display());
    let (mut seq, mut overflow, mut unplugged) = (0u32, 0u64, false);
    while !stop.load(Ordering::Relaxed) {
        if !unplugged && t0.elapsed().as_secs_f64() >= unplug_at {
            unplugged = true;
            drop(master); // 拔線：關掉 master，slave 端的讀取會收到 EIO／hang-up
            let _ = fs::remove_file(&link);
            eprintln!("[fake-hw] UART：拔線（seq={seq}），1.5 s 後插回");
            thread::sleep(Duration::from_millis(1500));
            (master, name) = open_pty().unwrap();
            relink(&name, &link);
            eprintln!("[fake-hw] UART：插回 {} -> {name}", link.display());
        }
        seq += 1;
        let milli = 25_000 + ((seq as f64 / 25.0).sin() * 3000.0) as i32;
        let mut line = proto::encode(seq, milli).into_bytes();
        if seq % 37 == 0 {
            let n = line.len();
            line[n - 4] ^= 0x01; // 改一個 bit：模擬線路雜訊，檢查碼會對不上
        }
        if master.write_all(&line).is_err() {
            overflow += 1;
        }
        thread::sleep(Duration::from_millis(20));
    }
    eprintln!("[fake-hw] UART：共送 {seq} 行，寫滿丟掉 {overflow} 行");
}

fn shm(stop: Arc<AtomicBool>) {
    let ring = Ring::create(Path::new("/dev/shm/lesson16-ring"), 8, 64 * 1024).unwrap();
    let len = 60_000usize;
    let mut seq = 0u64;
    while !stop.load(Ordering::Relaxed) {
        ring.push(seq, |p| {
            p[0..8].copy_from_slice(&seq.to_le_bytes());
            p[8..16].copy_from_slice(&now_ns().to_le_bytes());
            p[16..20].copy_from_slice(&(len as u32).to_le_bytes());
            p[PAYLOAD_HDR..PAYLOAD_HDR + len].fill(seq as u8); // 內容可驗證：整塊都是 seq 的低 8 位
        });
        seq += 1;
        thread::sleep(Duration::from_millis(10));
    }
    eprintln!("[fake-hw] SHM：共寫 {seq} 格");
}

fn map_file(path: &Path, len: usize) -> memmap2::MmapRaw {
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path).unwrap();
    f.set_len(len as u64).unwrap();
    memmap2::MmapRaw::map_raw(&f).unwrap()
}

fn pl(dir: &Path, stop: Arc<AtomicBool>, lose_every: u32) {
    let regs = Regs::new(map_file(&dir.join("axi_dma_regs"), 4096));
    let buf = map_file(&dir.join("udmabuf0"), DMA_BUF_SIZE);
    // 模擬 u-dma-buf 的 sysfs：/sys/class/u-dma-buf/udmabuf0/{phys_addr,size}
    fs::write(dir.join("sys/phys_addr"), format!("{PHYS_BASE:#x}\n")).unwrap();
    fs::write(dir.join("sys/size"), format!("{DMA_BUF_SIZE}\n")).unwrap();
    let fifo = dir.join("uio0");
    let _ = fs::remove_file(&fifo);
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    // O_RDWR：Linux 允許，這樣沒有讀者時 open 不會卡住
    let mut irq = OpenOptions::new().read(true).write(true).open(&fifo).unwrap();
    unsafe { libc::fcntl(irq.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    regs.write(S2MM_DMASR, SR_HALTED);
    let (mut n_xfer, mut n_irq, mut lost, mut frame) = (0u32, 0u32, 0u32, 0u32);
    let mut phase = 0f64;
    while !stop.load(Ordering::Relaxed) {
        let cr = regs.read(S2MM_DMACR);
        if cr & CR_RESET != 0 {
            // 真硬體：寫 Reset 後 IP 自己清掉這個 bit、回到 Halted
            regs.write(S2MM_DMACR, 0);
            regs.write(S2MM_LENGTH, 0);
            regs.write(S2MM_DMASR, SR_HALTED);
            continue;
        }
        if cr & CR_RS == 0 {
            thread::sleep(Duration::from_micros(200));
            continue;
        }
        let sr = regs.read(S2MM_DMASR);
        if sr & SR_HALTED != 0 {
            regs.write(S2MM_DMASR, (sr & !SR_HALTED) | SR_IDLE);
        }
        let len = regs.read(S2MM_LENGTH) as usize;
        if len == 0 {
            thread::sleep(Duration::from_micros(200));
            continue;
        }
        // SIM：真的 AXI DMA 在「寫入 LENGTH」那一刻開始傳輸，完成後 LENGTH 讀回實際 byte 數。
        // 一般記憶體沒有「寫入副作用」，所以模擬器改成：看到 LENGTH≠0 就接下這次傳輸並把它歸零。
        regs.write(S2MM_LENGTH, 0);
        regs.write(S2MM_DMASR, regs.read(S2MM_DMASR) & !SR_IDLE);
        n_xfer += 1;
        let da = regs.read(S2MM_DA) as u64 | ((regs.read(S2MM_DA_MSB) as u64) << 32);
        let off = da.wrapping_sub(PHYS_BASE) as usize;
        if off + len > DMA_BUF_SIZE {
            // 位址超出 buffer：真硬體會是 DMADecErr（AXI DECERR），並發出錯誤中斷
            regs.write(S2MM_DMASR, regs.read(S2MM_DMASR) | SR_DMA_DEC_ERR | SR_ERR_IRQ | SR_HALTED);
            n_irq += 1;
            let _ = irq.write_all(&n_irq.to_ne_bytes());
            continue;
        }
        thread::sleep(Duration::from_millis(20)); // 模擬 ADC 取樣時間：一塊 ≈ 20 ms → 50 塊/秒
        if lose_every > 0 && n_xfer % lose_every == 0 {
            lost += 1; // 故意不完成、不發中斷：模擬 PL 端 tlast 沒送出、DMA 卡住
            continue;
        }
        frame += 1;
        let n = (len - FRAME_HDR) / 2;
        let p = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().add(off), len) };
        p[0..4].copy_from_slice(&FRAME_MAGIC.to_le_bytes());
        p[4..8].copy_from_slice(&frame.to_le_bytes());
        p[8..12].copy_from_slice(&(n as u32).to_le_bytes());
        p[16..24].copy_from_slice(&now_ns().to_le_bytes());
        for i in 0..n {
            let v = ((phase + i as f64 * 0.01).sin() * 12000.0) as i16;
            p[FRAME_HDR + 2 * i..FRAME_HDR + 2 * i + 2].copy_from_slice(&v.to_le_bytes());
        }
        phase += n as f64 * 0.01;
        fence(Ordering::Release); // 資料先落地、再亮完成旗標（真硬體由 AXI 交易順序保證）
        regs.write(S2MM_DMASR, regs.read(S2MM_DMASR) | SR_IOC_IRQ | SR_IDLE);
        if cr & CR_IOC_IRQ_EN != 0 {
            n_irq += 1;
            let _ = irq.write_all(&n_irq.to_ne_bytes()); // 等同 UIO：read() 拿到累計中斷次數
        }
    }
    eprintln!("[fake-hw] PL：收到 {n_xfer} 次傳輸請求，完成 {frame} 塊，故意掉中斷 {lost} 次，發出中斷 {n_irq} 次");
}
```

### dataflow.yml

```yaml
# 第 16 篇：三種硬體節點 → monitor。先在另一個終端機跑 fake-hw（模擬硬體），再 dora run。
nodes:
  - id: uart
    build: cargo build --release --bin uart-node
    path: target/release/uart-node
    env:
      SERIAL_PORT: /tmp/lesson16/ttyFAKE0   # 真裝置：/dev/ttyUSB0、/dev/ttyPS1（ZYNQ PS UART1）…
      BAUD: "115200"
    inputs:
      tick: dora/timer/millis/100     # 每 100 ms 把累積的讀值打包送一次
    outputs:
      - temp
      - status

  - id: shm
    build: cargo build --release --bin shm-node
    path: target/release/shm-node
    inputs:
      tick: dora/timer/millis/5
    outputs:
      - frame
      - status

  - id: dma
    build: cargo build --release --bin dma-node
    path: target/release/dma-node
    env:
      DMA_SIM: "1"                    # 真板子拿掉，並設 DMA_REGS=/dev/uio0 等（見 dma-node.rs）
    inputs:
      tick: dora/timer/millis/5
    outputs:
      - frame
      - status

  - id: monitor
    build: cargo build --release --bin monitor
    path: target/release/monitor
    inputs:
      uart_temp: uart/temp
      uart_status: uart/status
      shm_frame: shm/frame
      shm_status: shm/status
      dma_frame: dma/frame
      dma_status: dma/status
```

`dataflow-backpressure.yml` 只改這幾行：uart 改成 `tick: dora/timer/millis/1000`，並加上 `QUEUE: "16"`；shm、dma 的 tick 都改成 `millis/200`。

### run-demo.sh

```bash
#!/usr/bin/env bash
# 一鍵實測：背景啟動 fake-hw（模擬硬體）→ dora run 12 秒。用法：./run-demo.sh [dataflow.yml] → 收掉 fake-hw。
set -uo pipefail
YML=${1:-dataflow.yml}; TAG=$(basename "$YML" .yml)
cd "$(dirname "$0")"
DORA=${DORA:-dora}
mkdir -p run
rm -rf /tmp/lesson16
cargo build --release --bin fake-hw -q
./target/release/fake-hw --seconds 16 --unplug-at 4 --lose-every 40 > run/$TAG-fake-hw.log 2>&1 &
HW=$!
sleep 0.5
$DORA build "$YML" > run/$TAG-build.log 2>&1
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/$TAG.log
$DORA run "$YML" --stop-after 12s >> run/$TAG.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/$TAG.log
wait $HW
echo "fake-hw 結束碼 $?" >> run/$TAG-fake-hw.log
```

## 實際執行

```bash
cd examples/lesson-16   # 自 repo 根目錄
export PATH="$HOME/dora/bin:$PATH"     # 或像 run-demo.sh 一樣用 DORA=… 指定
cargo build --release                          # 8 核虛擬機從零開始約 1 分 49 秒，0 個 warning
./run-demo.sh                                  # 正常設定 → run/dataflow.log
./run-demo.sh dataflow-backpressure.yml        # 背壓實驗 → run/dataflow-backpressure.log
```

手動跑的話：先在一個終端機執行 `./target/release/fake-hw --seconds 60`，再到另一個終端機執行 `dora run dataflow.yml --stop-after 12s`。

### 實測輸出 1：正常設定（2026-11-16 07:48:46 開始，台北時間）

節點的 stdout 經過 grep 整理（完整內容在 `run/dataflow.log`）。monitor 每 20 批印一次 uart、每 200 格印一次 shm、每 100 塊印一次 dma：

```
07:48:47 stdout shm: [shm] zero_copy_threshold = 4096 bytes
07:48:47 stdout dma: [dma] regs=/tmp/lesson16/axi_dma_regs buf=/tmp/lesson16/udmabuf0 (1048576 bytes, phys=0x3f000000) irq=/tmp/lesson16/uio0 sim=true
07:48:47 stdout shm: [shm] 附掛 /dev/shm/lesson16-ring：8 格 × 65536 bytes，從 seq=265 開始
07:48:47 stdout monitor: [monitor] shm seq=265 60000 bytes，資料位址 0x7fb160013b40 → rw-p [匿名：heap 或執行緒 arena]
07:48:47 stdout monitor: [monitor] dma frame=1 irq=1 16372 樣本，第一個樣本 0，資料位址 0x7fb160012dc0 → rw-p [匿名：heap 或執行緒 arena]
07:48:47 stdout uart: [uart] 已連線 /tmp/lesson16/ttyFAKE0 @ 115200 baud
07:48:47 stdout monitor: [monitor] uart 批次 seq 135..=138（4 筆）第一筆 22.682 °C
07:48:47 stdout monitor: [monitor] uart 缺口：上一批到 seq 147，這一批從 149 開始      ← seq 148 是 37 的倍數，檢查碼被故意改壞
07:48:48 stdout dma: [dma] 中斷逾時 200 ms（DMASR=0x0），重設 DMA                     ← 整輪共 11 次（之後的省略）
07:48:48 stdout uart: [uart] 斷線：讀取錯誤：Broken pipe；重新連線                      ← fake-hw 在第 4 秒「拔線」
07:48:48 stdout uart: [uart] 開啟 /tmp/lesson16/ttyFAKE0 失敗：No such file or directory；100ms 後重試
07:48:48 stdout uart: [uart] 開啟 /tmp/lesson16/ttyFAKE0 失敗：No such file or directory；200ms 後重試
07:48:49 stdout uart: [uart] 開啟 /tmp/lesson16/ttyFAKE0 失敗：No such file or directory；400ms 後重試
07:48:49 stdout uart: [uart] 開啟 /tmp/lesson16/ttyFAKE0 失敗：No such file or directory；800ms 後重試
07:48:49 stdout monitor: [monitor] dma frame=101 irq=101 16372 樣本，第一個樣本 -11004，資料位址 0x7fb160012dc0 → rw-p [匿名：heap 或執行緒 arena]
07:48:50 stdout uart: [uart] 開啟 /tmp/lesson16/ttyFAKE0 失敗：No such file or directory；1.6s 後重試
07:48:51 stdout uart: [uart] 已連線 /tmp/lesson16/ttyFAKE0 @ 115200 baud
07:48:51 stdout monitor: [monitor] uart 缺口：上一批到 seq 199，這一批從 280 開始
…
07:48:57 stdout monitor: [monitor] dma frame=401 irq=401 16372 樣本，第一個樣本 -11970，資料位址 0x7fb160012dc0 → rw-p [匿名：heap 或執行緒 arena]
07:48:58 stdout shm: [shm] 結束：送出 1132 格，讀太慢被覆蓋 0 格，讀到一半被改 0 格
07:48:58 stdout monitor: [monitor] UART：401 筆／84 批，序號缺口 91
07:48:58 stdout monitor: [monitor] SHM ：1132 格（67.9 MB），內容錯 0，序號缺口 0，延遲 p50 3.24 ms / p99 6.09 ms / max 29.26 ms
07:48:58 stdout monitor: [monitor] DMA ：446 塊（14.6 MB），格式錯 0，frame 缺口 0，延遲 p50 3.46 ms / p99 7.00 ms / max 52.62 ms
07:48:58 stdout uart: [uart] 結束：有效 405 行、送出 401 筆、檢查碼錯 12、半行/格式錯 0、背壓丟棄 0、重連 1 次
07:48:58 stdout dma: [dma] 結束：完成 447 塊、送出 446 塊、中斷逾時 11、DMA 錯誤 0、重設 11、等 buffer 1 次
dora run 結束碼 0
```

（dora daemon 自己的 log 用 UTC，`2026-11-15T23:48:58Z` 就是台北時間 11-16 07:48:58。）

模擬器的 log（`run/dataflow-fake-hw.log`）：

```
[fake-hw] UART：拔線（seq=199），1.5 s 後插回
[fake-hw] UART：插回 /tmp/lesson16/ttyFAKE0 -> /dev/pts/0
[fake-hw] PL：收到 458 次傳輸請求，完成 447 塊，故意掉中斷 11 次，發出中斷 447 次
[fake-hw] SHM：共寫 1580 格
[fake-hw] UART：共送 722 行，寫滿丟掉 0 行
```

怎麼讀這些數字：

- **UART**：91 個缺口中，80 個是 seq 200～279：這是插回之後、節點還在退避（這一輪剛好退到 1.6 s）時寫進新 pty 的資料，開埠時被 `clear(Input)` 清掉了。另外 11 個是檢查碼錯被丟掉的行。檢查碼錯一共 12 行，差的那 1 行推測是最後一批送出後才讀到的，後面沒有新批次，monitor 就看不出缺口（沒有逐行追查）。有效 405 行、送出 401 筆，差的 4 筆是 dora 停止時還留在 channel 裡的。另外兩輪的退避只到 800 ms 就連上，總缺口都是 14，其中 13 個是檢查碼錯。斷線時 serialport 回報的是 **Broken pipe**：master 關掉後，`poll` 看到 POLLHUP，不是 `EIO`，也不是讀到 0 byte。如果只把 `Ok(0)` 當成斷線，這種情況會被漏掉。
- **SHM**：monitor 驗證了每一格 60000 byte 的內容，都等於 seq 的低 8 位，0 錯；序號連續。p50 約 3 ms 主要是 `shm-node` 每 5 ms 才輪詢一次造成的（平均要等半個 tick），不能當成 dora 的延遲。
- **DMA**：458 次請求 = 447 塊完成 + 11 次故意漏掉的中斷。每次漏掉都會走「逾時 → reset → 重新開始」，`frame` 編號沒有缺口：漏掉的那次沒有產生 frame，所以 frame 編號照樣連續。`DMASR=0x0` 表示 Halted 和 Idle 都沒有設，也就是「還在跑、正在傳」，和卡住的狀態一致。最後一塊在 dora 停止時還沒送出（完成 447、送出 446）。
- **資料位址**：shm 和 dma 的資料到了 monitor，都在 `rw-p` 的匿名 mapping 裡，不在 `/dev/shm/*.zenoh`（另一次執行時，從外部 `cat /proc/<monitor pid>/maps` 也看到 monitor 確實有映射 `/dev/shm/1564978775.zenoh` 等 zenoh 檔案，但資料指標不在那些範圍內）。這和「下游零拷貝」的說法不一致，見文末。

另外兩輪正常設定（07:46:04、07:47:45）的數字很接近：SHM 送出 1137 格和 1133 格，內容錯 0；DMA 送出 449 塊和 438 塊，逾時都是 11 次；UART 送出 478 筆和 475 筆，都重連 1 次。

07:47:45 那一輪結束後，模擬 sysfs 的內容（證明 `sync_for_cpu()` 的寫入路徑有走到，只是寫進一般檔案）：

```
sys/phys_addr: 0x3f000000
sys/size: 1048576
sys/sync_direction: 2
sys/sync_for_cpu: 1
sys/sync_offset: 65536
sys/sync_size: 32768
```

同一輪最後一次的 `S2MM_DA` 是 `0x3f010000`（`od -t x4` 看暫存器檔 0x48 的位置），也就是 ping-pong 的第 2 塊（phys + 64 KB）。

### 實測輸出 2：背壓實驗（2026-11-16 07:46:20 開始，台北時間）

```
07:46:22 stdout monitor: [monitor] uart 批次 seq 132..=147（16 筆）第一筆 22.471 °C     ← 一批最多 16 筆＝channel 容量
07:46:33 stdout monitor: [monitor] UART：165 筆／11 批，序號缺口 274
07:46:33 stdout shm: [shm] 結束：送出 456 格，讀太慢被覆蓋 673 格，讀到一半被改 0 格
07:46:33 stdout monitor: [monitor] SHM ：456 格（27.4 MB），內容錯 0，序號缺口 662，延遲 p50 41.21 ms / p99 79.68 ms / max 81.68 ms
07:46:33 stdout monitor: [monitor] DMA ：112 塊（3.7 MB），格式錯 0，frame 缺口 0，延遲 p50 160.36 ms / p99 180.93 ms / max 181.26 ms
07:46:33 stdout dma: [dma] 結束：完成 114 塊、送出 112 塊、中斷逾時 2、DMA 錯誤 0、重設 2、等 buffer 219 次
07:46:33 stdout uart: [uart] 結束：有效 483 行、送出 165 筆、檢查碼錯 13、半行/格式錯 0、背壓丟棄 302、重連 1 次
[fake-hw] PL：收到 116 次傳輸請求，完成 114 塊，故意掉中斷 2 次，發出中斷 114 次
```

三種策略的差別一眼就看得出來：

| 節點 | 策略 | 結果 |
|---|---|---|
| uart | channel 滿了丟**最新**的 | 每批 16 筆，`背壓丟棄 302`。留下來的是「每秒最早的 16 筆」，對溫度這種「要最新值」的訊號其實不理想（見小練習的加分題） |
| shm | 寫者覆蓋**最舊**的 | 讀者每 200 ms 醒來時只剩最後 8 格，`覆蓋 673`；延遲變成 41～80 ms，但內容仍然 0 錯（seqlock 擋住了，沒讀到寫一半的資料） |
| dma | buffer 沒還就**停住上游** | PL 只收到 116 次請求（正常約 458 次），`等 buffer 219 次`。frame 沒有缺口，延遲卻到 160 ms。在真的系統上，「停住上游」表示 PL 端的 FIFO 會滿，ADC 樣本會在 **FPGA 那一側**掉，要在 PL 裡加 overflow 計數器才看得到 |

（shm 的 `覆蓋 673` 和 monitor 的 `缺口 662` 差 11，沒有逐筆追查原因。）

## 常見錯誤

1. **沒關 `serialport` 的預設 feature**（實測，在 `/tmp` 另開一個只有 `serialport = "4.10"` 的專案）：
   ```
   error: failed to run custom build command for `libudev-sys v0.1.4`
     … "Could not run `… pkg-config --libs --cflags libudev`
   The pkg-config command could not be found.
   ```
   改成 `serialport = { version = "4.10", default-features = false }`。真的需要列舉 USB 埠時，再裝 `pkg-config` 和 `libudev-dev`；交叉編譯時還要準備板子的 sysroot。
2. **只把 `Ok(0)` 當成斷線**：本篇實測，pty 拔線時回報的是 `Broken pipe`；真的 USB 序列埠被拔掉，常見的是 `EIO` 或 `No such device`（本篇沒有實測）。除了 `TimedOut` 和 `Interrupted` 以外，**所有錯誤都當作斷線處理**。另外要加靜默看門狗：有些轉接器斷線時不回報錯誤，只是不再送資料。
3. **重連時沒有把舊的 port drop 掉**：`serialport` 開埠時會用 `TIOCEXCL` 加 exclusive `flock`。舊的 fd 還開著，新的 open 就會被擋下來（依原始碼推論，本篇的寫法在 `loop` 結束時會 drop，沒有測試反例）。
4. **`send_output_raw` 的 closure 沒辦法取消**：closure 一跑，資料就送出去了。`shm-node` 如果在複製途中被寫者蓋掉（`ok=false`），下游還是會收到一筆壞資料，所以 monitor 一定要驗證內容。本篇實測 `讀到一半被改 0`，這條路徑沒有真的發生過。嚴謹一點的做法：先從 ring 複製到自己的緩衝並驗證 seq，再送出（多一次複製）；或者 metadata 帶 seq，讓下游自己驗證。
5. **忘了處理 cache 就讀 DMA buffer**（真硬體，本篇無法實測）：ZYNQ-7000 的 HP 埠上，讀到的可能是上一輪的舊資料，而且**不一定每次都錯**，很難除錯。本篇的 header magic 加 frame 檢查是最低限度的防線。
6. **DMA buffer 用 `Vec` 或 `malloc`**：DMA 要的是實體位址，而且實體上要連續。user space 的虛擬記憶體兩個條件都不保證。請用 u-dma-buf、CMA 或核心驅動配置。
7. **用一般讀寫或 `memcpy` 存取暫存器**：MMIO 一定要用 `read_volatile`／`write_volatile`、對齊的 32 位元存取。在 ARM 上不對齊或寬度不對可能直接觸發 bus error；用 `memcpy` 去讀，編譯器可能把讀取合併或重排。
8. **以為 W1C 和一般記憶體一樣**：真的 DMASR 要**寫 1** 才會清掉 IOC_Irq。寫 `sr & !IOC` 不但清不掉，還可能把其他已經設起來的錯誤位元也寫 1 清掉。本篇模擬時被迫用後者，`Hw::ack()` 用 `sim` 旗標區分兩種寫法。
9. **`/dev/mem` 沒加 `O_SYNC`**：映射會經過 cache，暫存器的讀寫可能被延遲或合併（真硬體，本篇沒有實測）。另外 PetaLinux 的核心如果開了 `CONFIG_STRICT_DEVMEM`，有些範圍根本映射不了。

## 小練習

**題目**：讓 UART 節點變成**雙向**的。在 `uart-node` 加一個 `cmd` 輸入，收到 `"RATE,<Hz>"` 時，透過序列埠送出 `$RATE,<Hz>*<CS>\r\n`。`fake-hw` 收到後改變送資料的頻率，monitor 驗證每秒筆數真的變了。

提示：

- **寫入也放在 I/O 執行緒**，主執行緒只把命令丟進第二條 channel。不要在主執行緒直接 `port.write()`：port 屬於 I/O 執行緒，而且斷線時寫入同樣會阻塞或出錯。可以用 `port.try_clone()` 拿到一個寫入用的 handle，或在讀取迴圈每次逾時醒來時，順便把命令 channel 清空。
- 命令要不要在重連後**重送**？裝置斷電重開後會回到預設頻率，建議在 I/O 執行緒記住「最後一次設定」，每次連上就重送一次。
- `fake-hw` 的 master fd 是 non-blocking，讀命令時會遇到 `WouldBlock`。可以在每次送資料的 sleep 之前試讀一次，用 `proto::parse` 的同一套檢查碼邏輯處理。
- 加分：把 UART 的背壓改成「**只留最新的**」（例如 `Arc<Mutex<Option<Reading>>>`，或者 channel 滿了就先 `try_recv` 丟掉最舊的一筆再放進去），用 `dataflow-backpressure.yml` 比較每批的 seq。

## 本篇沒有驗證成功的部分

- **所有真的硬體都沒有測**：沒有實體 UART／USB 序列埠，沒有 ZYNQ 板子、bitstream、AXI DMA IP，也沒有 UIO、u-dma-buf 或 `/dev/mem`。PG021 的暫存器偏移、位元定義和流程是照文件寫的，**沒有在 IP 上驗證**。device tree 寫法、`uio_pdrv_genirq.of_id` 開機參數、u-dma-buf 的 sysfs 檔名，都只是說明。真硬體上的 W1C、`LENGTH` 讀回實際長度、cache 失效是否有效，都沒有跑過。
- **socat**：這台機器沒有安裝 socat，本篇沒有另外安裝，改用 Rust 的 `openpty` 寫模擬裝置。文中的 socat 指令沒有實測。
- **dora 下游的零拷貝**：monitor 用 `/proc/self/maps` 檢查收到的 shm、dma 資料指標，**兩者都在匿名記憶體，不在 `/dev/shm/*.zenoh`**，代表在這台機器、這個 dataflow 裡，下游拿到的是複本。試了兩種方式查原因：一是 maps 位址比對，二是用 `RUST_LOG=warn` 再跑一次，看 `send_output_sample` 有沒有 warn 級的診斷訊息（node log 裡沒看到 warn，也沒看到 SHM provider 失敗的警告）。都沒找到原因，依「同一步卡兩次就停」沒有繼續追（可能的方向：zenoh SHM 在接收端的協商、daemon 路徑；第 10 篇也沒有檢查過接收端的位址）。送出端「外部 ring → dora sample 只複製一次」這部分是確定的（程式碼就是這樣寫的）。
- **seqlock 的「讀到一半被改」路徑**從來沒有觸發過（兩個設定都是 0），所以「偵測到並丟棄」這條路沒有被實際執行過。真的 ARM 上（記憶體模型比 x86 弱）也還沒跑過。
- 延遲數字受 5 ms 輪詢 tick 主導，不能當作 dora、UART 或 DMA 的效能數據；qemu 和實機都沒有量。

收尾：`cargo clean`（`target/` 已刪除），刪除模擬目錄 `/tmp/lesson16`，以及本篇執行時留在 `/dev/shm` 的 zenoh 殘檔；確認沒有殘留的 dora 和 fake-hw 行程。log 保留在 `run/`。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 15 篇](lesson-15.md)
- 下一課：[第 17 篇 →](lesson-17.md)
- 本課範例：[examples/lesson-16/](../../examples/lesson-16/)
