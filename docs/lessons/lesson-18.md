# dora-rs 教學 第 18 篇：效能調校——先量再改：release profile、記憶體複製、佇列大小與 CPU 綁定

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0），rustc／cargo 1.99.1。驗證日期：2026-11-18（台北時間），只在 x86_64 Linux 虛擬機（8 核 Intel Xeon、15 GB RAM、沒有 GPU）上跑，這是共用的虛擬機，**雜訊很大**。三個節點「producer（200 張/秒，每張 256 KB 或 1 MB）→ worker（三點平滑濾波）→ sink（量延遲、吞吐、丟幀）」，每次 2,000 張、前 100 張暖機不計，`dora run` 結束碼全部是 0。實測摘要：（1）debug 建置比 release 慢約 30 倍（worker 處理時間 p50 11,325 µs vs 364 µs），在 200 張/秒下丟了 460 張；（2）release 與 `lto="fat"`＋`codegen-units=1`＋`panic="abort"` 在這個負載下**量不出差別**（各跑 4 次，數字互相重疊）；（3）不經過 dora 的微基準：每則訊息多配置一個 Vec 多花約 4～30 µs，再多複製一次再多花約 6～53 µs（256 KB／1 MB），但在 dora 裡這點差異被雜訊淹沒；（4）下游略慢於上游時，`queue_size` 1／10／100 的 sink 延遲 p50 分別是 4.0／49.0／339.0 ms；（5）用 `taskset` 綁核在這個負載下沒有可量出的壞處或好處。**沒驗證**：perf／flamegraph（這台沒有 perf 也不能安裝）、批次處理、執行緒池大小對 dora 節點的影響、Arrow 的 `send_output` 與 `send_output_raw` 在 dora 裡的穩定差距（雜訊太大，沒能量出來）。

