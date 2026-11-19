# dora-rs 教學 第 11 篇：Operator vs Node——同一段邏輯，兩種包法

> 版本：dora v1.1.0（CLI、`dora-node-api`、`dora-operator-api` 都是 1.1.0）　驗證日期：2026-11-11（台北時間），只在這台 x86_64 Linux 虛擬機上實測。4 份 YAML 的 `dora validate` 都通過；`operator.yml`、`node.yml`、`mixed.yml` 用 `dora run --stop-after 3s` 跑，結束碼都是 0，接收端都收到 25 筆、自我檢查 0 筆錯誤；`multi-op.yml`（一個節點放兩個 operator）**刻意示範失敗**，結束碼 1。第一次完整 release 建置約 2 分 44 秒。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 10 篇](lesson-10.md)
- 下一課：[第 12 篇 →](lesson-12.md)
- 本課範例：[examples/lesson-11/](../../examples/lesson-11/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [小練習](#小練習)
## 重點說明

到目前為止我們寫的都是 **Node**：一支有 `main()` 的獨立執行檔，自己呼叫 `DoraNode::init_from_env()`、自己跑 `while let Some(event) = events.recv()` 事件迴圈。dora 還有另一種寫法叫 **Operator**：你只寫一個「收到事件時要做什麼」的回呼函式，編成共享函式庫（`.so`），由 dora 的 runtime 載入並幫你跑事件迴圈。

### 1. 兩者差在哪

| | Node | Operator |
|---|---|---|
| 產物 | 執行檔（`[[bin]]`） | 共享函式庫（`crate-type = ["cdylib"]`，Linux 上是 `libxxx.so`） |
| 誰跑事件迴圈 | 你自己：`events.recv()` | dora runtime：呼叫你的 `on_event()` |
| 進入點 | `main()` | `register_operator!(型別)` ＋ 實作 `DoraOperator` trait |
| 狀態 | 一般區域變數 | 放在 struct 欄位，runtime 用 `Default` 建立（不能帶參數） |
| 行程 | 一個 node 一個行程 | 由 `dora runtime` 行程載入（本篇實測：一個 runtime 節點只能放一個 operator） |
| API crate | `dora-node-api` | `dora-operator-api` |
| 自由度 | 高：可以開執行緒、用 async、自己決定何時結束、`send_output_raw` 零拷貝寫入 | 低：只能在 `on_event` 裡回應；送出只有 `send(id, impl IntoArrow)` |
| YAML | `path:` 指向執行檔 | `operators:` 清單裡寫 `shared-library:` |
| 輸出位址 | `節點/輸出`，例：`acc/total` | `節點/operator/輸出`，例：`acc-rt/acc/total` |

### 2. Operator 的骨架

```rust
register_operator!(Double);          // 產生 runtime 要找的 C 介面符號

#[derive(Default)]
struct Double { count: i64 }

impl DoraOperator for Double {
    fn on_event(&mut self, event: &Event, out: &mut DoraOutputSender)
        -> Result<DoraStatus, String>
    {
        match event {
            Event::Input { id, data, .. } => { /* 處理後 out.send("value", x.into_arrow())? */ }
            Event::Stop => return Ok(DoraStatus::Stop),
            _ => {}                    // Event 是 #[non_exhaustive]，一定要有萬用分支
        }
        Ok(DoraStatus::Continue)
    }
}
```

- 回傳 `Continue` 繼續、`Stop` 停掉這個 operator（其他節點照跑）。原始碼註解說 `StopAll`（停掉整個 dataflow）**尚未實作**。
- 回傳 `Err(String)`：回報致命錯誤並停止這個 operator。
- 事件種類：`Input`、`InputParseError`、`InputClosed`、`Stop`、`Error`。注意 operator 版 `Event::Input` 在 1.1.0 有 `id`、`metadata`、`data` 三個欄位，但 repo 裡 `docs/api-rust.md` 的範例只寫了 `{ id, data }`，照抄會編譯失敗，用 `{ id, data, .. }` 最保險。
- `dora-operator-api` 1.1.0 原始碼的單元測試顯示：`on_event` 或 `Default` 裡 panic 會被攔下來、轉成錯誤，不會讓整個行程直接 abort（我沒有在實際 dataflow 裡測這件事，留作練習）。

### 3. 本篇實測發現：v1.1.0 一個 runtime 節點只能放一個 operator

官方 guide（`guide/src/concepts/dataflow-yaml.md`）有一個「一個節點底下放兩個 operator」的 YAML 範例，看起來像是可以把多個 operator 放進同一個行程省資源。但在 v1.1.0 實際跑，runtime 直接拒絕：

```
runtime-node: daemon exited with code 1 with stderr output:
    multiple operators are not supported
```

原始碼 `binaries/runtime-api/src/lib.rs` 也寫死了：

```rust
} else if operators.len() > 1 {
    bail!("multiple operators are not supported");
```

而且 `dora validate` 對這份 YAML 回報 OK，要到 `dora run` 才失敗。所以在 v1.1.0：**一個 operator 就要一個 runtime 節點，等於一個行程**——「多個 operator 共用一個行程」這個傳統上 operator 的主要好處，目前用不到。

### 4. 實測的行程與記憶體

跑的時候用 `ps` 抓的快照（RSS = 實際佔用的實體記憶體，單一次量測，僅供參考）：

| dataflow | 行程 | RSS |
|---|---|---|
| `operator.yml` | `dora runtime` ×2（各載入一個 `.so`） | 29640 KB、29860 KB |
|  | `sink`（node） | 17576 KB |
| `node.yml` | `double-node`、`acc-node`、`sink` | 17584 KB、17644 KB、17712 KB |

在這台機器上，每個 operator 行程反而比同功能的 node 多約 12 MB，因為它是整支 `dora` CLI 執行檔以 `runtime` 子命令啟動。另外兩個 runtime 行程啟動時都印了一行警告：

```
WARN dora_node_api::node: failed to create zenoh SHM provider (Unable to create POSIX shm segment: OS error 12 ...); falling back to heap-buffered publishes
```

意思是建立共享記憶體區段失敗，改用一般記憶體傳送。三個 node 行程沒有這個警告。原因**我還沒查清楚**；本篇每筆資料只有 8 位元組，本來就低於第 10 篇講的 4096 位元組零拷貝門檻，所以結果不受影響，但如果你要用 operator 送大影格，要先確認這件事。

### 5. 那什麼時候用哪個？（以 v1.1.0 的實況）

- **預設用 Node**：除錯方便（可以直接在節點裡加 log、用一般除錯器）、能用 async／多執行緒、能用 `send_output_raw` 零拷貝寫入，第 15 篇交叉編譯到 ZYNQ 也比較單純（只要一支執行檔）。
- **Operator 適合**：純粹「收一筆→算一下→送一筆」的小元件、想讓框架管生命週期、或要寫 Python operator（第 12 篇）時。
- 兩者可以在同一個 dataflow **混接**（本篇 `mixed.yml`）。

## 完整範例

位置：[examples/lesson-11/](../../examples/lesson-11/)。同一段邏輯：計時器每 100 ms 一個 tick → `double` 把計數乘 2 → `acc` 累加 → `sink` 檢查第 n 筆的累計和是否等於 2+4+…+2n = n(n+1)。

```
examples/lesson-11/
├── Cargo.toml            # workspace
├── op-double/            # operator（cdylib）
├── op-acc/               # operator（cdylib）
├── nodes/src/bin/        # double-node、acc-node、sink（node）
├── operator.yml          # double、acc 都是 operator
├── node.yml              # 全部是 node
├── mixed.yml             # double 是 operator、acc 是 node
└── multi-op.yml          # 一個節點放兩個 operator（v1.1.0 會失敗）
```

### Cargo.toml（workspace）

```toml
[workspace]
resolver = "3"
members = ["op-double", "op-acc", "nodes"]

[workspace.dependencies]
dora-operator-api = "1.1.0"
dora-node-api = "1.1.0"
eyre = "0.6"
```

### op-double/Cargo.toml（op-acc 相同，只改 name）

```toml
[package]
name = "op-double"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
dora-operator-api = { workspace = true }
```

### op-double/src/lib.rs

```rust
//! Operator：每收到一個 tick，計數器 +1，送出「計數 × 2」。
use dora_operator_api::{
    DoraOperator, DoraOutputSender, DoraStatus, Event, IntoArrow, register_operator,
};

register_operator!(Double);

/// 狀態放在 struct 裡；runtime 用 Default 建立它（不能有參數）。
#[derive(Default)]
struct Double {
    count: i64,
    said_hello: bool,
}

impl DoraOperator for Double {
    fn on_event(
        &mut self,
        event: &Event,
        out: &mut DoraOutputSender,
    ) -> Result<DoraStatus, String> {
        match event {
            Event::Input { id, .. } if *id == "tick" => {
                if !self.said_hello {
                    self.said_hello = true;
                    println!(
                        "[op double] pid={} thread={:?}",
                        std::process::id(),
                        std::thread::current().id()
                    );
                }
                self.count += 1;
                out.send("value", (self.count * 2).into_arrow())?;
                Ok(DoraStatus::Continue)
            }
            Event::InputParseError { id, error } => Err(format!("{id}: {error}")),
            Event::Stop => Ok(DoraStatus::Stop),
            _ => Ok(DoraStatus::Continue),
        }
    }
}
```

### op-acc/src/lib.rs

```rust
//! Operator：把收到的 i64 累加，送出累計和。
use dora_operator_api::{
    DoraOperator, DoraOutputSender, DoraStatus, Event, IntoArrow, register_operator,
};

register_operator!(Acc);

#[derive(Default)]
struct Acc {
    total: i64,
    said_hello: bool,
}

impl DoraOperator for Acc {
    fn on_event(
        &mut self,
        event: &Event,
        out: &mut DoraOutputSender,
    ) -> Result<DoraStatus, String> {
        match event {
            Event::Input { id, data, .. } if *id == "value" => {
                if !self.said_hello {
                    self.said_hello = true;
                    println!(
                        "[op acc] pid={} thread={:?}",
                        std::process::id(),
                        std::thread::current().id()
                    );
                }
                // DoraArray → i64：用 TryFrom，型別不符就回傳錯誤字串
                let v: i64 = data
                    .try_into()
                    .map_err(|e| format!("value 不是 i64: {e}"))?;
                self.total += v;
                out.send("total", self.total.into_arrow())?;
                Ok(DoraStatus::Continue)
            }
            Event::InputClosed { id } if *id == "value" => Ok(DoraStatus::Stop),
            Event::Stop => Ok(DoraStatus::Stop),
            _ => Ok(DoraStatus::Continue),
        }
    }
}
```

### nodes/Cargo.toml

```toml
[package]
name = "nodes"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = { workspace = true }
eyre = { workspace = true }
```

### nodes/src/bin/double-node.rs（同邏輯的 Node 版）

```rust
//! 與 op-double 相同邏輯，但寫成獨立行程的 Node。
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("[node double] pid={}", std::process::id());
    let mut count: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                count += 1;
                node.send_output("value".into(), MetadataParameters::default(), count * 2)?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
```

### nodes/src/bin/acc-node.rs

```rust
//! 與 op-acc 相同邏輯，寫成 Node。
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("[node acc] pid={}", std::process::id());
    let mut total: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "value" => {
                total += i64::try_from(&data)?;
                node.send_output("total".into(), MetadataParameters::default(), total)?;
            }
            Event::InputClosed { id } if id.as_str() == "value" => break,
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
```

### nodes/src/bin/sink.rs（三份 dataflow 共用）

```rust
//! 共用的接收端（一定是 Node）：印出累計和，結束時自我檢查。
//! 第 n 筆 total 應該等於 2+4+…+2n = n(n+1)。
use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    println!("[sink] pid={}", std::process::id());
    let mut n: i64 = 0;
    let mut bad = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "total" => {
                n += 1;
                let total = i64::try_from(&data)?;
                if total != n * (n + 1) {
                    bad += 1;
                }
                if n <= 3 || n % 10 == 0 {
                    println!("[sink] 第 {n} 筆 total={total}");
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[sink] 總結：收到 {n} 筆，不符 n(n+1) 的 {bad} 筆");
    Ok(())
}
```

### operator.yml

```yaml
nodes:
  # dora v1.1.0：一個 runtime 節點只能放「一個」operator
  - id: double-rt
    operators:
      - id: double
        build: cargo build --release -p op-double
        shared-library: target/release/op_double   # 會自動補成 libop_double.so
        inputs:
          tick: dora/timer/millis/100
        outputs:
          - value

  - id: acc-rt
    operators:
      - id: acc
        build: cargo build --release -p op-acc
        shared-library: target/release/op_acc
        inputs:
          value: double-rt/double/value   # 三段式：節點/operator/輸出
        outputs:
          - total

  - id: sink
    build: cargo build --release -p nodes --bin sink
    path: target/release/sink
    inputs:
      total: acc-rt/acc/total
```

### node.yml

```yaml
nodes:
  - id: double
    build: cargo build --release -p nodes --bin double-node
    path: target/release/double-node
    inputs:
      tick: dora/timer/millis/100
    outputs:
      - value

  - id: acc
    build: cargo build --release -p nodes --bin acc-node
    path: target/release/acc-node
    inputs:
      value: double/value
    outputs:
      - total

  - id: sink
    build: cargo build --release -p nodes --bin sink
    path: target/release/sink
    inputs:
      total: acc/total
```

### mixed.yml（operator → node → node）

```yaml
nodes:
  - id: double-rt
    operators:
      - id: double
        build: cargo build --release -p op-double
        shared-library: target/release/op_double
        inputs:
          tick: dora/timer/millis/100
        outputs:
          - value

  - id: acc
    build: cargo build --release -p nodes --bin acc-node
    path: target/release/acc-node
    inputs:
      value: double-rt/double/value
    outputs:
      - total

  - id: sink
    build: cargo build --release -p nodes --bin sink
    path: target/release/sink
    inputs:
      total: acc/total
```

### multi-op.yml（反例：v1.1.0 會失敗）

```yaml
nodes:
  - id: runtime-node
    operators:
      - id: double
        build: cargo build --release -p op-double
        shared-library: target/release/op_double
        inputs:
          tick: dora/timer/millis/100
        outputs:
          - value
      - id: acc
        build: cargo build --release -p op-acc
        shared-library: target/release/op_acc
        inputs:
          value: runtime-node/double/value
        outputs:
          - total

  - id: sink
    build: cargo build --release -p nodes --bin sink
    path: target/release/sink
    inputs:
      total: runtime-node/acc/total
```

### 執行

```bash
cd examples/lesson-11   # 自 repo 根目錄
cargo build --release                       # 先整個 workspace 建一次
dora validate operator.yml                  # 4 份都會顯示 OK（包括 multi-op.yml）
dora run operator.yml --stop-after 3s; echo "exit=$?"
dora run node.yml     --stop-after 3s; echo "exit=$?"
dora run mixed.yml    --stop-after 3s; echo "exit=$?"
dora run multi-op.yml --stop-after 3s; echo "exit=$?"   # 預期失敗
```

> 小提醒：我先用 `cargo build --release` 建完整個 workspace，第一次 `dora run` 時各節點的 `cargo build -p ...` 又重編了一部分依賴（`dora-node-api`、`zenoh` 等），多花約 2 分鐘，第二次之後就是零點幾秒。我推測是單獨建一個套件時，依賴的 feature 組合和整個 workspace 一起建時不同，這點沒有深入確認。

### 實測輸出（節錄）

`operator.yml`（結束碼 0）：

```
[sink] pid=1713266
[op double] pid=1713265 thread=ThreadId(1)
[op acc] pid=1713264 thread=ThreadId(1)
[sink] 第 1 筆 total=2
[sink] 第 2 筆 total=6
[sink] 第 3 筆 total=12
[sink] 第 10 筆 total=110
[sink] 第 20 筆 total=420
[sink] 總結：收到 25 筆，不符 n(n+1) 的 0 筆
```

`node.yml`（結束碼 0）：

```
[node acc] pid=1713484
[sink] pid=1713496
[node double] pid=1713485
...
[sink] 總結：收到 25 筆，不符 n(n+1) 的 0 筆
```

`mixed.yml`（結束碼 0）：`[op double]`、`[node acc]`、`[sink]` 三個不同 pid，同樣收到 25 筆、0 筆不符。

`multi-op.yml`（結束碼 1）：

```
runtime-node: daemon exited with code 1 with stderr output:
    multiple operators are not supported
sink: daemon exited with code 1. This error occurred because node `runtime-node` exited before connecting to dora.
Dataflow failed
```

觀察重點：

1. 三種寫法的結果完全相同（3 秒內約 25 個 tick），邏輯可以在 operator 和 node 之間直接搬。
2. `operator.yml` 裡兩個 operator 的 pid 不同，證實每個 runtime 節點是獨立行程；`ps` 看到的是 `.../bin/dora runtime`。
3. operator 程式碼裡沒有 `main`、沒有 `init_from_env`、沒有事件迴圈，Node 版反而更短（double：operator 40 行、node 19 行，因為 operator 要寫 trait 樣板）。

## 小練習

在 `op-acc` 裡加一條規則：`total` 超過 1000 時回傳 `Err("total 超過上限".into())`。然後：

1. 跑 `dora run operator.yml --stop-after 5s`，記下 `sink` 最後收到幾筆、`dora run` 的結束碼。
2. 把同樣規則加到 `acc-node`（用 `eyre::bail!`），跑 `node.yml` 比較：兩種寫法下錯誤訊息出現在哪裡、結束碼是否一樣？
3. 進階：把 `Err` 改成 `panic!("boom")`，看 operator 版是被攔下來轉成錯誤，還是整個行程崩潰（這就是本篇第 2 節「沒有在實際 dataflow 裡測」的那件事）。

提示：n(n+1) > 1000 時 n = 32，所以 3 秒的 run 碰不到上限，要把 `--stop-after` 拉長或把計時器改快。

下一篇：[第 12 篇](lesson-12.md)：混用 Python 節點——Rust 與 Python 節點在同一個 dataflow 裡互傳 Arrow 資料。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 10 篇](lesson-10.md)
- 下一課：[第 12 篇 →](lesson-12.md)
- 本課範例：[examples/lesson-11/](../../examples/lesson-11/)
