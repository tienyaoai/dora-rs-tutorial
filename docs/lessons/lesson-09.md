# dora-rs 教學 第 9 篇：錯誤處理與重啟——節點當掉時，dora 怎麼幫你撐住

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-09（台北時間），僅在 x86_64 Linux 上實測。3 份 YAML 的 `dora validate` 結束碼都是 0，`dora build` 成功。實測結果：`on-failure.yml` 跑 3 次，worker 當掉 2 次、被自動重啟 2 次，資料流結束碼 0；`never.yml` 跑 4 次，worker 第一次當掉資料流就以結束碼 1 失敗；`exhausted.yml`（重啟上限 1 次）跑 1 次，第二次當掉後放棄重啟，結束碼 1。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 8 篇](lesson-08.md)
- 下一課：第 10 篇**尚未發布**（僅規劃；零拷貝大資料等見 [課程大綱](../../README.md#課程大綱20-篇規劃)）
- 本課範例：[examples/lesson-09/](../../examples/lesson-09/)

## 目錄

- [重點說明](#重點說明)
- [完整範例：producer → worker（會當掉）→ sink](#完整範例producer-worker會當掉-sink)
- [實測結果（2026-11-09，x86_64 Linux）](#實測結果2026-11-09x8664-linux)
- [設計建議](#設計建議)
- [小練習](#小練習)
## 重點說明

前 8 篇的節點都「乖乖跑完」。真實系統裡，感測器會送來壞資料、驅動會出錯、程式會 panic。本篇分兩層談：

1. **節點內部**：哪些錯誤自己吞掉繼續跑、哪些錯誤該讓行程結束。
2. **dataflow 層級**：用 YAML 的 `restart_policy` 讓 dora daemon 自動把當掉的節點拉起來，並讓下游知道發生了什麼。

### 1. 先分類：可恢復 vs 致命

| 類型 | 例子 | 建議作法 |
|---|---|---|
| 可恢復（單筆資料問題） | 校驗碼錯、數值超出範圍、某一張影像解碼失敗 | 記錄警告，`continue` 處理下一筆，**不要讓節點退出** |
| 致命（節點已經不可信） | 輸入型別接錯、硬體裝置斷線、內部狀態壞掉 | 用 `?` 或 `bail!` 回傳 `Err`，讓 `main` 以**非 0 結束碼**退出，交給 `restart_policy` |

Rust 的好處是這個分類會寫在程式碼裡：`match` 處理掉的就是可恢復，`?` 往上拋的就是致命。`main` 回傳 `eyre::Result<()>` 時，回傳 `Err` 會印出錯誤鏈並以結束碼 1 退出；`panic!` 也會讓結束碼非 0，dora 對兩者一視同仁。

> 建議用 `.context("...")` 替錯誤加上「我當時在做什麼」，dora 會把 stderr 原文轉給你看（下面的實測日誌就是這樣）。

### 2. `restart_policy`：三種選擇

寫在節點層級（v1.1.0 原始碼 `libraries/message/src/descriptor.rs`）：

| 值 | 行為 |
|---|---|
| `never`（預設） | 不重啟。節點非 0 退出，整個 `dora run` 最後回報 `Dataflow failed`，結束碼 1 |
| `on-failure` | 只在**非 0 結束碼**時重啟；正常結束（0）就不再拉起 |
| `always` | 不論結束碼都重啟；但被使用者 `dora stop` 停掉、或所有輸入都已關閉且非 0 退出時不重啟 |

搭配的參數：

| 參數 | 意義 |
|---|---|
| `max_restarts` | 最多重啟幾次；**0 代表不限**（預設 0，小心無限重啟） |
| `restart_delay` | 第一次重啟前等幾秒，之後**每次加倍**（指數退避）：0.2 → 0.4 → 0.8… |
| `max_restart_delay` | 退避上限，例如 `restart_delay: 1.0` 加 `max_restart_delay: 30.0` 會是 1, 2, 4, 8, 16, 30, 30… |
| `restart_window` | 計數視窗（秒）。設了之後語意變成「M 秒內最多 N 次」，視窗過了計數歸零 |
| `health_check_timeout` | 節點**連上 daemon 之後**若超過這麼久沒跟 daemon 互動，就被殺掉並套用重啟策略（不管啟動慢） |
| `startup_timeout` | 行程啟動後若這麼久還沒完成 `init`（訂閱事件），就殺掉並套用重啟策略 |

`health_check_timeout` 與 `startup_timeout` 本篇沒有實測，只依原始碼註解說明；它們處理的是「卡住不動」而不是「當掉」。

### 3. 節點怎麼知道自己是重生的？

`DoraNode` 有 `restart_count()`：第一次啟動是 0，第一次重啟後是 1，依此類推。重生後的節點是**全新的行程**，記憶體裡的變數全部重來，要保留的狀態得自己存（檔案、上游重送等）。

### 4. 下游會收到什麼事件？

v1.1.0 的 `Event` 有兩個跟故障有關的變體：

- `Event::NodeRestarted { id }`：上游依重啟策略**成功重啟**時送給下游，可以拿來清快取、重置狀態。
- `Event::NodeFailed { source_node_id, error, affected_input_ids }`：上游以非 0 退出時送給下游，內含 stderr 錯誤訊息。

另外 `Event` 標成 `#[non_exhaustive]`，`match` 一定要留 `_ => {}`，未來版本加新事件你的程式才不會壞。

## 完整範例：producer → worker（會當掉）→ sink

位置：[examples/lesson-09/](../../examples/lesson-09/)

- **producer**：每 100ms 送一個序號 1～20，送完自己結束。
- **worker**：5 的倍數當作壞資料（可恢復，略過）；用環境變數 `CRASH_AT="7,12"` 指定「第 0 代在 seq=7 當掉、第 1 代在 seq=12 當掉」（致命）；正常資料乘 10 送出。
- **sink**：收結果，並印出 `NodeRestarted`、`NodeFailed`、`InputClosed`，最後列出收到哪些序號。

### Cargo.toml

```toml
[package]
name = "lesson-09"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

### src/bin/producer.rs

```rust
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let total: u64 = std::env::var("TOTAL").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let mut seq: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                node.send_output("seq".into(), MetadataParameters::default(), seq)?;
                println!("producer 送出 seq={seq}");
                if seq >= total {
                    break; // 計時器不會關閉，來源節點自己收工
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("producer 結束：共送出 {seq} 筆");
    Ok(())
}
```

### src/bin/worker.rs

```rust
//! 示範三種錯誤處理：
//! 1. 可恢復的資料錯誤：記錄後 `continue`，節點繼續活著
//! 2. 致命錯誤：回傳 `Err`，行程以非 0 結束碼退出，交給 YAML 的 restart_policy
//! 3. 重啟後用 `restart_count()` 得知自己是第幾次重生
use dora_node_api::{DoraNode, Event, MetadataParameters};
use eyre::{Context, bail, eyre};

/// 模擬「感測器讀值」處理：5 的倍數當作壞資料（可恢復）
fn process(seq: u64) -> eyre::Result<u64> {
    if seq % 5 == 0 {
        return Err(eyre!("seq={seq} 校驗失敗（模擬壞資料）"));
    }
    Ok(seq * 10)
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) =
        DoraNode::init_from_env().context("worker 無法連上 dora daemon")?;
    let restart = node.restart_count();

    // CRASH_AT="7,12" 表示：第 0 代在 seq=7 當掉、第 1 代在 seq=12 當掉
    let crash_at: Vec<u64> = std::env::var("CRASH_AT")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let my_crash = crash_at.get(restart as usize).copied();
    println!("worker 啟動：restart_count={restart}，本代預計當掉於 {my_crash:?}");

    let mut ok = 0u32;
    let mut bad = 0u32;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "seq" => {
                // 型別不對屬於「接線錯誤」，直接用 ? 往上拋（致命）
                let seq = u64::try_from(&data).context("seq 不是 u64")?;

                if Some(seq) == my_crash {
                    // 致命錯誤：回傳 Err → main 結束碼非 0 → daemon 依 restart_policy 判斷
                    bail!("worker 第 {restart} 代在 seq={seq} 模擬當機");
                }

                match process(seq) {
                    Ok(v) => {
                        ok += 1;
                        node.send_output("result".into(), MetadataParameters::default(), v)?;
                    }
                    Err(e) => {
                        bad += 1;
                        eprintln!("worker 警告：{e:#}，略過這筆");
                        continue; // 可恢復：不讓一筆壞資料拖垮整個節點
                    }
                }
            }
            Event::InputClosed { id } => println!("worker 輸入 {id} 已關閉"),
            Event::Stop(cause) => {
                println!("worker 收到 Stop({cause:?})");
                break;
            }
            _ => {}
        }
    }
    println!("worker 第 {restart} 代正常結束：成功 {ok} 筆、壞資料 {bad} 筆");
    Ok(())
}
```

### src/bin/sink.rs

```rust
use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut got: Vec<u64> = Vec::new();
    let mut restarts = 0u32;
    let mut failures = 0u32;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "result" => {
                let v = u64::try_from(&data)?;
                got.push(v / 10);
            }
            // 上游依 restart_policy 重啟成功
            Event::NodeRestarted { id } => {
                restarts += 1;
                println!("sink 通知：上游 {id} 已重啟（第 {restarts} 次），已收 {} 筆", got.len());
            }
            // 上游以非 0 結束碼退出
            Event::NodeFailed { source_node_id, error, affected_input_ids } => {
                failures += 1;
                println!("sink 通知：上游 {source_node_id} 失敗，影響輸入 {affected_input_ids:?}：{error}");
            }
            Event::InputClosed { id } => println!("sink 輸入 {id} 已關閉"),
            Event::Stop(cause) => {
                println!("sink 收到 Stop({cause:?})");
                break;
            }
            _ => {} // Event 是 non_exhaustive，不認得的事件就忽略
        }
    }
    println!("sink 總結：收到 {} 筆 {:?}；NodeRestarted {restarts} 次、NodeFailed {failures} 次", got.len(), got);
    Ok(())
}
```

### on-failure.yml

```yaml
nodes:
  - id: producer
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "20"
    inputs:
      tick: dora/timer/millis/100
    outputs:
      - seq

  - id: worker
    build: cargo build --bin worker
    path: target/debug/worker
    restart_policy: on-failure
    max_restarts: 3
    restart_delay: 0.2
    env:
      CRASH_AT: "7,12"
    inputs:
      seq: producer/seq
    outputs:
      - result

  - id: sink
    build: cargo build --bin sink
    path: target/debug/sink
    inputs:
      result: worker/result