> **關於日誌：** 本篇引用的 `run/*.log` 等完整執行日誌**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。範例可依「怎麼跑／執行」小節自行重現。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 17 篇](lesson-17.md)
- 下一課：[第 19 篇 →](lesson-19.md)
- 本課範例：[examples/lesson-18/](../../examples/lesson-18/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [debug](#debug)
- [release](#release)
- [fast](#fast)
- [常見錯誤](#常見錯誤)
- [小練習](#小練習)
- [本篇沒有驗證成功的部分](#本篇沒有驗證成功的部分)
## 重點說明

第 17 篇讓推論節點跑起來，第 19～20 篇的綜合專題（攝影機 → 辨識 → 顯示）會同時有影像、推論、顯示三個節點搶 CPU 與記憶體頻寬。本篇不教「某某設定讓你快 3 倍」——這台機器的實測就是不會這麼簡單——而是教**怎麼量、怎麼判斷一個改動有沒有用**，再把幾個值得先檢查的地方逐項量一遍。

### 1. 調校的順序：先量，再改，每次只改一件事

1. **先定目標**：是延遲（p50／p99）？吞吐？還是「不丟幀」？第 8 篇講過頻率算術，第 10 篇講過零拷貝，第 17 篇講過推論的丟幀策略；本篇的目標是 200 張/秒、每張 256 KB（52 MB/s）不丟幀、延遲在幾 ms 內。
2. **量測要有暖機**：本篇所有節點都跳過前 100 張（`WARMUP`），因為前幾張要建立共享記憶體區、頁面第一次被碰到，會特別慢。
3. **看分位數，不要只看平均**：p50 是「典型」，p99 是「壞的那 1%」，max 是最壞的一張。影像系統的使用者感受到的是卡頓，也就是 p99 與 max。
4. **重複量**：這台機器同一組設定，worker 處理時間 p50 可以在 163～366 µs 之間跳（見實測輸出 1）。**只跑一次就下結論，一定會被雜訊騙。**
5. **每次只改一件事**，改完用同樣的方法再量。

### 2. 量測方法：metadata 帶時間戳

producer 在送出前把 `SystemTime` 轉成奈秒，放進 metadata 的 `t_ns`（`Parameter::Integer`）；worker 把 `metadata.parameters.clone()` 原樣傳給輸出（所以 `seq`、`t_ns` 一路帶到 sink）；sink 收到時再取一次現在時間，兩者相減就是**端到端延遲**（producer 呼叫 `send_output_raw` 之前 → sink 收到之後）。三個節點在同一台機器上，用同一個系統時鐘，可以直接相減。跨機器就不行了（時鐘不同步），那時要用 dora metadata 內建的 HLC 時間戳（第 10 篇的做法）。

worker 另外量「自己處理一張的時間」（`Instant`，從開始 filter 到 `send_output*` 回來）。**分開量**很重要：端到端延遲＝兩段傳輸＋worker 的處理＋排隊時間，只看一個數字無法知道時間花在哪。

### 3. release profile：debug 與 release 差 30 倍；LTO 等選項要實測

`cargo build` 預設是 `dev` profile（`opt-level=0`，沒有最佳化）。**這是新手最常見的效能問題**：dataflow.yml 的 `path:` 或 `build:` 指到 debug 版，結果節點慢 30 倍。

| profile | 設定 |
|---|---|
| `dev`（`cargo build`） | `opt-level = 0`，有 debuginfo |
| `release`（`cargo build --release`） | `opt-level = 3`、`lto = false`、`codegen-units = 16`、`panic = "unwind"` |
| 本篇自訂 `fast`（`cargo build --profile fast`） | 繼承 release，再加 `lto = "fat"`、`codegen-units = 1`、`panic = "abort"` |

三項選項各自的意思：

- `lto`（link-time optimization）：連結時跨 crate 做最佳化（例如把 `dora-node-api`、`arrow` 的小函式內嵌進你的程式）。代價是**編譯時間大增**。
- `codegen-units = 1`：不把一個 crate 切成多份平行編譯，最佳化機會較多，編譯較慢。
- `panic = "abort"`：panic 時直接結束行程，不做堆疊展開，二進位較小。**注意**：節點 panic 時不會執行 `Drop`，dora 只會看到行程異常結束。

實測結果（見實測輸出 1）：debug 慢 30 倍是真的；但 release 與 fast **在這個負載下沒有可量出的差別**（因為時間大多花在記憶體搬移與行程間通訊，不是計算本身）。fast 的編譯代價卻是真的：第一次編譯 fast（含重新編譯所有相依套件）**4 分 20 秒**。結論：**先用 release，等量測顯示計算是瓶頸時，再試 LTO，並且重複量過才採用。**

### 4. 避免每則訊息配置記憶體與不必要的複製

第 10 篇量過送出端：`send_output_raw` 讓你直接寫進 dora 配好的輸出緩衝區（≥ 4096 位元組時是共享記憶體），`send_output(Vec)` 則是先在堆積上配置、再被轉成 Arrow、再複製進輸出緩衝區。本篇在 worker 加上同樣的對照（`MODE=raw` vs `MODE=alloc`），而且為了排除 dora 通訊的雜訊，另外寫了 `micro`，在單一行程裡只量「配置＋複製」本身：

- `reuse`：filter 寫進事先配好、重複使用的緩衝區（類似 `send_output_raw`）。
- `alloc`：每筆 `vec![0; n]`（配置並清零）再 filter。
- `alloc+copy`：`alloc` 之後再 `.to_vec()` 一次（類似 `send_output` 的「Vec → 再複製進輸出緩衝區」，但這裡只是類比，**不是 dora 內部的完整流程**）。

接收端同理：`TryFrom::try_from(&data)` 取得 `&[u8]` 是借用，不複製；一旦 `.to_vec()` 就多一次配置加搬移。

### 5. 佇列大小（`queue_size`）：用延遲換不丟幀

第 8 篇講過預設是 `drop_oldest`。第 17 篇看過丟幀策略。本篇用「下游每張約 5.3 ms（上限約 189 張/秒），上游 200 張/秒」的輕微過載量一次：`queue_size` 越大，丟得越少，但**每張在佇列裡等得越久**。影像／感測器流程如果「舊資料沒有價值」，佇列要設小（1～2）；如果每張都要處理（例如記錄資料），要設大，並接受延遲。

### 6. CPU 親和性

`taskset -c 0 dora run …` 會讓 `dora run` 及它啟動的節點都只能用第 0 核（子行程繼承親和性）。在 dataflow.yml 裡沒有每個節點獨立設定親和性的欄位（本篇沒有在 dora 1.1.0 的文件找到，也沒有去試，**請自行確認**）；要綁個別節點，需要在節點程式裡自己呼叫 `sched_setaffinity`，本篇**沒有做**。綁核的好處是減少排程器把行程在核心間搬來搬去（快取變冷）；壞處是節點之間搶同一顆核。實測見實測輸出 4。

### 7. 共享記憶體門檻

第 10 篇已經量過：4096 位元組以下走一般訊息，以上走共享記憶體（本篇 producer 啟動時印出 `zero_copy_threshold=4096`）。本篇的影格都是 256 KB／1 MB，所以全部走共享記憶體。小訊息（幾百位元組）反而**不要**為了零拷貝而湊成大塊。

### 8. 找熱點：perf／flamegraph——**本篇沒有成功**

要看「時間到底花在哪個函式」，標準工具是 `perf`（Linux）加 `cargo flamegraph`。這台機器沒有 `perf`，我試過一次 `apt-get install -y linux-perf`，失敗（`Could not open lock file /var/lib/dpkg/lock-frontend ... are you root?`，沒有 root 權限）。依「同一步卡兩次就停」，不再嘗試其他安裝方式。本篇用的是手動計時（`Instant`）與 `micro` 微基準，只能回答「哪一段花多少」，不能像 flamegraph 那樣自動列出函式。

## 完整範例

專案結構：

```
examples/lesson-18/
├── Cargo.toml
├── dataflow.tpl.yml      # 範本，gen.sh 產生各次實驗用的 yml
├── gen.sh
├── run-exp.sh
├── build-all.sh
└── src/
    ├── lib.rs
    └── bin/{producer,worker,sink,micro}.rs
```

### Cargo.toml

```toml
[package]
name = "lesson-18"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"

# 內建的 release 就是預設：opt-level=3、lto=false、codegen-units=16、panic=unwind
# 下面這個自訂 profile 繼承 release，再打開 fat LTO、單一 codegen unit、panic=abort
[profile.fast]
inherits = "release"
lto = "fat"
codegen-units = 1
panic = "abort"
```

### src/lib.rs

```rust
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 現在的 Unix 時間（奈秒）。三個節點在同一台機器上，可以直接拿來算端到端延遲。
pub fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

/// 三點平滑濾波：out[i] = (in[i-1] + 2*in[i] + in[i+1]) / 4，邊界用自己。
/// 這就是本篇的「模擬工作」：每個位元組做一點算術，和真實的影像前處理同一個量級。
pub fn filter(src: &[u8], dst: &mut [u8]) {
    let n = src.len();
    assert_eq!(n, dst.len());
    if n < 3 {
        dst.copy_from_slice(src);
        return;
    }
    dst[0] = src[0];
    dst[n - 1] = src[n - 1];
    for (o, w) in dst[1..n - 1].iter_mut().zip(src.windows(3)) {
        *o = ((w[0] as u16 + 2 * w[1] as u16 + w[2] as u16) >> 2) as u8;
    }
}

/// 忙等 us 微秒（模擬「這個節點比較重」），用 spin 而不是 sleep，才會真的佔 CPU
pub fn busy_us(us: u64) {
    if us == 0 {
        return;
    }
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_micros(us) {
        std::hint::spin_loop();
    }
}

pub fn pct(v: &[i64], p: f64) -> i64 {
    if v.is_empty() {
        return 0;
    }
    v[(((v.len() - 1) as f64) * p).round() as usize]
}
```

### src/bin/producer.rs

```rust
//! 假攝影機：每個 tick 送一張 SIZE 位元組的影格，metadata 帶 seq 與送出時間 t_ns。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_18::{env_or, now_ns};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let size: usize = env_or("SIZE", 262_144);
    let total: i64 = env_or("N", 2000);
    let out = DataId::from("frame".to_owned());
    println!("producer SIZE={size} N={total} zero_copy_threshold={}", node.zero_copy_threshold());
    let mut seq = 0i64;
    while let Some(ev) = events.recv() {
        if let Event::Input { .. } = ev {
            seq += 1;
            let mut p = MetadataParameters::default();
            p.insert("seq".into(), Parameter::Integer(seq));
            p.insert("t_ns".into(), Parameter::Integer(now_ns()));
            node.send_output_raw(out.clone(), p, size, |buf| {
                // 填入隨 seq 變動的內容（簡單的遞增樣式）
                let base = seq as u8;
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = base.wrapping_add(i as u8);
                }
            })?;
            if seq >= total {
                break;
            }
        }
    }
    println!("producer 送出 {seq} 張");
    Ok(())
}
```

### src/bin/worker.rs

```rust
//! 處理節點：對每張影格做 filter，再送出同樣大小的結果。
//! MODE=alloc：每則訊息 vec![0; n]（配置）→ filter → send_output（Arrow 轉換，再複製一次）
//! MODE=raw  ：send_output_raw，filter 直接寫進 dora 配好的輸出緩衝區（沒有額外配置、沒有額外複製）
//! SLOW_US   ：每張額外忙等多少微秒，模擬更重的運算
use dora_node_api::{DoraNode, Event, MetadataParameters, dora_core::config::DataId};
use lesson_18::{busy_us, env_or, filter, pct};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mode = std::env::var("MODE").unwrap_or_else(|_| "raw".into());
    let slow: u64 = env_or("SLOW_US", 0);
    let warmup: usize = env_or("WARMUP", 100);
    let out = DataId::from("result".to_owned());
    let mut cost: Vec<i64> = Vec::new(); // 每張的處理時間（奈秒）
    let mut n = 0usize;
    while let Some(ev) = events.recv() {
        match ev {
            Event::Input { metadata, data, .. } => {
                let src: &[u8] = TryFrom::try_from(&data)?;
                let params: MetadataParameters = metadata.parameters.clone(); // seq、t_ns 原樣傳下去
                let t0 = Instant::now();
                match mode.as_str() {
                    "raw" => node.send_output_raw(out.clone(), params, src.len(), |dst| {
                        filter(src, dst)
                    })?,
                    "alloc" => {
                        let mut v = vec![0u8; src.len()];
                        filter(src, &mut v);
                        node.send_output(out.clone(), params, v)?
                    }
                    other => eyre::bail!("未知的 MODE={other}"),
                }
                busy_us(slow);
                n += 1;
                if n > warmup {
                    cost.push(t0.elapsed().as_nanos() as i64);
                }
            }
            _ => {}
        }
    }
    cost.sort();
    println!(
        "worker MODE={mode} 處理 {n} 張（暖機 {warmup} 張不計）：處理時間 p50={:.1} µs  p99={:.1} µs  max={:.1} µs",
        pct(&cost, 0.5) as f64 / 1e3,
        pct(&cost, 0.99) as f64 / 1e3,
        pct(&cost, 1.0) as f64 / 1e3
    );
    Ok(())
}
```

### src/bin/sink.rs

```rust
//! 收尾節點：量端到端延遲（producer 蓋的 t_ns → 收到）、吞吐、丟幀。前 WARMUP 張不計。
use dora_node_api::{DoraNode, Event, Parameter};
use lesson_18::{env_or, now_ns, pct};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let label = std::env::var("LABEL").unwrap_or_else(|_| "run".into());
    let warmup: usize = env_or("WARMUP", 100);
    let (mut lat, mut got, mut max_seq) = (Vec::<i64>::new(), 0usize, 0i64);
    let (mut first, mut last): (Option<Instant>, Option<Instant>) = (None, None);
    let mut bytes = 0usize;
    while let Some(ev) = events.recv() {
        if let Event::Input { metadata, data, .. } = ev {
            let arrive = now_ns();
            let d: &[u8] = TryFrom::try_from(&data)?;
            got += 1;
            if let Some(Parameter::Integer(s)) = metadata.parameters.get("seq") {
                max_seq = max_seq.max(*s);
            }
            if got > warmup {
                if let Some(Parameter::Integer(t)) = metadata.parameters.get("t_ns") {
                    lat.push(arrive - t);
                }
                first.get_or_insert(Instant::now());
                last = Some(Instant::now());
                bytes += d.len();
            }
        }
    }
    lat.sort();
    let secs = match (first, last) {
        (Some(a), Some(b)) => (b - a).as_secs_f64(),
        _ => 0.0,
    };
    let counted = lat.len();
    println!(
        "sink [{label}] 收到 {got} 張 / producer 最大 seq {max_seq}（丟 {}）；計入 {counted} 張：延遲 p50={:.1} µs  p99={:.1} µs  max={:.1} µs；吞吐 {:.1} 張/秒、{:.1} MB/s",
        max_seq - got as i64,
        pct(&lat, 0.5) as f64 / 1e3,
        pct(&lat, 0.99) as f64 / 1e3,
        pct(&lat, 1.0) as f64 / 1e3,
        counted as f64 / secs,
        bytes as f64 / secs / 1e6
    );
    Ok(())
}
```

### src/bin/micro.rs

```rust
//! 不經過 dora 的微基準：只量「配置＋複製」vs「重用緩衝區」，排除行程間通訊的雜訊。
//! 三種寫法做的 filter 完全一樣，差別只在記憶體：
//!   reuse  ：filter 寫進事先配好、重複使用的緩衝區（等同 send_output_raw 寫進 dora 配好的緩衝區）
//!   alloc  ：每筆 vec![0; n] 再 filter（配置＋清零）
//!   alloc+copy：alloc 之後再 to_vec() 一次（等同 send_output 把 Vec 再編碼／複製進輸出緩衝區）
use lesson_18::{filter, pct};
use std::time::Instant;

fn main() {
    let size: usize = lesson_18::env_or("SIZE", 262_144);
    let iters: usize = lesson_18::env_or("ITERS", 2000);
    let rounds: usize = lesson_18::env_or("ROUNDS", 5);
    let src: Vec<u8> = (0..size).map(|i| (i * 7) as u8).collect();
    let mut reuse_buf = vec![0u8; size];
    let mut sink = 0u64;
    println!("micro SIZE={size} ITERS={iters}（每輪每種寫法各 {iters} 次，前 100 次暖機不計），單位 µs");
    for r in 1..=rounds {
        let mut res = [Vec::new(), Vec::new(), Vec::new()];
        for i in 0..iters {
            let t = Instant::now();
            filter(&src, &mut reuse_buf);
            sink += reuse_buf[size / 2] as u64;
            let a = t.elapsed().as_nanos() as i64;

            let t = Instant::now();
            let mut v = vec![0u8; size];
            filter(&src, &mut v);
            sink += v[size / 2] as u64;
            let b = t.elapsed().as_nanos() as i64;

            let t = Instant::now();
            let mut v = vec![0u8; size];
            filter(&src, &mut v);
            let c2 = v.to_vec();
            sink += c2[size / 2] as u64;
            let c = t.elapsed().as_nanos() as i64;
            if i >= 100 {
                res[0].push(a);
                res[1].push(b);
                res[2].push(c);
            }
        }
        for v in res.iter_mut() {
            v.sort();
        }
        println!(
            "round {r}: reuse p50={:.1} p99={:.1} | alloc p50={:.1} p99={:.1} | alloc+copy p50={:.1} p99={:.1}",
            pct(&res[0], 0.5) as f64 / 1e3, pct(&res[0], 0.99) as f64 / 1e3,
            pct(&res[1], 0.5) as f64 / 1e3, pct(&res[1], 0.99) as f64 / 1e3,
            pct(&res[2], 0.5) as f64 / 1e3, pct(&res[2], 0.99) as f64 / 1e3,
        );
    }
    println!("(sink={sink})");
}
```

### dataflow.tpl.yml

```yaml
# 第 18 篇：producer（200 張/秒）→ worker（filter）→ sink。
# 用 gen.sh 把兩個 @ 夾住的大寫佔位字串（PROFILE、MODE、SIZE…）換成實際值，產生各次實驗的 yml。
nodes:
  - id: producer
    path: ../target/@PROFILE@/producer
    env:
      SIZE: "@SIZE@"
      N: "@N@"
    inputs:
      tick: dora/timer/millis/@MS@
    outputs:
      - frame

  - id: worker
    path: ../target/@PROFILE@/worker
    env:
      MODE: @MODE@
      SLOW_US: "@SLOW@"
    inputs:
      frame:
        source: producer/frame
        queue_size: @Q@
    outputs:
      - result

  - id: sink
    path: ../target/@PROFILE@/sink
    env:
      LABEL: "@LABEL@"
    inputs:
      result:
        source: worker/result
        queue_size: @Q@
```

> dataflow 的 `path:` 是相對於 yml 所在目錄，所以產生出來的 yml 放在 `run/`，路徑寫 `../target/…`。這裡沒有用 `build:` 欄位，改成事先用 `build-all.sh` 把 debug／release／fast 三個版本都編好，才能在同一個 dataflow 範本裡換 profile 比較。

### gen.sh

```bash
#!/usr/bin/env bash
# 用法：./gen.sh 輸出.yml PROFILE MODE SIZE MS N SLOW Q LABEL
sed -e "s/@PROFILE@/$2/g" -e "s/@MODE@/$3/g" -e "s/@SIZE@/$4/g" -e "s/@MS@/$5/g" \
    -e "s/@N@/$6/g" -e "s/@SLOW@/$7/g" -e "s/@Q@/$8/g" -e "s/@LABEL@/$9/g" dataflow.tpl.yml > "$1"
```

### run-exp.sh

```bash
#!/usr/bin/env bash
# 用法：./run-exp.sh 名稱 PROFILE MODE SIZE MS N SLOW Q [前綴指令，如 taskset -c 0]
name=$1; shift
mkdir -p run
./gen.sh run/$name.yml $1 $2 $3 $4 $5 $6 $7 $name
shift 7
$@ dora run run/$name.yml > run/$name.log 2>&1
echo "== $name 結束碼 $?"; grep -oE "(worker MODE|sink \[).*" run/$name.log
```

### build-all.sh

```bash
#!/usr/bin/env bash
for p in "" "--release" "--profile fast"; do
  s=$(date +%s); cargo build $p 2>&1 | tail -2; echo "[cargo build $p] $(( $(date +%s)-s )) 秒"
done
```

## 實際執行

```bash
export PATH="$HOME/dora/bin:$PATH"   # 依你的 dora CLI 安裝位置調整
export RUSTUP_TOOLCHAIN=1.99.1-x86_64-unknown-linux-gnu   # 只有 rustup 沒設預設 toolchain 時才需要，見常見錯誤
cd examples/lesson-18   # 自 repo 根目錄
./build-all.sh                                   # dev、release、fast 各編一次
./run-exp.sh p-release release raw 262144 5 2000 0 10   # 名稱 PROFILE MODE SIZE 間隔ms N SLOW_US queue_size [前綴指令]
```

一次 `dora run` 的 log（`p-release`，節錄，時間為台北時間 2026-11-18）：

```
07:58:26 INFO   [dora]: daemon all nodes are ready, starting dataflow
07:58:26 stdout producer: producer SIZE=262144 N=2000 zero_copy_threshold=4096
07:58:36 stdout producer: producer 送出 2000 張
07:58:36 stdout worker: worker MODE=raw 處理 2000 張（暖機 100 張不計）：處理時間 p50=363.9 µs  p99=644.1 µs  max=992.9 µs
07:58:36 stdout sink: sink [p-release] 收到 2000 張 / producer 最大 seq 2000（丟 0）；計入 1900 張：延遲 p50=1704.0 µs  p99=2524.5 µs  max=6898.3 µs；吞吐 200.1 張/秒、52.5 MB/s
07:58:36 INFO   [dora]: daemon dataflow finished on machine `01a1ebf2-9639-7462-8d15-c8c6443826f4`
```

### 實測輸出 1：debug vs release vs fast（256 KB × 200 張/秒，`MODE=raw`，queue_size 10）

debug 只跑 1 次，因為已經慢到丟幀、差距遠大於雜訊：

```
worker MODE=raw 處理 1540 張（暖機 100 張不計）：處理時間 p50=11324.8 µs  p99=15235.0 µs  max=40196.1 µs
sink [p-debug] 收到 1540 張 / producer 最大 seq 2000（丟 460）；計入 1440 張：延遲 p50=112357.3 µs  p99=133647.6 µs  max=146059.6 µs；吞吐 86.2 張/秒、22.6 MB/s
```

release 與 fast 各跑 4 次（第 1 次是上面那組，其後 3 輪交錯執行 release、fast；全部 2,000 張都收到，丟 0）：

| 次 | release：worker p50 | release：端到端 p50 / p99 | fast：worker p50 | fast：端到端 p50 / p99 |
|---|---|---|---|---|
| 1 | 363.9 µs | 1,704.0 / 2,524.5 µs | 346.1 µs | 1,352.7 / 3,367.3 µs |
| 2 | 339.7 µs | 1,678.7 / 2,725.8 µs | 340.5 µs | 1,682.4 / 2,440.4 µs |
| 3 | 188.3 µs | 1,207.5 / 4,717.0 µs | 365.8 µs | 1,709.7 / 2,589.1 µs |
| 4 | 312.2 µs | 1,305.7 / 9,102.6 µs | 163.0 µs | 1,070.7 / 3,757.2 µs |

**解讀**：debug 比 release 慢約 31 倍（11,325 vs 364 µs，第 1 次的對照）。release 與 fast 的範圍互相重疊（worker p50 release 188～364、fast 163～366），同一組設定自己跑就會差一倍，所以**不能說 fast 比較快或比較慢**。吞吐方面，release 與 fast 都是約 199～200 張/秒（52.2～52.5 MB/s），等於 producer 的送出速率，系統沒有飽和。

不經過 dora 的微基準（`micro`，256 KB，同一個 filter，3 輪，p50，單位 µs；「reuse」是只有 filter 的時間）：

```
## debug
round 1: reuse p50=3594.3 p99=6133.6 | alloc p50=3607.7 p99=6182.0 | alloc+copy p50=3611.9 p99=6140.2
round 2: reuse p50=3664.9 p99=7491.7 | alloc p50=3688.3 p99=7461.1 | alloc+copy p50=3699.2 p99=7442.7
round 3: reuse p50=3823.9 p99=5667.1 | alloc p50=3839.7 p99=5886.0 | alloc+copy p50=3839.0 p99=5665.1
## release
round 1: reuse p50=27.0 p99=53.1 | alloc p50=31.6 p99=68.3 | alloc+copy p50=38.1 p99=70.2
round 2: reuse p50=26.3 p99=46.4 | alloc p50=30.8 p99=48.4 | alloc+copy p50=37.1 p99=73.6
round 3: reuse p50=26.4 p99=34.3 | alloc p50=30.9 p99=39.4 | alloc+copy p50=37.2 p99=47.1
## fast
round 1: reuse p50=28.7 p99=48.4 | alloc p50=31.9 p99=55.0 | alloc+copy p50=38.3 p99=56.3
round 2: reuse p50=28.8 p99=65.7 | alloc p50=32.0 p99=64.7 | alloc+copy p50=38.4 p99=85.3
round 3: reuse p50=28.7 p99=43.0 | alloc p50=31.9 p99=44.4 | alloc+copy p50=38.4 p99=53.0
```

debug 的 filter 要 3.6 ms 以上（是 release 的約 135 倍），所以在 debug 裡「配置與複製」完全看不出來——**最佳化關掉的時候，先改記憶體用法是白費力氣**。release 與 fast 在這裡也一樣：filter 本身 fast 的 p50（28.7 µs）還略高於 release（26.3～27.0 µs），沒有證據顯示 LTO 對這段迴圈有幫助。

另外有個重要發現：256 KB 的 filter 在 release 只要約 26 µs，但同一個 filter 在 dora worker 裡「從開始到 `send_output_raw` 回來」的 p50 是 163～366 µs。**多出來的 140～340 µs 不是計算**，而是 dora 配置輸出緩衝區（共享記憶體）、頁面第一次被碰到、送出事件等的成本。想再快，改演算法前要先知道時間不在演算法。

### 實測輸出 2：記憶體——`raw` vs `alloc`（1 MB × 200 張/秒，release，各 3 輪交錯）

| 輪 | raw：worker p50 | raw：端到端 p50 / p99 | alloc：worker p50 | alloc：端到端 p50 / p99 |
|---|---|---|---|---|
| 1 | 1,195.4 µs | 3,506.3 / 9,680.0 µs | 582.3 µs | 2,888.1 / 8,767.5 µs |
| 2 | 508.9 µs | 2,955.4 / 6,597.2 µs | 1,410.6 µs | 3,660.6 / 11,144.4 µs |
| 3 | 1,273.4 µs | 3,575.1 / 8,511.2 µs | 1,337.6 µs | 3,733.5 / 10,754.6 µs |

六組的丟幀都是 0，吞吐 197.6～200.0 張/秒（約 207～210 MB/s）。**結論：量不出來。** raw 的 worker p50 在 509～1,273 µs 之間，alloc 在 582～1,411 µs 之間，範圍幾乎完全重疊，而且兩種模式都出現了「約 500～600 µs」與「約 1,200～1,400 µs」兩個群，推測是這台虛擬機的雜訊（共用主機、頻率），**這是推測，沒有驗證**。不經過 dora 的 `micro`（1 MB）比較乾淨，5 輪 p50 都很穩定：

```
round 1: reuse p50=100.8 p99=180.4 | alloc p50=132.0 p99=217.1 | alloc+copy p50=184.8 p99=309.7
round 2: reuse p50=101.2 p99=204.3 | alloc p50=132.6 p99=240.7 | alloc+copy p50=185.5 p99=323.7
round 3: reuse p50=101.6 p99=192.4 | alloc p50=131.8 p99=219.5 | alloc+copy p50=183.7 p99=301.1
round 4: reuse p50=100.1 p99=241.2 | alloc p50=130.1 p99=258.5 | alloc+copy p50=180.6 p99=314.5
round 5: reuse p50=102.6 p99=153.2 | alloc p50=132.1 p99=170.9 | alloc+copy p50=185.2 p99=274.1
```

1 MB 時，重用緩衝區約 101 µs，每則配置新 Vec 約 131 µs（多 30%），再多複製一次約 184 µs（多 83%）。256 KB 時（`micro` 5 輪）是約 24～26 µs／29～30 µs／35～37 µs。**這個差距是真的，而且和大小成正比；但相對於 dora 的固定成本與雜訊，在這台機器上單看一次 dora 執行看不出來。**能省就省，但不要期待它是主要瓶頸。

### 實測輸出 3：`queue_size` 與丟幀（256 KB × 200 張/秒，每張再多忙等 5 ms，worker 每張約 5.3 ms，上限約 189 張/秒）

```
== q1   worker 處理 1895 張：p50=5234.8 µs
sink [q1]   收到 1895 張 / producer 最大 seq 2000（丟 105）；延遲 p50=3961.8 µs   p99=7076.5 µs   max=19761.4 µs；吞吐 189.6 張/秒
== q10  worker 處理 1895 張：p50=5253.6 µs
sink [q10]  收到 1895 張 / producer 最大 seq 2000（丟 105）；延遲 p50=48972.0 µs  p99=52744.8 µs  max=85387.0 µs；吞吐 188.3 張/秒
== q100 worker 處理 1981 張：p50=5259.2 µs
sink [q100] 收到 1981 張 / producer 最大 seq 2000（丟 19） ；延遲 p50=338992.5 µs p99=522025.1 µs max=528070.9 µs；吞吐 188.4 張/秒
```

| queue_size | 丟幀 | 端到端延遲 p50 | p99 |
|---|---|---|---|
| 1 | 105 | 4.0 ms | 7.1 ms |
| 10 | 105 | 49.0 ms | 52.7 ms |
| 100 | 19 | 339.0 ms | 522.0 ms |

三次的吞吐都是約 188～190 張/秒（受限於 worker）。`worker 處理張數 = sink 收到張數`（1895＝1895、1981＝1981），所以丟幀發生在 worker 的**輸入佇列**（dora 的 `drop_oldest`），不是 worker 之後。**q100 只丟 19 張，是因為 2,000 張送完時佇列還沒滿，行程就結束了**（延遲 339 ms ≈ 佇列裡排了約 64 張 × 5.3 ms）；如果跑更久，q100 最後也一樣會丟，而且延遲更大。q1 與 q10 都丟 105 張，是因為上下游速度差決定了「每秒必須丟多少」，`queue_size` 只決定「留在佇列裡的是多舊的資料」。log 裡沒有找到 dora 印出的丟幀訊息，所以以上「丟在 worker 輸入」是用張數推出來的，不是 dora 直接告訴我的。

### 實測輸出 4：CPU 親和性（256 KB × 200 張/秒，release，`SLOW_US=0`，各 1 次）

```
== aff-all taskset -c 0-7  worker p50=354.2 µs   端到端 p50=1633.5 µs  p99=2420.3 µs  丟 0  200 張/秒 ≈ 198.5
== aff-c0  taskset -c 0    worker p50=273.1 µs   端到端 p50=1434.0 µs  p99=3106.8 µs  丟 0
== aff-c01 taskset -c 0,1  worker p50=244.9 µs   端到端 p50=1409.6 µs  p99=3377.1 µs  丟 0
== aff-c0-1MB taskset -c 0 (1 MB)  worker p50=460.0 µs  端到端 p50=2826.0 µs  p99=3861.6 µs  丟 0
```

三個節點全部擠在**同一顆核心**也沒有變慢，因為這個負載下每個節點大部分時間都在等事件（200 張/秒，每張只花幾百微秒，CPU 使用率很低）。每組只跑一次，數字落在前面量到的雜訊範圍內，所以**不能說綁核有好處**。綁核有意義的情境是：CPU 吃滿、節點被搬來搬去，或要把即時節點與其他工作隔離；本篇沒有測這種情境。

## 常見錯誤

1. **拿 debug 版量效能（或部署）**：`cargo build` 沒加 `--release`，dataflow.yml 的 `path:` 指向 `target/debug/…`。本篇實測慢約 31 倍並丟了 460 張。檢查 `dora run` log 裡 `spawning` 後面的路徑是 `target/release/…`。
2. **只跑一次就下結論**：本篇 release 的 worker p50 在 4 次間是 188～364 µs。至少跑 3 次、看範圍，範圍重疊就是「沒差」。
3. **沒有暖機、只看平均**：前幾張特別慢，平均會被拉高；分位數才反映卡頓。
4. **把 LTO／`codegen-units=1`／`panic="abort"` 當成萬靈丹**：本篇這個負載下量不出差別，卻付出 4 分 20 秒的編譯時間。`panic="abort"` 還會讓 panic 時不展開堆疊。
5. **把 `queue_size` 開很大「以免丟幀」**：上游持續比下游快時，只是把丟幀換成延遲（q100 的 p50 延遲 339 ms）。要嘛加速下游，要嘛接受丟幀、把佇列設小。
6. **兩個節點的時間相減，但時鐘不同步**：本篇用 `SystemTime` 只因為三個節點在同一台機器。跨機器要另外處理時鐘。
7. **這台機器的 `cargo` 在 shell 裡直接失敗**：
   ```
   error: rustup could not choose a version of cargo to run, because one wasn't specified explicitly, and no default is configured.
   ```
   `rustup toolchain list` 有 `1.99.1-x86_64-unknown-linux-gnu` 但沒有設預設。我用環境變數 `export RUSTUP_TOOLCHAIN=1.99.1-x86_64-unknown-linux-gnu` 解決（沒有動 rustup 設定）。
8. **一個前景的 shell 指令跑太久被中止**：第一次把三個 profile 的編譯放在同一個前景指令，指令被工具回報 `Command failed to spawn: Aborted`（編譯其實已在背景完成了 dev 和 release）。之後改用 `nohup ./build-all.sh &` 在背景編譯。因此 dev 與 release 的編譯時間**沒有量到**（build-all.sh 的 log 裡 dev 1 秒、release 0 秒是第二次，已編好）；只有 fast 的 4 分 20 秒是真的（而且那段時間機器上另有別人的 `cargo test` 在跑，所以時間會偏長）。
9. **範本裡的註解被 `sed` 一起換掉**：`dataflow.tpl.yml` 的註解裡一開始寫了佔位字串，結果被 `gen.sh` 換成實際值，產生出來的 yml 註解變成亂碼（不影響執行）。已把範本註解改成不含佔位字串的寫法，所以 `run/` 裡舊的 yml 註解仍是舊版。

## 小練習

**題目**：在 worker 裡加入**批次**：每次 `events.recv()` 收到一張後，用 `events.try_recv()`（dora-node-api 1.1.0 的 `EventStream` 有這個方法，回傳 `Result<Event, TryRecvError>`；本篇沒有實際用過）把佇列裡已到的影像也取出來，最多湊 `BATCH` 張，一次處理完再送出。然後回答：

1. 在「下游略慢於上游」（本篇實測輸出 3 的設定，`SLOW_US=5000`，`queue_size=10`）時，`BATCH=1`、`4`、`8` 三種設定的丟幀數與端到端延遲 p50／p99 各是多少？
2. 每批多處理 1 張，延遲會不會變差？（提示：第一張要等批次湊滿才輸出。）
3. 把忙等改成「每批固定忙等 5 ms，而不是每張 5 ms」，模擬「批次攤平固定成本」，丟幀會怎麼變？

要求：每個設定至少跑 3 次、看範圍；數字一定要自己跑出來，不要用猜的。加分：用 `taskset` 把 `dora run` 綁在一顆核心，再比一次。

## 本篇沒有驗證成功的部分

- **perf／cargo flamegraph**：這台機器沒有 `perf`，`apt-get install -y linux-perf` 失敗（沒有 root 權限，`Could not open lock file /var/lib/dpkg/lock-frontend`）。依「同一步卡兩次就停」沒有再嘗試別的安裝方式，所以沒有 flamegraph，也沒有函式層級的熱點資料。
- **批次處理**：只在小練習提出，沒有實作也沒有量測。
- **`send_output_raw` 與 `send_output` 在 dora 裡的穩定差距**：dora 內的量測被雜訊淹沒（輸出 2：兩種模式範圍重疊）。只有不經過 dora 的 `micro` 量得出差距，而且 `micro` 的 `alloc+copy` 只是「配置再複製」的類比，不是 `send_output` 內部真正的 Arrow 轉換流程；沒有直接量 Arrow 轉換的成本。
- **LTO／`codegen-units`／`panic` 各自的貢獻**：只比了「全部打開」的 `fast` 與預設 release，沒有逐項拆開，也沒有對真正計算密集的節點（例如第 17 篇的推論）量過。
- **dora 對每個節點分別設定 CPU 親和性**：沒有查到 dataflow.yml 有這種欄位（沒有翻完整份文件，也沒試），沒有在節點程式內呼叫 `sched_setaffinity`；`taskset` 只綁整個 `dora run`，而且只在低負載下測，沒有在 CPU 吃滿時測。
- **雜訊的來源**：worker 處理時間在同一設定下出現約 2 倍的兩個群，推測與共用虛擬機有關，沒有驗證，也沒有再追（依「同一步卡兩次就停」）。
- **log 裡 dora 沒有丟幀訊息**：輸出 3 的「丟在 worker 輸入」是由張數推出來的。
- **共享記憶體門檻與 Python 節點、多機、GPU**：沒有在本篇測；門檻見第 10 篇。
- **只量了一組硬體**（8 核虛擬機，x86_64）；ZYNQ／aarch64 上的數字一定不同，沒有量。
- **下一篇預告**：第 19～20 篇綜合專題「攝影機 → 辨識 → 顯示」會把第 16 篇的資料來源、第 17 篇的推論、本篇的量測方法與佇列設定接起來；本篇的 `t_ns` 時間戳與 p50／p99 量測可以直接沿用。

收尾：對 `lesson-18` 執行 `cargo clean`，刪除 `target/`（`cargo clean` 回報移除 12,488 個檔案、共 8.6 GiB；`du` 在清除前量到 6.8 GB，兩個數字不一致，未追查；內容是 dev、release、fast 三份加上相依套件）；刪除本篇執行時留在 `/dev/shm` 的 zenoh 殘檔 `2497450950.zenoh`（12 KB，只刪這一個，時間戳在本篇最後一次 `dora run` 期間）；確認沒有殘留的 dora 行程（`pgrep` 找不到 `dora` 或 `lesson-18` 的行程）。保留：原始碼、`dataflow.tpl.yml`、各腳本，以及 `run/` 裡全部實驗的 yml 與 log（約 656 KB）。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 17 篇](lesson-17.md)
- 下一課：[第 19 篇 →](lesson-19.md)
- 本課範例：[examples/lesson-18/](../../examples/lesson-18/)
