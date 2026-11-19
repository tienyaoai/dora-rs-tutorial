# dora-rs 教學 第 13 篇：測試與除錯——單元測試、不開 daemon 的整合測試、錄製重播

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0）　驗證日期：2026-11-13（台北時間），只在這台 x86_64 Linux 虛擬機上實測。`cargo test --release`：單元測試 4 個、整合測試 3 個全部通過；`check.sh` 黑箱比對 PASS（並刻意改壞預期值示範 FAIL，結束碼 1）；`dora validate dataflow.yml` 通過；`dora run dataflow.yml --stop-after 2s` 結束碼 0；用 `DORA_WRITE_EVENTS_TO` 錄下的輸入重播，結果與實跑一致（收到 14 筆、警告 9 筆、壞輸入 1 筆）；刻意設錯參數的 `dataflow-bad.yml` 結束碼 1。第一次 release 建置約 2 分 8 秒。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 12 篇](lesson-12.md)
- 下一課：[第 14 篇 →](lesson-14.md)
- 本課範例：[examples/lesson-13/](../../examples/lesson-13/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [怎麼跑](#怎麼跑)
- [實測輸出（節錄）](#實測輸出節錄)
- [小練習](#小練習)
## 重點說明

前面 12 篇驗證節點的方法都是「整條 dataflow 跑起來，看 log」。這樣能用，但慢、結果會隨時間抖動，而且壞掉時不容易知道是哪一段錯。這篇把測試分成三層，由快到慢：

| 層級 | 測什麼 | 需要 daemon 嗎 | 指令 |
|---|---|---|---|
| 1. 單元測試 | 純邏輯（移動平均、門檻判斷） | 不用 | `cargo test` |
| 2. 整合測試 | 整個事件迴圈：假輸入 → 檢查輸出 | **不用** | `cargo test` |
| 3. 黑箱／重播 | 編譯好的執行檔本身，或重播實際錄下的輸入 | 不用 | 環境變數 + 執行檔 |
| 4. 整條 dataflow | 節點之間的連線、YAML 設定 | 要 | `dora run` |

### 1. 先把程式拆成兩半

能測的關鍵是：**`main()` 只負責初始化，事件迴圈寫成一個吃 `DoraNode` + `EventStream` 的函式**。

```rust
// main.rs
let (node, events) = DoraNode::init_from_env()?;
smoother::run(node, events, &cfg)?;      // 正式執行
// tests/integration.rs
let (node, events) = DoraNode::init_testing(inputs, outputs, options)?;
let stats = smoother::run(node, events, &cfg)?;   // 測試，同一個 run()
```

純計算（`Smoother::push`、`check_alarm`）完全不碰 dora，用一般 `#[test]` 就能測。`run()` 回傳一個 `Stats`，讓測試可以直接檢查「收到幾筆、發了幾個警告、略過幾筆壞資料」。

### 2. 整合測試模式（`dora_node_api::integration_testing`）

`dora-node-api` 1.1.0 內建一個**不連 daemon** 的測試模式，我是讀套件原始碼（`src/integration_testing.rs`、`src/node/mod.rs`）找到的。有三種用法：

- **`DoraNode::init_testing(input, output, options)`**：在測試裡直接造出一組 `(node, events)`，不動任何全域狀態，同一個測試函式裡可以跑好幾次。本篇用這個。
- **`setup_integration_testing(...)` 然後呼叫 `main()`**：設定的是「目前執行緒」的狀態，之後 `init_from_env()` 就會進測試模式。適合連 `main()` 一起測。本篇沒有實測這一種。
- **環境變數**：直接執行編好的執行檔，設 `DORA_TEST_WITH_INPUTS=輸入.json`，`init_from_env()` 就會從檔案讀事件，輸出寫到 `DORA_TEST_WRITE_OUTPUTS_TO`（預設是輸入檔旁邊的 `outputs.jsonl`）。再設 `DORA_TEST_NO_OUTPUT_TIME_OFFSET=1` 就不記時間，輸出每次都一樣，可以拿來 `diff`。

輸入的寫法（Rust 版和 JSON 版是同一個格式）：

- 事件種類：`Input`（要有 `id`，可選 `metadata`、`data`、`data_type`）、`Stop`、`InputClosed`、`AllInputsClosed`。每個事件都有 `time_offset_secs`（相對開始的秒數）。
- `data` 用 JSON 寫，`data_type` 寫 Arrow 型別名稱，例如 `"Float64"`、`"Utf8"`、`"Int32"`。
- metadata 在 JSON 裡要寫成 `{"seq": {"Integer": 1}}`（`Parameter` 列舉的外層標籤格式）。我一開始寫成 `{"seq": 1}`，執行檔直接報錯 ``invalid type: integer `1`, expected string or map``。
- 事件列表用完，事件流就關閉，`run()` 的 `while let` 自然結束。

輸出的樣子（每筆一行 JSON）：

```json
{"id":"avg","data":[1.5],"data_type":"Float64"}
```

注意：**輸出不含 metadata**，只有 `id`、`data`、`data_type`（加上沒關掉時的 `time_offset_secs`）。另外測試模式下，沒在 YAML 宣告的輸出名稱也送得出去（原始碼裡測試節點被當成 interactive，跳過 `validate_output` 檢查），所以「輸出名稱打錯」這種錯要靠第 4 層的 `dora run` 抓。

### 3. 錄製實際執行，再拿來重播

跑 dataflow 時設 `DORA_WRITE_EVENTS_TO=資料夾`，每個節點結束時會把它收到的所有事件寫成 `inputs-<節點id>.json`，格式和上面的輸入檔完全一樣，可以直接餵給 `DORA_TEST_WITH_INPUTS`。實測要注意兩件事：

- 錄下來的檔案**沒有 metadata**，所以重播時 `seq` 都讀成預設值 -1。資料本身（`data`、`data_type`）都在，平均值和警告數量和實跑一模一樣。
- 原始碼註解說錄製時會把所有事件放在記憶體裡、結束才寫檔，不適合長時間或大資料的 dataflow。

這招最適合抓「只在實際跑時才出現」的 bug：錄一次，之後在測試裡反覆重播，不用每次都把整條管線開起來。

### 4. 除錯的幾個實用習慣

- **壞資料不要 panic**：`into_vec::<f64>` 失敗時印到 stderr、計數、`continue`。實測 `into_vec` 會自動把 Int32 等數值型別轉成 f64（所以本篇改用字串 `"oops"` 當壞資料），字串才會失敗，錯誤訊息是 `Unsupported data type for conversion: Utf8`。
- **設定檔錯誤要帶上下文**：`WINDOW` 解析失敗用 `.with_context(|| format!("WINDOW={window:?} 不是整數"))`。`dora run` 失敗時會印出節點的 stderr，你一眼就看得到是哪個節點、哪個參數：
  ```
  Dataflow failed:
  Node `smoother` failed: exited with code 1 with stderr output:
  Error: window 必須 >= 1
  ```
- **`eprintln!` 在 `dora run` 的 log 裡也標成 `stdout <節點>:`**，所以 log 用 `[節點名]` 開頭，比較好 grep。
- **測試失敗先讀 `left`／`right`**：本篇的測試我第一次就寫錯了預期值（以為 2 個警告，其實平均是 6、9、12，只有 1 個超過 10），`cargo test` 印出 `left: Stats { received: 3, alarms: 1, … }`、`right: … alarms: 2 …`，就知道是測試寫錯不是程式錯。
- 更細的追蹤（`RUST_LOG`、OpenTelemetry）放在第 18 篇效能調校再講，本篇沒有實測。

## 完整範例

位置：[examples/lesson-13/](../../examples/lesson-13/)

```
examples/lesson-13/
├── Cargo.toml
├── dataflow.yml          # source → smoother → printer
├── dataflow-bad.yml      # 同上，但 WINDOW: 0（示範失敗）
├── check.sh              # 黑箱比對
├── fixtures/
│   ├── inputs.json       # 手寫輸入
│   └── expected.jsonl    # 預期輸出
├── src/
│   ├── lib.rs            # Smoother、check_alarm、run()、單元測試
│   ├── main.rs           # smoother 節點執行檔（bin 名稱 lesson-13）
│   └── bin/
│       ├── source.rs     # 每 100ms 送 2,4,6,…；第 4 筆送字串
│       └── printer.rs    # 印出 avg / alarm
└── tests/
    └── integration.rs    # init_testing 整合測試
```

### Cargo.toml

```toml
[package]
name = "lesson-13"
version = "0.1.0"
edition = "2024"

[lib]
name = "smoother"
path = "src/lib.rs"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

### src/lib.rs

```rust
//! 第 13 篇：把「邏輯」和「dora 事件迴圈」拆開，兩層都能測。
//!
//! - `Smoother`、`check_alarm`：純 Rust，不碰 dora → 一般 `#[test]` 單元測試。
//! - `run()`：吃一個 `DoraNode` + `EventStream`，正式執行和整合測試共用同一份程式碼。
use std::collections::VecDeque;

use dora_node_api::{
    DoraNode, Event, EventStream, MetadataParameters, Parameter, dora_core::config::DataId,
    get_integer_param, into_vec,
};
use eyre::{Context, bail};

/// 移動平均：保留最近 `window` 個值。
#[derive(Debug)]
pub struct Smoother {
    window: usize,
    buf: VecDeque<f64>,
    sum: f64,
}

impl Smoother {
    pub fn new(window: usize) -> eyre::Result<Self> {
        if window == 0 {
            bail!("window 必須 >= 1");
        }
        Ok(Self { window, buf: VecDeque::with_capacity(window), sum: 0.0 })
    }

    /// 放進一個新值，回傳目前的平均。
    pub fn push(&mut self, x: f64) -> f64 {
        if self.buf.len() == self.window {
            self.sum -= self.buf.pop_front().unwrap();
        }
        self.buf.push_back(x);
        self.sum += x;
        self.sum / self.buf.len() as f64
    }
}

/// 平均值超過門檻就回傳警告字串。
pub fn check_alarm(seq: i64, avg: f64, limit: f64) -> Option<String> {
    (avg > limit).then(|| format!("seq={seq} avg={avg:.2} > limit={limit:.2}"))
}

/// 節點設定（從環境變數讀，YAML 的 `env:` 可以改）。
#[derive(Debug, Clone)]
pub struct Config {
    pub window: usize,
    pub limit: f64,
}

impl Config {
    pub fn from_env() -> eyre::Result<Self> {
        let window = std::env::var("WINDOW").unwrap_or_else(|_| "3".into());
        let limit = std::env::var("LIMIT").unwrap_or_else(|_| "10".into());
        Ok(Self {
            window: window.parse().with_context(|| format!("WINDOW={window:?} 不是整數"))?,
            limit: limit.parse().with_context(|| format!("LIMIT={limit:?} 不是數字"))?,
        })
    }
}

/// 跑完之後的統計，方便測試檢查。
#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    pub received: usize,
    pub alarms: usize,
    pub bad_inputs: usize,
}

/// 事件迴圈本體。輸入 `value`：Float64 陣列（每個元素都餵進移動平均）。
/// 輸出 `avg`（每筆輸入一個 Float64）與 `alarm`（Utf8 字串，超過門檻才送）。
pub fn run(mut node: DoraNode, mut events: EventStream, cfg: &Config) -> eyre::Result<Stats> {
    let mut smoother = Smoother::new(cfg.window)?;
    let avg_out = DataId::from("avg".to_owned());
    let alarm_out = DataId::from("alarm".to_owned());
    let mut stats = Stats::default();

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "value" => {
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                // 型別不對不要 panic：記下來、印到 stderr，繼續處理下一筆
                let values: Vec<f64> = match into_vec::<f64>(&data) {
                    Ok(v) if !v.is_empty() => v,
                    Ok(_) => {
                        eprintln!("[smoother] seq={seq} 收到空陣列，略過");
                        stats.bad_inputs += 1;
                        continue;
                    }
                    Err(e) => {
                        eprintln!("[smoother] seq={seq} 型別不對：{e}");
                        stats.bad_inputs += 1;
                        continue;
                    }
                };
                stats.received += 1;
                let mut avg = 0.0;
                for v in values {
                    avg = smoother.push(v);
                }
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                node.send_output(avg_out.clone(), p.clone(), vec![avg])?;
                if let Some(msg) = check_alarm(seq, avg, cfg.limit) {
                    stats.alarms += 1;
                    node.send_output(alarm_out.clone(), p, msg)?;
                }
            }
            Event::Input { id, .. } => eprintln!("[smoother] 未知輸入 {id}，略過"),
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[smoother] 結束：{stats:?}");
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_of_partial_window() {
        let mut s = Smoother::new(3).unwrap();
        assert_eq!(s.push(3.0), 3.0);
        assert_eq!(s.push(5.0), 4.0); // 還沒滿 3 個，用實際個數平均
    }

    #[test]
    fn old_values_drop_out() {
        let mut s = Smoother::new(3).unwrap();
        for x in [1.0, 2.0, 3.0] {
            s.push(x);
        }
        assert_eq!(s.push(10.0), 5.0); // (2+3+10)/3
    }

    #[test]
    fn zero_window_is_error() {
        assert!(Smoother::new(0).is_err());
    }

    #[test]
    fn alarm_only_above_limit() {
        assert_eq!(check_alarm(1, 10.0, 10.0), None); // 等於不算超過
        assert_eq!(check_alarm(2, 10.5, 10.0).as_deref(), Some("seq=2 avg=10.50 > limit=10.00"));
    }
}
```

### src/main.rs

```rust
//! smoother 節點的執行檔：只負責初始化，邏輯都在 lib.rs。
use dora_node_api::DoraNode;

fn main() -> eyre::Result<()> {
    let cfg = smoother::Config::from_env()?;
    // init_from_env：被 dora daemon 啟動時連 daemon；
    // 設了 DORA_TEST_WITH_INPUTS 時改成「整合測試模式」，從 JSON 檔讀輸入。
    let (node, events) = DoraNode::init_from_env()?;
    smoother::run(node, events, &cfg)?;
    Ok(())
}
```

### src/bin/source.rs

```rust
//! 測試用來源節點：每個 tick 送一個 Float64 值 seq*2（2, 4, 6, …），metadata 帶 seq。
//! 第 4 筆刻意送字串 "oops"，讓 smoother 練習「型別不對」的處理。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("value".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                if seq == 4 {
                    node.send_output(out.clone(), p, "oops")?;
                } else {
                    node.send_output(out.clone(), p, vec![seq as f64 * 2.0])?;
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[source] 送出 {seq} 筆");
    Ok(())
}
```

### src/bin/printer.rs

```rust
//! 把 avg / alarm 印出來，dora logs 看得到。
use dora_node_api::{DoraNode, Event, get_integer_param, into_vec};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => {
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                match id.as_str() {
                    "avg" => println!("[printer] seq={seq} avg={:?}", into_vec::<f64>(&data)?),
                    "alarm" => {
                        let s: &str = (&data).try_into()?;
                        println!("[printer] ALARM {s}");
                    }
                    other => println!("[printer] 其他輸入 {other}"),
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
```

### tests/integration.rs

```rust
//! 整合測試：不啟動 dora daemon，直接把假事件餵給 `smoother::run()`。
use dora_node_api::{
    DoraNode, MetadataParameters, Parameter,
    integration_testing::{
        IntegrationTestInput, TestingInput, TestingOptions, TestingOutput, drain_outputs,
        integration_testing_format::{IncomingEvent, InputData, TimedIncomingEvent},
        output_channel,
    },
    serde_json::{Value, json},
};
use smoother::{Config, Stats, run};

/// 造一個 `value` 輸入事件：data 用 JSON 表示，data_type 指定 Arrow 型別。
fn input(t: f64, seq: i64, data: Value, data_type: &str) -> TimedIncomingEvent {
    let mut p = MetadataParameters::new();
    p.insert("seq".into(), Parameter::Integer(seq));
    TimedIncomingEvent {
        time_offset_secs: t,
        event: IncomingEvent::Input {
            id: "value".into(),
            metadata: Some(p),
            data: Some(Box::new(InputData::JsonObject {
                data,
                data_type: Some(json!(data_type)),
            })),
        },
    }
}

fn stop(t: f64) -> TimedIncomingEvent {
    TimedIncomingEvent { time_offset_secs: t, event: IncomingEvent::Stop }
}

/// 跑一次節點，回傳 (統計, 輸出 JSON 陣列)。
fn run_with(events: Vec<TimedIncomingEvent>, cfg: Config) -> (Stats, Vec<Value>) {
    let inputs = TestingInput::Input(IntegrationTestInput::new("smoother".parse().unwrap(), events));
    let (tx, mut rx) = output_channel();
    let options = TestingOptions { skip_output_time_offsets: true }; // 不記時間 → 結果固定
    let (node, events) =
        DoraNode::init_testing(inputs, TestingOutput::ToChannel(tx), options).unwrap();
    let stats = run(node, events, &cfg).unwrap();
    let outputs = drain_outputs(&mut rx).into_iter().map(Value::Object).collect();
    (stats, outputs)
}

#[test]
fn moving_average_and_alarm() {
    let events = vec![
        input(0.00, 1, json!([6.0]), "Float64"),
        input(0.01, 2, json!([12.0]), "Float64"),
        input(0.02, 3, json!([18.0]), "Float64"), // 平均依序 6、9、12，只有 12 > 10 → 1 個 alarm
        stop(0.03),
    ];
    let (stats, out) = run_with(events, Config { window: 3, limit: 10.0 });
    assert_eq!(stats, Stats { received: 3, alarms: 1, bad_inputs: 0 });

    let ids: Vec<&str> = out.iter().map(|o| o["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["avg", "avg", "avg", "alarm"]);
    // avg 依序是 6、9、12
    let avgs: Vec<f64> = out
        .iter()
        .filter(|o| o["id"] == "avg")
        .map(|o| o["data"][0].as_f64().unwrap())
        .collect();
    assert_eq!(avgs, [6.0, 9.0, 12.0]);
    assert_eq!(out[3]["data"][0], "seq=3 avg=12.00 > limit=10.00");
}

#[test]
fn wrong_type_is_skipped_not_panicking() {
    let events = vec![
        input(0.00, 1, json!(["hello"]), "Utf8"), // 字串 → into_vec::<f64> 失敗
        input(0.01, 2, json!([]), "Float64"),     // 空陣列
        input(0.02, 3, json!([1.0, 2.0, 3.0]), "Float64"),
        stop(0.03),
    ];
    let (stats, out) = run_with(events, Config { window: 2, limit: 100.0 });
    assert_eq!(stats, Stats { received: 1, alarms: 0, bad_inputs: 2 });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["data"][0].as_f64(), Some(2.5)); // 視窗 2：只剩 2、3
}

#[test]
fn print_raw_output_format() {
    // 給讀者看輸出 JSON 長什麼樣：cargo test -- --nocapture print_raw
    let (_, out) = run_with(vec![input(0.0, 7, json!([1.5]), "Float64"), stop(0.01)],
        Config { window: 3, limit: 10.0 });
    for o in &out {
        println!("{o}");
    }
}
```

### fixtures/inputs.json

```json
{
  "id": "smoother",
  "events": [
    { "time_offset_secs": 0.00, "type": "Input", "id": "value", "metadata": { "seq": { "Integer": 1 } }, "data": [4.0],  "data_type": "Float64" },
    { "time_offset_secs": 0.01, "type": "Input", "id": "value", "metadata": { "seq": { "Integer": 2 } }, "data": [8.0],  "data_type": "Float64" },
    { "time_offset_secs": 0.02, "type": "Input", "id": "value", "metadata": { "seq": { "Integer": 3 } }, "data": [30.0], "data_type": "Float64" },
    { "time_offset_secs": 0.03, "type": "Stop" }
  ]
}
```

### fixtures/expected.jsonl

```json
{"id":"avg","data":[4.0],"data_type":"Float64"}
{"id":"avg","data":[6.0],"data_type":"Float64"}
{"id":"avg","data":[14.0],"data_type":"Float64"}
{"id":"alarm","data":["seq=3 avg=14.00 > limit=10.00"],"data_type":"Utf8"}
```

### check.sh

```bash
#!/usr/bin/env bash
# 黑箱測試：拿「編譯好的那個執行檔」跑固定輸入，和預期輸出逐行比對。
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --bin lesson-13 -q
out=$(mktemp)
DORA_TEST_WITH_INPUTS=fixtures/inputs.json \
DORA_TEST_WRITE_OUTPUTS_TO="$out" \
DORA_TEST_NO_OUTPUT_TIME_OFFSET=1 \
  ./target/release/lesson-13 </dev/null 2>/dev/null
if diff -u fixtures/expected.jsonl "$out"; then
  echo "PASS：輸出與 fixtures/expected.jsonl 完全相同"
else
  echo "FAIL：輸出不同（上面是 diff）"; exit 1
fi
```

### dataflow.yml

```yaml
nodes:
  - id: source
    build: cargo build --release --bin source
    path: target/release/source
    inputs:
      tick: dora/timer/millis/100
    outputs:
      - value

  - id: smoother
    build: cargo build --release --bin lesson-13
    path: target/release/lesson-13
    env:
      WINDOW: 3
      LIMIT: 10
    inputs:
      value: source/value
    outputs:
      - avg
      - alarm

  - id: printer
    build: cargo build --release --bin printer
    path: target/release/printer
    inputs:
      avg: smoother/avg
      alarm: smoother/alarm
```

`dataflow-bad.yml` 和它一樣，只把 `WINDOW: 3` 改成 `WINDOW: 0`。

## 怎麼跑

```bash
cd examples/lesson-13   # 自 repo 根目錄
export PATH="$HOME/dora/bin:$PATH"   # 依你的 dora CLI 安裝位置調整

# 第 1、2 層：單元 + 整合測試
cargo test --release
cargo test --release --test integration -- --nocapture print_raw   # 看輸出 JSON 長相

# 第 3 層：黑箱比對編好的執行檔
./check.sh

# 第 4 層：整條跑，順便錄下每個節點的輸入
dora validate dataflow.yml
mkdir -p recorded
DORA_WRITE_EVENTS_TO=$PWD/recorded dora run dataflow.yml --stop-after 2s; echo "exit=$?"

# 重播錄下來的 smoother 輸入
DORA_TEST_WITH_INPUTS=recorded/inputs-smoother.json \
DORA_TEST_WRITE_OUTPUTS_TO=/tmp/replay.jsonl \
DORA_TEST_NO_OUTPUT_TIME_OFFSET=1 \
  ./target/release/lesson-13 </dev/null
grep -c alarm /tmp/replay.jsonl

# 示範失敗
dora run dataflow-bad.yml --stop-after 2s; echo "exit=$?"
```

> 直接執行節點時記得加 `</dev/null`：`init_from_env()` 發現 stdin 是終端機、又沒設測試變數時，會進「互動模式」等你打字。

## 實測輸出（節錄）

`cargo test --release`：

```
test tests::average_of_partial_window ... ok
test tests::alarm_only_above_limit ... ok
test tests::old_values_drop_out ... ok
test tests::zero_window_is_error ... ok
test result: ok. 4 passed; 0 failed; ...
     Running tests/integration.rs
test print_raw_output_format ... ok
test moving_average_and_alarm ... ok
test wrong_type_is_skipped_not_panicking ... ok
test result: ok. 3 passed; 0 failed; ...
```

`./check.sh`（正常）與把預期值 14 改成 13 之後（結束碼 1）：

```
PASS：輸出與 fixtures/expected.jsonl 完全相同
---
-{"id":"avg","data":[13.0],"data_type":"Float64"}
-{"id":"alarm","data":["seq=3 avg=13.00 > limit=10.00"],"data_type":"Utf8"}
+{"id":"avg","data":[14.0],"data_type":"Float64"}
+{"id":"alarm","data":["seq=3 avg=14.00 > limit=10.00"],"data_type":"Utf8"}
FAIL：輸出不同（上面是 diff）
```

`dora run dataflow.yml --stop-after 2s`（結束碼 0）：

```
stdout printer: [printer] seq=1 avg=[2.0]
stdout printer: [printer] seq=2 avg=[3.0]
stdout printer: [printer] seq=3 avg=[4.0]
stdout smoother: [smoother] seq=4 型別不對：Unsupported data type for conversion: Utf8
stdout printer: [printer] seq=5 avg=[6.666666666666667]
stdout printer: [printer] seq=6 avg=[9.333333333333334]
stdout printer: [printer] seq=7 avg=[12.0]
stdout printer: [printer] ALARM seq=7 avg=12.00 > limit=10.00
...
stdout printer: [printer] ALARM seq=15 avg=28.00 > limit=10.00
stdout source: [source] 送出 15 筆
stdout smoother: [smoother] 結束：Stats { received: 14, alarms: 9, bad_inputs: 1 }
```

重播 `recorded/inputs-smoother.json`（16 個事件，含最後的 Stop）：

```
[smoother] seq=-1 型別不對：Unsupported data type for conversion: Utf8    ← 錄檔沒有 metadata，seq 變 -1
[smoother] 結束：Stats { received: 14, alarms: 9, bad_inputs: 1 }       ← 與實跑一致
```

`dora run dataflow-bad.yml`（結束碼 1）：

```
Dataflow failed:
Node `smoother` failed: exited with code 1 with stderr output:
Error: window 必須 >= 1

Location:
    src/lib.rs:24:13
```

另外，在測試模式下每次結束會多印三行 `node reports EventStreamDropped`、`node reports closed outputs []`、`node reports OutputsDone`，這是 dora-node-api 自己印的，不是錯誤。

## 小練習

1. 在 `tests/integration.rs` 加一個測試：送一個 `InputClosed { id: "value" }` 事件，再送一筆 `value`，觀察 `run()` 還會不會處理那一筆（提示：`IncomingEvent::InputClosed { id: "value".into() }`），把結果寫成斷言。
2. 讓 smoother 多一個輸出 `stats`：收到 `Stop` 時送出 `[received, alarms, bad_inputs]`（Int64 陣列）。先改 `fixtures/expected.jsonl` 讓 `check.sh` 失敗，再改程式讓它通過（先寫測試、再寫程式）。
3. 進階：寫一個小腳本，把 `recorded/inputs-smoother.json` 每個 `Input` 事件依序補上 `"metadata": {"seq": {"Integer": N}}`，重播後確認 log 裡的 `seq` 不再是 -1。

下一篇：[第 14 篇](lesson-14.md)講分散式部署：coordinator 與 daemon 分在不同機器上跑。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 12 篇](lesson-12.md)
- 下一課：[第 14 篇 →](lesson-14.md)
- 本課範例：[examples/lesson-13/](../../examples/lesson-13/)