```

另外兩份 YAML 只改 worker 的策略：

- `never.yml`：`restart_policy: never`
- `exhausted.yml`：`restart_policy: on-failure`、`max_restarts: 1`、`restart_delay: 0.2`

### 執行

```bash
cd examples/lesson-09   # 在 clone 本 repo 後，於 repo 根目錄執行
dora validate on-failure.yml
dora build on-failure.yml          # 三份 YAML 共用同一組 binary
dora run on-failure.yml;  echo "exit=$?"
dora run never.yml;       echo "exit=$?"
dora run exhausted.yml;   echo "exit=$?"
```

## 實測結果（2026-11-09，x86_64 Linux）

### on-failure.yml：自動重啟 2 次，資料流成功（結束碼 0，3 次結果相同）

節錄日誌：

```text
worker: worker 啟動：restart_count=0，本代預計當掉於 Some(7)
worker: worker 警告：seq=5 校驗失敗（模擬壞資料），略過這筆
worker: Error: worker 第 0 代在 seq=7 模擬當機
daemon: node exited with error: ExitCode(1) node_id=worker
worker: daemon waiting 200ms before restart (attempt 1)
sink:   sink 通知：上游 worker 已重啟（第 1 次），已收 5 筆
worker: worker 啟動：restart_count=1，本代預計當掉於 Some(12)
worker: worker 警告：seq=10 校驗失敗（模擬壞資料），略過這筆
worker: Error: worker 第 1 代在 seq=12 模擬當機
worker: daemon waiting 400ms before restart (attempt 2)
sink:   sink 通知：上游 worker 已重啟（第 2 次），已收 6 筆
worker: worker 啟動：restart_count=2，本代預計當掉於 None
worker: worker 第 2 代正常結束：成功 3 筆、壞資料 1 筆
sink:   sink 總結：收到 9 筆 [1, 2, 3, 4, 6, 11, 17, 18, 19]；NodeRestarted 2 次、NodeFailed 0 次
```

觀察重點：

1. **指數退避真的有效**：第 1 次等 200ms、第 2 次等 400ms。
2. **重啟期間的資料會遺失**：sink 少了 8、9（第 1 次重啟空窗）與 13～16（第 2 次空窗，等得更久所以丟得更多）。7 與 12 是當機當下那筆，5、10、15、20 是 worker 自己略過的壞資料。**重啟不是重送**：上游不會因為你重生就補送，需要「不漏資料」得在應用層自己做（例如上游保留緩衝、序號對帳後請求重送）。
3. 重啟成功時，下游收到的是 `NodeRestarted`，**沒有** `NodeFailed`，`dora run` 結束碼 0。

### never.yml：不重啟，整個資料流失敗（結束碼 1，4 次結果相同）

```text
worker: Error: worker 第 0 代在 seq=7 模擬當機
sink:   sink 輸入 result 已關閉
sink:   sink 收到 Stop(AllInputsClosed)
sink:   sink 總結：收到 5 筆 [1, 2, 3, 4, 6]；NodeRestarted 0 次、NodeFailed 0 次
producer: producer 結束：共送出 20 筆
[ERROR] Dataflow failed: Node `worker` failed: exited with code 1 ...
```

注意：這 4 次實測，sink 都**只收到 `InputClosed`，沒有收到 `NodeFailed`**，就因為所有輸入都關閉而停止。所以不要把「一定會收到 `NodeFailed`」當成唯一的故障偵測手段，`InputClosed` 也要處理。producer 仍把 20 筆送完（送往已退出的節點不會讓 producer 出錯）。

### exhausted.yml：重啟額度用完就放棄（結束碼 1）

```text
worker: daemon waiting 200ms before restart (attempt 1)
sink:   sink 通知：上游 worker 已重啟（第 1 次），已收 5 筆
worker: Error: worker 第 1 代在 seq=12 模擬當機
worker: daemon max restarts (1) exceeded, giving up
sink:   sink 通知：上游 worker 失敗，影響輸入 [DataId("result")]：exited with code 1 with stderr output: ...
sink:   sink 輸入 result 已關閉
sink:   sink 總結：收到 6 筆 [1, 2, 3, 4, 6, 11]；NodeRestarted 1 次、NodeFailed 1 次
[ERROR] Dataflow failed: Node `worker` failed: exited with code 1 ...
```

這次 sink 先收到 `NodeFailed`（帶著 worker 的 stderr 全文），再收到 `InputClosed`。

## 設計建議

- **壞資料不要讓節點死**：一筆錯就 `continue`，並計數或送出一個 `error` 輸出給監控節點。
- **該死就死得乾淨**：致命錯誤用 `?`／`bail!` 加 `.context()` 回傳，讓 dora 決定是否重啟，不要自己在節點裡寫無限重試迴圈。
- **一定要設 `max_restarts`**（或搭配 `restart_window`），避免一直當掉一直重啟，把 CPU 和日誌塞滿。
- **重啟會丟資料**：對「不能漏」的資料要在應用層設計序號與重送。
- **下游同時處理** `NodeRestarted`、`NodeFailed`、`InputClosed`，並保留 `_ => {}`。

## 小練習

1. 把 `on-failure.yml` 的 `restart_delay` 改成 `0.05`，再跑一次，比較 sink 收到的序號：空窗縮短後，丟掉的資料變少了嗎？
2. 把 worker 改成 `restart_policy: always`、`max_restarts: 2`，並讓 worker 收到 seq=3 就**正常** `return Ok(())`。預測：它會重啟幾次？sink 收到哪些序號？跑跑看驗證你的預測。
3. 進階：讓 worker 在重生時（`restart_count() > 0`）透過新輸出 `restarted` 送出自己的 `restart_count`，在 sink 印出來，跟 `NodeRestarted` 的通知對照。

下一篇：第 10 篇**尚未發布**（僅規劃；零拷貝大資料與 benchmark 等見 [課程大綱](../../README.md#課程大綱20-篇規劃)）。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 8 篇](lesson-08.md)
- 下一課：第 10 篇**尚未發布**（僅規劃；零拷貝大資料等見 [課程大綱](../../README.md#課程大綱20-篇規劃)）
- 本課範例：[examples/lesson-09/](../../examples/lesson-09/)
