# dora-rs 教學 第 4 篇：第一個像樣的 Rust 節點——`DoraNode` 與事件迴圈

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-04（台北時間），x86_64 Linux 上實際 `dora validate` / `dora build` / `dora run` 成功（結束碼 0）。


**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 3 篇](lesson-03.md)
- 下一課：[第 5 篇 →](lesson-05.md)
- 本課範例：[examples/lesson-04/](../../examples/lesson-04/)

## 目錄

- [重點說明](#重點說明)
- [完整可執行範例：sensor → stats → printer](#完整可執行範例sensor-stats-printer)
- [小練習](#小練習)
## 重點說明

前三篇的節點都很短。這一篇把 Rust 節點的骨架講完整：怎麼初始化、事件迴圈怎麼寫、有哪些事件、怎麼送輸出，以及節點「該怎麼收工」。

### 1. 骨架只有三步

```rust
let (mut node, mut events) = DoraNode::init_from_env()?;   // ① 初始化
while let Some(event) = events.recv() {                    // ② 事件迴圈
    match event { /* ... */ _ => {} }
}                                                          // ③ 迴圈結束 = 節點結束
```

- `DoraNode::init_from_env()` 讀的是 daemon 啟動節點時設好的環境變數，所以**直接 `./my-node` 手動執行會失敗**（沒有 daemon 給設定）。要用 `dora run` / `dora start` 起。
- 回傳兩個東西：`DoraNode`（**發送端**，負責 `send_output`、`log_info` 等）和 `EventStream`（**接收端**，負責 `recv`）。分開是為了讓你能一邊迴圈收事件、一邊用 `node` 送資料，不會互相借用打架。
- `events.recv()` 會阻塞等下一個事件；回 `None` 代表事件串流已經結束，節點也該結束了。
- 沒有 `inputs` 的節點（純資料來源，例如感測器）一樣要 `init_from_env()`，`events` 用不到也要留著，不要提早丟掉。

### 2. `Event` 有哪些種類

**`Input { id, metadata, data }`**

- 什麼時候來：某個輸入來資料。`id` 是**你在 YAML 裡取的輸入名稱**，不是上游的輸出名稱
- 你通常怎麼處理：依 `id` 分派；`data` 是 Arrow 陣列

**`InputClosed { id }`**

- 什麼時候來：該輸入的上游節點結束了，不會再有資料
- 你通常怎麼處理：清理狀態；若這是唯一資料來源就收工

**`Stop(cause)`**

- 什麼時候來：事件串流即將關閉。`cause` 是 `Manual`（`dora stop` / Ctrl-C，要盡快退出）或 `AllInputsClosed`（所有輸入都關了）
- 你通常怎麼處理：跳出迴圈

**`Error(msg)`**

- 什麼時候來：dora 內部錯誤，**以及 `recv_timeout` 逾時**
- 你通常怎麼處理：印出來；不要直接當成致命錯誤

**`NodeFailed { .. }` / `NodeRestarted { id }` / `InputRecovered { id }`**

- 什麼時候來：上游節點失敗、重啟、恢復（搭配重啟策略，第 9 篇）
- 你通常怎麼處理：先印出來；進階可切換備援資料

**`ParamUpdate { key, value }` / `ParamDeleted { key }`**

- 什麼時候來：有人用 `dora param set / delete` 動態改參數
- 你通常怎麼處理：調整門檻、頻率等

**`Reload { .. }`**

- 什麼時候來：目前只給 Python operator 熱重載用
- 你通常怎麼處理：一般節點用不到

`Event` 與 `StopCause` 都是 `#[non_exhaustive]`，所以 `match` 一定要留 `_ => {}`（或像範例一樣印出來）。

### 3. 送輸出：`send_output`

```rust
node.send_output(output_id, MetadataParameters::default(), value)?;
```

- `output_id` 是 `DataId`，要先在 YAML 的 `outputs:` 宣告過；**沒宣告的輸出會被默默忽略（回 `Ok`）**，這是常見的「怎麼下游沒收到」原因。
- 第二個參數是 metadata 參數（附帶的小標籤，第 5 篇細講），不需要就給 `Default::default()`；想把收到的標籤原樣傳下去就用 `metadata.parameters`（第 1、3 篇是這樣）。
- 第三個參數只要實作了 `IntoArrow` 就行：`f64`、`u64` 等純量、`Vec<T>`、`&str`、Arrow 陣列都可以。這篇用 `f64` 和 `&str`。
- 收的那一邊：`f64::try_from(&data)?` 取純量，`TryFrom::try_from(&data)` 取 `&str`。型別對不上會得到錯誤，不會 panic。

### 4. 逾時：`recv_timeout`

`events.recv_timeout(Duration)` 最多等指定時間。**逾時不是回 `None`，而是回一個 `Event::Error`**（訊息是 `Timeout event stream error: Receiver timed out`，`None` 仍然只代表串流結束）。想做「上游多久沒資料就警告」之類的看門狗，就在 `Error` 分支處理。

### 5. 節點怎麼「收工」

這一點最容易踩雷：

- 迴圈結束（`recv` 回 `None`、或你自己 `break`）→ `main` 回傳 → 節點結束，dora 會對它的下游發 `InputClosed`。
- 當節點**同時有計時器輸入**時，計時器永遠不會關閉，`AllInputsClosed` 就不會來。所以範例中的 `stats` 在「`reading` 這個資料輸入關閉」時**自己 `break`**；否則整個資料流會一直跑下去。
- 收到 `Stop(Manual)` 要盡快退出，dora 會在等一段時間後強制殺掉不退出的節點。

## 完整可執行範例：sensor → stats → printer

```
examples/lesson-04/
├── Cargo.toml
├── dataflow.yml
└── src/bin/
    ├── sensor.rs
    ├── stats.rs
    └── printer.rs
```

`Cargo.toml`：

```toml
[package]
name = "lesson-04"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

`dataflow.yml`：

```yaml
nodes:
  - id: sensor
    description: 模擬感測器：送 8 筆讀數，第 4 筆後故意停頓 1.5 秒
    build: cargo build --bin sensor
    path: target/debug/sensor
    outputs:
      - reading

  - id: stats
    description: 第一個「完整」的 Rust 節點：累計讀數、每秒送一次摘要
    build: cargo build --bin stats
    path: target/debug/stats
    inputs:
      reading: sensor/reading
      report: dora/timer/millis/1000
    outputs:
      - summary

  - id: printer
    description: 印出 stats 的摘要
    build: cargo build --bin printer
    path: target/debug/printer
    inputs:
      summary: stats/summary
```

`src/bin/sensor.rs`：

```rust
use dora_node_api::{DoraNode, MetadataParameters, dora_core::config::DataId};
use std::{thread::sleep, time::Duration};

fn main() -> eyre::Result<()> {
    // 沒有 inputs 的節點也要 init；events 這裡用不到，但要留著不能丟掉
    let (mut node, _events) = DoraNode::init_from_env()?;
    let out = DataId::from("reading".to_owned());

    for i in 1..=8u32 {
        let value = 20.0 + (i as f64) * 0.5; // 假裝是溫度
        println!("sensor 送出第 {i} 筆：{value}");
        // f64 實作了 IntoArrow，所以可以直接丟進 send_output
        node.send_output(out.clone(), MetadataParameters::default(), value)?;
        if i == 4 {
            println!("sensor 故意停頓 1.5 秒…");
            sleep(Duration::from_millis(1500));
        } else {
            sleep(Duration::from_millis(200));
        }
    }
    println!("sensor 結束");
    Ok(())
}
```

`src/bin/stats.rs`（本篇主角）：

```rust
use dora_node_api::{
    DoraNode, Event, MetadataParameters, StopCause, dora_core::config::DataId,
};
use std::time::Duration;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let summary = DataId::from("summary".to_owned());

    // 節點自己的狀態：就放在迴圈外的區域變數
    let (mut count, mut sum, mut max) = (0u32, 0.0f64, f64::MIN);

    // recv_timeout：最多等 500 ms；逾時不會回 None，而是回一個 Event::Error
    while let Some(event) = events.recv_timeout(Duration::from_millis(500)) {
        match event {
            Event::Input { id, data, .. } => match id.as_str() {
                "reading" => {
                    let v = f64::try_from(&data)?; // Arrow 陣列 → f64
                    count += 1;
                    sum += v;
                    max = max.max(v);
                    println!("stats 收到 reading={v}（累計 {count} 筆）");
                }
                "report" => {
                    let text = if count == 0 {
                        "尚無資料".to_string()
                    } else {
                        format!("{count} 筆，平均 {:.2}，最大 {max:.1}", sum / count as f64)
                    };
                    node.send_output(summary.clone(), MetadataParameters::default(), text.as_str())?;
                }
                other => println!("stats 收到沒預期的輸入 {other}"),
            },
            Event::InputClosed { id } => {
                println!("stats：輸入 {id} 已關閉");
                if id.as_str() == "reading" {
                    // 感測器沒資料了：送最後一份摘要，然後自己收工
                    let text = format!("最終：{count} 筆，總和 {sum:.1}");
                    node.send_output(summary.clone(), MetadataParameters::default(), text.as_str())?;
                    break;
                }
            }
            Event::Stop(cause) => {
                match cause {
                    StopCause::Manual => println!("stats：被手動停止（dora stop / Ctrl-C）"),
                    StopCause::AllInputsClosed => println!("stats：所有輸入都關閉了"),
                    _ => println!("stats：停止（其他原因）"),
                }
                break;
            }
            // 逾時（recv_timeout 沒等到事件）會以 Error 出現
            Event::Error(msg) => println!("stats：Error 事件：{msg}"),
            // 其他事件（NodeFailed、ParamUpdate…）先印出來看看
            other => println!("stats：其他事件 {other:?}"),
        }
    }
    println!("stats 結束（共 {count} 筆）");
    Ok(())
}
```

`src/bin/printer.rs`：

```rust
use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { data, .. } => {
                let text: &str = TryFrom::try_from(&data)?;
                println!("printer 摘要：{text}");
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
```

執行：

```bash
export PATH=$HOME/dora/bin:$PATH
cd examples/lesson-04              # 在 clone 本 repo 後，於 repo 根目錄執行
dora validate dataflow.yml
dora build dataflow.yml
dora run dataflow.yml
```

**實測輸出（2026-11-04 08:11 台北時間，`dora run` 結束碼 0；為節省篇幅略去 dora 自己的 INFO 行與重複行）**：

```text
sensor 送出第 1 筆：20.5
stats 收到 reading=20.5（累計 1 筆）
printer 摘要：1 筆，平均 20.50，最大 20.5        ← 計時器一啟動就先觸發一次
sensor 送出第 2 筆：21
stats 收到 reading=21（累計 2 筆）
sensor 送出第 3 筆：21.5
stats 收到 reading=21.5（累計 3 筆）
sensor 送出第 4 筆：22
sensor 故意停頓 1.5 秒…
stats 收到 reading=22（累計 4 筆）
printer 摘要：4 筆，平均 21.25，最大 22.0         ← 每秒一次的 report
stats：Error 事件：Timeout event stream error: Receiver timed out   ← 感測器停頓，500 ms 沒事件
printer 摘要：4 筆，平均 21.25，最大 22.0
sensor 送出第 5 筆：22.5
…（第 6～8 筆照常收到）
sensor 結束
stats：輸入 reading 已關閉
stats 結束（共 8 筆）
printer 摘要：最終：8 筆，總和 178.0
daemon sensor / stats / printer finished successfully
daemon dataflow finished
```

（實際輸出的逾時 Error 訊息後面還會跟著幾行 `Location: …/event_stream/mod.rs:1304:17`，那是 eyre 錯誤報告的位置資訊，可以忽略。）

**再測一次手動停止**：`dora run dataflow.yml --stop-after 2s`（2 秒後模擬 Ctrl-C），`stats` 印出：

```text
stats：被手動停止（dora stop / Ctrl-C）
stats 結束（共 4 筆）
```

對應 `Stop(StopCause::Manual)`。另外可以看到 `sensor` 沒有處理 `Stop`，所以它照樣把 8 筆送完才結束，這次剛好很快結束所以沒被強制終止；真實專案裡的長時間節點務必要處理 `Stop`。

> 編譯備註：第一次 `dora build` 約 1 分 09 秒（8 核，依賴已快取一部分）；實驗完我已清掉 `target/`。你在自己的機器上練習完也建議 `cargo clean`。

## 小練習

1. 把 `stats` 的 `recv_timeout` 改成 `recv()`，重跑。逾時那一行還會出現嗎？為什麼？
2. 把 `stats` 在 `InputClosed` 時的 `break` 拿掉再執行。資料流會結束嗎？（提示：想想 `report` 計時器。記得用 `dora run dataflow.yml --stop-after 8s` 避免跑不停。）
3. 進階：在 `stats` 加一個輸出 `alert`，當「連續 2 次逾時」就送出一則警告字串，再新增一個節點接收並印出。

下一篇：[第 5 篇：Arrow 資料與 metadata——除了純量，還能送什麼？](lesson-05.md)。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 3 篇](lesson-03.md)
- 下一課：[第 5 篇 →](lesson-05.md)
- 本課範例：[examples/lesson-04/](../../examples/lesson-04/)
