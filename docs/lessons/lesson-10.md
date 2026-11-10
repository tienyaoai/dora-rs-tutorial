# dora-rs 教學 第 10 篇：零拷貝大資料與 benchmark——影格變大時，資料到底被搬了幾次

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-10（台北時間），只在這台 x86_64 Linux 虛擬機（8 核 Intel Xeon）上實測。3 份 YAML 的 `dora validate` 結束碼都是 0，`dora build` 成功（release，第一次約 1 分 48 秒）；`raw.yml`、`vec.yml`、`noshm.yml` 各跑 3 輪共 9 次，`dora run` 結束碼全是 0，每輪每種大小 100 筆、抽查內容 0 筆錯誤。數字會隨機器與負載變動，請當作「怎麼量」的示範，不要當成 dora 的官方效能數據。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 9 篇](lesson-09.md)
- 下一課：第 11 篇**尚未發布**（僅規劃；見 [課程大綱](../../README.md#課程大綱20-篇規劃)）
- 本課範例：[examples/lesson-10/](../../examples/lesson-10/)

## 目錄

- [重點說明](#重點說明)
- [完整範例：producer（三種模式）→ sink](#完整範例producer三種模式-sink)
- [執行步驟](#執行步驟)
- [本篇重點整理](#本篇重點整理)
- [小練習](#小練習)
- [驗證紀錄](#驗證紀錄)
## 重點說明

前幾篇送的都是一個整數或一小段陣列。真實的機器人／影像系統裡，一張 1080p RGB 影格就有約 6 MB，每秒 30 張。這時候問題不再是「送不送得到」，而是**資料被複製了幾次**、每次複製花多少時間。本篇做三件事：

1. 看懂 dora v1.1.0 什麼時候走共享記憶體（shared memory，多個行程共用同一塊實體記憶體，不必複製）。
2. 比較兩種送法：`send_output_raw`（直接寫進 dora 給的緩衝區）與 `send_output`（先在自己的 `Vec` 準備好再交出去）。
3. 比較兩種讀法：借用 `&[u8]`（零拷貝）與 `.to_vec()`（複製一份），並自己寫一個小 benchmark 把數字量出來。

### 1. 門檻：4096 位元組

`dora-node-api` 1.1.0 原始碼（`src/node/mod.rs`）裡有一個常數：

```rust
pub const ZERO_COPY_THRESHOLD: usize = 4096;
```

- 資料 **≥ 4096 位元組**：向 zenoh（dora 的資料傳輸層）的共享記憶體池要一塊緩衝區，送出時只是把這塊記憶體的所有權交出去，本機的接收端直接映射同一塊記憶體來讀。
- 資料 **< 4096 位元組**：用一般的堆積記憶體（heap）緩衝區發送。原始碼註解說明理由：共享記憶體至少要佔一整頁（4 KB），而且建立共享區段有固定成本，小訊息直接複製反而便宜。
- 門檻可用環境變數 `DORA_ZERO_COPY_THRESHOLD` 覆寫，寫在**送出端節點**的 `env` 裡就會生效（範例的 `noshm.yml` 把它調到 1 GB，等於永遠不用共享記憶體）。可用 `node.zero_copy_threshold()` 印出目前值。
- 原始碼註解也提到：共享記憶體池暫時滿了時，不會卡住等待，而是退回堆積緩衝區，多付一次複製，換取送出端不被拖慢。

### 2. 送出端：搬幾次？

| 寫法 | 資料流向 | 複製次數（送出端） |
|---|---|---|
| `send_output_raw(id, params, len, \|out\| ...)` | dora 先配好 `len` 大小的緩衝區（≥ 門檻時就是共享記憶體），你在閉包裡**直接寫進去** | 0 次額外複製（你寫入本身就是產生資料） |
| `send_output(id, params, vec)` | 你先在自己的 `Vec<u8>` 做好資料，dora 再把它編碼（Arrow IPC 格式）進共享記憶體 | 多 1 次 |

`send_output_raw` 的原始碼註解寫得很直接：先把 Arrow IPC 的檔頭寫進樣本緩衝區，再讓呼叫端把位元組寫進資料區，「zero payload copies」。所以如果你的資料來源本來就能寫進任意 `&mut [u8]`（例如相機驅動、DMA 搬進來的資料、解碼器輸出），用 `send_output_raw` 讓它直接寫進 dora 的緩衝區最省。`send_output` 的註解則寫「might copy the message once to move it to shared memory」，好處是型別方便（`Vec<f32>`、Arrow 陣列都能直接丟）。

### 3. 接收端：借用 vs 複製

收到的 `data` 是 `ArrowData`，背後可能就是那塊共享記憶體。

```rust
let bytes: &[u8] = TryFrom::try_from(&data)?;   // 借用：不複製，O(1)
let owned: Vec<u8> = bytes.to_vec();             // 複製：O(n)，4 MB 就要搬 4 MB
```

借用的限制是生命週期：`bytes` 不能活得比 `data` 久。需要把資料留到下一個事件（例如做多張影格的平均）時才複製，而且只複製需要的部分。

### 4. 怎麼量才算數

- **一定用 release 建置**（`cargo build --release`），debug 建置的數字沒有參考價值。
- **端到端延遲**：送出端在呼叫 send 時，dora 會用節點的 HLC（混合邏輯時鐘）在 metadata 蓋時間戳；接收端用 `metadata.timestamp().get_time().to_system_time().elapsed()` 算出「從蓋章到收到」的時間。原始碼中這個時間戳是在資料寫好、真正送出時才產生，所以它**不含**送出端準備資料的時間。同一台機器上兩個節點共用系統時鐘，這個值才有意義；跨機器還要考慮時鐘差。
- **送出端成本**：在送出端自己用 `Instant` 量「產生資料＋交給 dora」的時間。
- **讀取成本**：在接收端量「轉成 `&[u8]`＋抽查內容（或複製）」的時間。
- 每種大小送 100 筆、間隔 5 ms，量的是延遲而不是把佇列塞爆（塞爆的情形是第 8 篇的主題）。看中位數（p50）與 p99，不要只看平均。
- 跑**多輪**，看數字是否穩定。

## 完整範例：producer（三種模式）→ sink

位置：[examples/lesson-10/](../../examples/lesson-10/)

```
examples/lesson-10/
├── Cargo.toml
├── raw.yml      # send_output_raw ＋ 借用讀取
├── vec.yml      # send_output(Vec) ＋ 複製讀取
├── noshm.yml    # 同 raw.yml，但把門檻調到 1 GB，強制不用共享記憶體
└── src/bin/
    ├── producer.rs
    └── sink.rs
```

### Cargo.toml

```toml
[package]
name = "lesson-10"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"

[profile.release]
debug = false
```

### src/bin/producer.rs

```rust
//! 大資料來源：依序送出不同大小的「影格」，量測送出那一刻要花多少時間。
//! MODE=raw ：send_output_raw，直接把資料寫進 dora 配好的（共享記憶體）緩衝區
//! MODE=vec ：先在自己的 Vec<u8> 準備好資料，再交給 send_output（會多複製一次）
use dora_node_api::{
    DoraNode, MetadataParameters, Parameter, dora_core::config::DataId,
};
use std::time::{Duration, Instant};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 每筆資料的內容：第 i 個位元組 = (seq + i) % 251，接收端可以抽查驗證。
/// 先做一份比最大影格多 251 位元組的樣板，之後每筆只要從 seq % 251 開始切一段複製，
/// 這樣「產生資料」就只是一次 memcpy，量到的時間主要是搬資料本身。
fn template(max: usize) -> Vec<u8> {
    (0..max + 251).map(|i| (i % 251) as u8).collect()
}

fn main() -> eyre::Result<()> {
    let (mut node, _events) = DoraNode::init_from_env()?;
    let mode = std::env::var("MODE").unwrap_or_else(|_| "raw".into());
    let n: u64 = env_or("N", 100);
    let sizes: Vec<usize> = std::env::var("SIZES")
        .unwrap_or_else(|_| "1024,4096,65536,1048576,4194304".into())
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<Result<_, _>>()?;
    let frame = DataId::from("frame".to_owned());
    let tpl = template(*sizes.iter().max().unwrap_or(&0));

    println!(
        "producer MODE={mode} N={n} zero_copy_threshold={} 位元組",
        node.zero_copy_threshold()
    );

    let mut seq = 0u64;
    for &size in &sizes {
        let mut send_times = Vec::with_capacity(n as usize);
        for _ in 0..n {
            seq += 1;
            let mut params = MetadataParameters::default();
            params.insert("seq".into(), Parameter::Integer(seq as i64));
            let src = &tpl[(seq % 251) as usize..][..size];
            // 計時範圍＝「產生這一筆資料」＋「交給 dora」，兩種模式做的事一樣，只差搬幾次
            let t0 = Instant::now();
            match mode.as_str() {
                // 直接寫進 dora 配好的緩衝區（≥ 門檻時就是共享記憶體）：只搬 1 次
                "raw" => node.send_output_raw(frame.clone(), params, size, |out| {
                    out.copy_from_slice(src)
                })?,
                "vec" => {
                    // 先放進自己的 Vec（配置＋搬 1 次），send_output 再編碼進共享記憶體（再搬 1 次）
                    let owned: Vec<u8> = src.to_vec();
                    node.send_output(frame.clone(), params, owned)?
                }
                other => eyre::bail!("未知的 MODE={other}，請用 raw 或 vec"),
            }
            send_times.push(t0.elapsed());
            std::thread::sleep(Duration::from_millis(5)); // 間隔 5ms，量延遲而不是塞爆佇列
        }
        send_times.sort();
        let avg = send_times.iter().sum::<Duration>() / n as u32;
        println!(
            "producer size={:>8}  send() avg={:>10.1?}  p50={:>10.1?}  max={:>10.1?}",
            size,
            avg,
            send_times[send_times.len() / 2],
            send_times[send_times.len() - 1]
        );
        std::thread::sleep(Duration::from_millis(300)); // 讓下游清空再換大小
    }
    println!("producer 全部 {seq} 筆送完，結束");
    Ok(())
}
```

### src/bin/sink.rs

```rust
//! 接收端：量測端到端延遲（送出時間戳 → 收到），並比較「借用」與「複製」讀取的成本。
//! READ=borrow：TryFrom 取得 &[u8]，直接讀共享記憶體，不複製
//! READ=copy  ：再呼叫 .to_vec() 複製一份到自己的堆積記憶體
use dora_node_api::{DoraNode, Event, Parameter};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Stat {
    latency: Vec<Duration>,
    read: Vec<Duration>,
    bad: u32,
    first: Option<Instant>,
    last: Option<Instant>,
}

fn pct(v: &[Duration], p: f64) -> Duration {
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let read_mode = std::env::var("READ").unwrap_or_else(|_| "borrow".into());
    let label = std::env::var("LABEL").unwrap_or_else(|_| "run".into());
    let mut stats: BTreeMap<usize, Stat> = BTreeMap::new();
    let mut keep: u64 = 0; // 讓編譯器不能把讀取最佳化掉

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "frame" => {
                // 1) 到達延遲：送出端在 metadata 裡蓋的 HLC 時間戳 → 現在
                let latency = metadata
                    .timestamp()
                    .get_time()
                    .to_system_time()
                    .elapsed()
                    .unwrap_or_default();
                let seq = match metadata.parameters.get("seq") {
                    Some(Parameter::Integer(s)) => *s as u64,
                    _ => 0,
                };
                // 2) 讀取：借用 &[u8]（零拷貝）或再複製一份
                let t0 = Instant::now();
                let bytes: &[u8] = TryFrom::try_from(&data)?;
                let size = bytes.len();
                let ok = if read_mode == "copy" {
                    let owned: Vec<u8> = bytes.to_vec();
                    keep = keep.wrapping_add(owned[size / 2] as u64);
                    check(&owned, seq)
                } else {
                    keep = keep.wrapping_add(bytes[size / 2] as u64);
                    check(bytes, seq)
                };
                let read = t0.elapsed();

                let s = stats.entry(size).or_default();
                let now = Instant::now();
                s.first.get_or_insert(now);
                s.last = Some(now);
                s.latency.push(latency);
                s.read.push(read);
                if !ok {
                    s.bad += 1;
                }
            }
            Event::InputClosed { id } => println!("sink 輸入 {id} 已關閉"),
            Event::Stop(_) => {}
            _ => {}
        }
    }

    println!("sink [{label}] READ={read_mode}（keep={keep}）");
    println!(
        "{:>8} {:>5} {:>11} {:>11} {:>11} {:>11} {:>5}",
        "size", "n", "lat p50", "lat p99", "read p50", "read MB/s", "bad"
    );
    let mut csv = String::from("label,size,n,lat_p50_us,lat_p99_us,read_p50_us,read_mb_s,bad\n");
    for (size, s) in stats.iter_mut() {
        s.latency.sort();
        s.read.sort();
        let read_p50 = pct(&s.read, 0.5);
        let mbps = if read_p50.is_zero() {
            f64::INFINITY
        } else {
            *size as f64 / read_p50.as_secs_f64() / 1e6
        };
        println!(
            "{:>8} {:>5} {:>11.1?} {:>11.1?} {:>11.1?} {:>11.0} {:>5}",
            size,
            s.latency.len(),
            pct(&s.latency, 0.5),
            pct(&s.latency, 0.99),
            read_p50,
            mbps,
            s.bad
        );
        csv += &format!(
            "{label},{size},{},{},{},{},{mbps:.0},{}\n",
            s.latency.len(),
            pct(&s.latency, 0.5).as_micros(),
            pct(&s.latency, 0.99).as_micros(),
            read_p50.as_micros(),
            s.bad
        );
    }
    if let Ok(path) = std::env::var("CSV") {
        std::fs::write(&path, csv)?;
        println!("sink 結果已寫入 {path}");
    }
    Ok(())
}

/// 抽查開頭、中間、結尾三個位元組是否符合 (seq + i) % 251
fn check(buf: &[u8], seq: u64) -> bool {
    if buf.is_empty() {
        return true;
    }
    [0, buf.len() / 2, buf.len() - 1]
        .iter()
        .all(|&i| buf[i] == ((seq as usize + i) % 251) as u8)
}
```

### raw.yml

```yaml
nodes:
  - id: producer
    build: cargo build --release --bin producer
    path: target/release/producer
    env:
      MODE: raw
      N: "100"
    outputs:
      - frame

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    env:
      READ: borrow
      LABEL: raw
      CSV: result-raw.csv
    inputs:
      frame:
        source: producer/frame
        queue_size: 100
```

`vec.yml` 只差兩處：producer 的 `MODE: vec`、sink 的 `READ: copy`（`LABEL`、`CSV` 改成 vec）。`noshm.yml` 跟 `raw.yml` 一樣，只在 producer 的 `env` 多一行：

```yaml
      DORA_ZERO_COPY_THRESHOLD: "1000000000"   # 門檻調到 1GB = 永遠不用共享記憶體
```

## 執行步驟

```bash
cd examples/lesson-10   # 在 clone 本 repo 後，於 repo 根目錄執行
dora validate raw.yml && dora validate vec.yml && dora validate noshm.yml
dora build raw.yml        # 三份 YAML 共用同一個 crate，建一次就好（release）
dora run raw.yml
dora run vec.yml
dora run noshm.yml
cat result-*.csv          # sink 會把每種大小的結果寫成 CSV
```

> `CSV: result-raw.csv` 是相對路徑；dora 會在 YAML 所在目錄啟動節點（日誌裡的 `spawning ... in .../examples/lesson-10`），所以 CSV 會出現在專案目錄。

### 實際輸出（`raw.yml` 第 1 輪，節錄）

```
producer: producer MODE=raw N=100 zero_copy_threshold=4096 位元組
producer: producer size=    1024  send() avg=    62.1µs  p50=    61.4µs  max=   188.2µs
producer: producer size=    4096  send() avg=    44.6µs  p50=    44.7µs  max=    79.6µs
producer: producer size=   65536  send() avg=    96.6µs  p50=    88.7µs  max=   657.9µs
producer: producer size= 1048576  send() avg=   898.1µs  p50=   869.0µs  max=     1.8ms
producer: producer size= 4194304  send() avg=     4.8ms  p50=     4.9ms  max=     7.2ms
producer: producer 全部 500 筆送完，結束
sink: sink [raw] READ=borrow（keep=57055）
sink:     size     n     lat p50     lat p99    read p50   read MB/s   bad
sink:     1024   100     368.7µs     882.3µs     604.0ns        1695     0
sink:     4096   100     450.6µs     787.3µs     375.0ns       10923     0
sink:    65536   100     546.6µs     937.7µs     573.0ns      114373     0
sink:  1048576   100       2.1ms       3.0ms     437.0ns     2399487     0
sink:  4194304   100       7.4ms      10.2ms     470.0ns     8924051     0
```

`vec.yml` 第 1 輪的 sink（READ=copy）：

```
sink:     size     n     lat p50     lat p99    read p50   read MB/s   bad
sink:     1024   100     570.5µs     983.9µs       1.1µs         958     0
sink:     4096   100     478.5µs     977.1µs       1.5µs        2783     0
sink:    65536   100     408.6µs     922.5µs       5.4µs       12063     0
sink:  1048576   100       2.3ms       5.9ms      61.2µs       17138     0
sink:  4194304   100       7.1ms      11.5ms     277.7µs       15106     0
```

`bad` 全是 0：抽查的開頭、中間、結尾位元組都符合 `(seq + i) % 251`，代表 500 筆資料內容都沒有錯位。

### 三輪彙整（p50，單位 µs，三個數字依序是第 1／2／3 輪）

**接收端讀取時間**（這是本篇最清楚的結論）

| 大小 | 借用 `&[u8]`（raw、noshm） | 複製 `.to_vec()`（vec） |
|---|---|---|
| 1 KB | < 1 | 0～1 |
| 64 KB | < 1 | 4～5 |
| 1 MB | < 1（約 0.4～0.5） | 61～69 |
| 4 MB | < 1（約 0.5） | 255～277 |

借用的時間不隨大小增加（只是拿到指標，再讀 3 個位元組）；複製則跟大小成正比，4 MB 約 0.27 ms，大約每秒 15 GB 的搬運速度。以 30 fps 的 4 MB 影格來說，每張多複製一次就吃掉每格 33 ms 預算裡的約 0.8%；管線有 5 個節點都各複製一次，就是 4% 左右，而且還會佔記憶體頻寬。

> 表中 `read MB/s` 欄在借用模式下是「資料大小 ÷ 讀取時間」的名目值，因為根本沒讀整塊資料，數字大得不合理，只能拿來對照，不是真實的記憶體頻寬。

**送出端 `send()`（含產生資料）**

| 大小 | raw | vec | noshm |
|---|---|---|---|
| 64 KB | 89／91／96 | 117／117／115 | 74／76／74 |
| 1 MB | 869／881／957 | 941／986／924 | 1300／1300／1300 |
| 4 MB | 4900／3600／5500 | 4900／4900／4000 | 4600／4600／4800 |

**端到端延遲（蓋時間戳 → sink 收到）**

| 大小 | raw | vec | noshm |
|---|---|---|---|
| 1 KB | 368／487／511 | 570／620／502 | 315／604／594 |
| 64 KB | 546／522／554 | 408／527／583 | 467／563／537 |
| 1 MB | 2120／2254／2189 | 2287／2301／2160 | 1263／1274／1260 |
| 4 MB | 7417／6971／8526 | 7086／7569／7078 | 3738／3280／4496 |

### 怎麼解讀（含意料之外的結果）

1. **接收端零拷貝確實有效**：借用讀取在 1 KB 到 4 MB 都低於 1 µs，複製則線性成長。這是寫大資料節點時最該先做的事：**能借用就不要 `.to_vec()`**。
2. **送出端 raw 與 vec 在這台機器上差距不大**：64 KB 時 raw 約快 20%，1 MB 時約快 3～8%，4 MB 時三輪結果互有高低、看不出穩定差異。原因是兩種模式都要把資料搬進共享記憶體至少一次，vec 多出的那次複製（4 MB 約 0.3 ms）被其他成本蓋過了。
3. **意料之外：關掉共享記憶體（noshm）在 1 MB、4 MB 的端到端延遲反而約快一半**（4 MB 約 3.3～4.5 ms，對比 raw 約 7～8.5 ms），三輪都一樣。這和「共享記憶體一定比較快」的直覺相反。我**沒有找到確定的原因**，只有待驗證的推測：一是 raw 的 4 MB `send()` 要 3.6～5.5 ms，遠比單純 memcpy 4 MB（接收端實測約 0.27 ms）慢，可能是每筆都從共享記憶體池拿新的區塊，首次寫入時觸發大量分頁錯誤（page fault），在虛擬機上特別貴；二是共享記憶體路徑在本機的通知與映射流程有額外步驟。要確認得用 `perf` 之類的工具看分頁錯誤與系統呼叫，本篇沒做。
4. 結論不是「不要用共享記憶體」，而是：**零拷貝的好處主要在「不用一再複製」，傳輸本身快不快要在你的目標硬體上量**。之後上 ZYNQ（第 15 篇）時，請用同一個範例在板子上重量。
5. 小訊息（1 KB、4 KB）三種模式都在 0.3～0.6 ms，差異落在雜訊範圍內，不值得為它們優化資料搬運。

### 官方 benchmark 對照

dora 上游 monorepo 內附 `examples/benchmark`（tag `v1.1.0`），它也用 `send_output_raw`，量 0 B～約 4 MB 共 10 種大小的延遲與吞吐量。當時量到 400 KB 中位數約 1.1 ms、約 3.9 MB 中位數約 22.9 ms；那次機器較忙，與本篇的數字不能直接比較，但同樣顯示大資料的延遲隨大小上升，而不是如官方 README 範例所說「4 KB 以上保持平坦」。這再次說明官方範例的數字只是示意，要以自己機器實測為準。

## 本篇重點整理

- dora v1.1.0 預設 ≥ 4096 位元組走共享記憶體，可用 `DORA_ZERO_COPY_THRESHOLD` 調整。
- 送出大資料優先用 `send_output_raw`，讓資料直接寫進 dora 的緩衝區；`send_output` 比較方便但可能多複製一次。
- 接收端用 `TryFrom::try_from(&data)` 借用 `&[u8]`（或 `&[f32]` 等），是真正的零拷貝；`.to_vec()` 的成本跟大小成正比。
- benchmark 要用 release、量 p50／p99、跑多輪，並在目標硬體上重量；本篇在虛擬機上就量到「關掉共享記憶體反而延遲較低」的反直覺結果。

## 小練習

1. **讓借用也「讀完整塊」**：把 `sink.rs` 的 `check` 改成把整塊資料加總（例如 `buf.iter().map(|&b| b as u64).sum::<u64>()`），再跑 `raw.yml` 與 `vec.yml`。借用模式的讀取時間會變成多少？和複製模式相比，差距還剩多少？（提示：這時量到的才是真正讀共享記憶體的頻寬。）
2. **找門檻**：在 `noshm.yml` 的 producer `env` 把 `DORA_ZERO_COPY_THRESHOLD` 改成 `"65536"`，並把 `SIZES` 設成 `"32768,65536,131072,1048576"`，觀察 64 KB 上下的延遲是否出現轉折。
3. 進階：在 sink 收到資料時，把 `&[u8]` 存進一個 `Vec<&[u8]>` 留到下一個事件再用，看編譯器給你什麼錯誤，並用你自己的話解釋為什麼借用不能活得比 `data` 久。

下一篇：第 11 篇**尚未發布**（僅規劃；見 [課程大綱](../../README.md#課程大綱20-篇規劃)）。

## 驗證紀錄

- 環境：x86_64 Linux 虛擬機（8 核），Rust stable，`dora-cli 1.1.0`、`dora-node-api 1.1.0`。
- `dora validate raw.yml / vec.yml / noshm.yml`：結束碼皆 0。
- `dora build raw.yml`：成功（release）。
- `dora run`：三份 YAML 各 3 輪，9 次結束碼全為 0，每次 5 種大小各 100 筆，`bad` 全為 0。
- 未測：Windows、macOS、aarch64／ZYNQ；跨機器（分散式）傳輸；吞吐量（每秒可送幾筆）的極限值；`perf` 分析 noshm 較快的原因。
- 編譯產物 `target/` 已在驗證後刪除；三輪 benchmark 的 CSV 與節錄日誌保留在 `examples/lesson-10/rounds/`。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 9 篇](lesson-09.md)
- 下一課：第 11 篇**尚未發布**（僅規劃；見 [課程大綱](../../README.md#課程大綱20-篇規劃)）
- 本課範例：[examples/lesson-10/](../../examples/lesson-10/)
