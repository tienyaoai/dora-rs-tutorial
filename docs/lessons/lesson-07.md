# dora-rs 教學 第 7 篇：多節點管線——扇入、扇出與 YAML 連線

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-07（台北時間），x86_64 Linux 上實際 `dora validate` / `dora build` / `dora run` 成功（結束碼 0）。範例拓撲 `ticker → multiply → accumulate → printer`（另有 `ticker → printer` 扇出），最終累計 `total=210`（= 10×(1+…+6)）。另刻意改錯 YAML 驗證 `dora validate` 能抓到不存在的輸出名與節點 id（結束碼 1）。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 6 篇](lesson-06.md)
- 下一課：[第 8 篇 →](lesson-08.md)
- 本課範例：[examples/lesson-07/](../../examples/lesson-07/)

## 目錄

- [重點說明](#重點說明)
- [完整可執行範例：ticker → multiply → accumulate → printer](#完整可執行範例ticker-multiply-accumulate-printer)
- [怎麼跑](#怎麼跑)
- [小練習](#小練習)
## 重點說明

第 4 篇已出現三節點鏈（`sensor → stats → printer`），第 6 篇則是長跑的兩節點。本篇把**多節點管線**講完整：資料如何一站接一站、一個輸出如何同時餵給多個下游（**扇出**）、一個節點如何同時收多個上游（**扇入**），以及這些連線**全部寫在 YAML**，程式裡不要 hardcode 鄰居。

### 1. 管線長什麼樣子

本篇範例的拓撲：

```
ticker ──┬──→ multiply ──→ accumulate ──→ printer（輸入 total）
         │                                  ↑
         └──────────────────────────────────┘
                    （輸入 raw：扇出）
```

**`ticker`（來源）**

- 行為：送 `1..=6` 後自行結束
- 輸入：（無）
- 輸出：`count`

**`multiply`（轉換：×10）**

- 輸入：`value ← ticker/count`
- 輸出：`scaled`

**`accumulate`（轉換：累加）**

- 輸入：`scaled ← multiply/scaled`
- 輸出：`total`

**`printer`（匯流端，扇入）**

- 輸入：`raw ← ticker/count`、`total ← accumulate/total`
- 輸出：（無）

重點有三：

1. **鏈式管線**：`ticker → multiply → accumulate → printer`，每站只做一件事。
2. **扇出（fan-out）**：同一個 `ticker/count` 同時被 `multiply` 與 `printer` 訂閱。
3. **扇入（fan-in）**：`printer` 同時收 `raw` 與 `total` 兩條輸入。

### 2. YAML 連線語法（一定要背）

```yaml
inputs:
  本地輸入名: 上游節點id/上游輸出名
```

例如：

```yaml
inputs:
  value: ticker/count          # multiply 的本地名是 value
  total: accumulate/total      # printer 的本地名是 total
  raw: ticker/count            # 同一個上游輸出，可被多個下游訂閱
```

規則整理：

- **左邊：** **本節點**在程式裡用的輸入 id（`Event::Input { id, .. }` 比對的就是它）
- **右邊：** `上游節點的 id` + `/` + `上游在 outputs 宣告過的名字`
- **扇出：** 同一個 `上游id/輸出名` 可以出現在多個下游的 `inputs`
- **扇入：** 同一個節點的 `inputs` 可以寫多行，每行一個輸入
- **計時器：** 特殊來源 `dora/timer/millis/N`（本篇沒用；見第 4、6 篇）

程式裡**不要**寫死「我的鄰居叫 multiply」——鄰居是誰，只由 YAML 決定。這樣換成別的上游（例如真實感測器）時，節點原始碼不用改。

### 3. 節點 id / inputs / outputs 三件套

每個節點在 YAML 至少要有：

- **`id`**：資料流內唯一。連線右邊的「上游節點 id」就是它。
- **`outputs`**（有送資料才需要）：字串清單。沒宣告的輸出，`send_output` 會被**默默忽略**（回 `Ok`），下游永遠收不到——這是超常見的除錯坑。
- **`inputs`**（有收資料才需要）：`本地名: 上游id/輸出名` 對應表。
- **`path` / `build`**：可執行檔路徑，以及（可選）建置指令。`dora build` / `dora run` 會跑 `build:`。

### 4. 扇入時怎麼結束？`InputClosed` 很重要

上游節點結束後，dora 會對每個下游送 `Event::InputClosed { id }`（`id` 是**下游本地輸入名**）。

- **只有一個資料輸入**的節點（如本篇的 `multiply`、`accumulate`）：收到該輸入的 `InputClosed` 就可以 `break`。
- **扇入（多個資料輸入）**的節點（如本篇的 `printer`）：要等**所有**資料輸入都 `InputClosed`（或收到 `Stop`）再退出。範例用 `open_inputs` 計數器示範。
- 若還掛著 `dora/timer/...`，計時器**永遠不關**，就不能指望 `AllInputsClosed`；必須自己決定何時 `break`（第 4 篇的 `stats`），或靠 `dora stop`（第 6 篇）。

本篇刻意讓 `ticker` 送完 6 筆就結束（不掛計時器），整條管線會自動收工，方便 `dora run` 一次看完。

### 5. `dora validate` 怎麼抓連錯的 id

`dora validate dataflow.yml` 會做配線檢查。本篇實測兩種常見打字錯誤：

**輸出名打錯：** `value: ticker/WRONG`

- validate 訊息（摘要）：`output ticker/WRONG mapped to input multiply/value does not exist`
- 結束碼：1

**節點 id 打錯：** `raw: tickerr/count`

- validate 訊息（摘要）：`source node tickerr mapped to input printer/raw does not exist`
- 結束碼：1

養成習慣：**改完 YAML 先 validate，再 build / run**。比跑起來才發現下游沒資料快得多。

### 6. 常見地雷

1. **輸出沒在 YAML `outputs:` 宣告** → `send_output` 成功但下游收不到。
2. **本地輸入名 vs 上游輸出名搞混** → 程式 `match id` 要比對的是**左邊本地名**（例如 `value`），不是 `count`。
3. **扇入只等其中一條關閉就退出** → 另一條還在送，資料會丟；或反過來永遠等不到 `AllInputsClosed`。
4. **在程式裡 hardcode 鄰居節點名** → 換配線就要改碼；正確做法是只認本地輸入 id。
5. **以為扇出要在上游 `send` 兩次** → 不用。上游只 `send_output` 一次，YAML 裡誰訂閱誰就會收到同一份（語意上；實作可能複製或共享，細節見之後篇章）。
6. **改錯 id 不跑 validate** → 直接 `run` 才失敗，訊息較長；先 validate 最省時間。

## 完整可執行範例：ticker → multiply → accumulate → printer

```
examples/lesson-07/
├── Cargo.toml
├── dataflow.yml
└── src/bin/
    ├── ticker.rs
    ├── multiply.rs
    ├── accumulate.rs
    └── printer.rs
```

`Cargo.toml`：

```toml
[package]
name = "lesson-07"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

`dataflow.yml`：

```yaml
nodes:
  - id: ticker
    description: 資料來源：送出 1..=6 的 u64 計數後自行結束（扇出給 multiply 與 printer）
    build: cargo build --bin ticker
    path: target/debug/ticker
    outputs:
      - count

  - id: multiply
    description: 中間節點：把 count 乘以 10
    build: cargo build --bin multiply
    path: target/debug/multiply
    inputs:
      value: ticker/count
    outputs:
      - scaled

  - id: accumulate
    description: 中間節點：對 scaled 做累加，每次送出目前總和
    build: cargo build --bin accumulate
    path: target/debug/accumulate
    inputs:
      scaled: multiply/scaled
    outputs:
      - total

  - id: printer
    description: 匯流端：扇入 raw（直接來自 ticker）與 total（管線末端）
    build: cargo build --bin printer
    path: target/debug/printer
    inputs:
      raw: ticker/count
      total: accumulate/total
```

`src/bin/ticker.rs`：

```rust
use dora_node_api::{DoraNode, MetadataParameters};
use std::{thread::sleep, time::Duration};

fn main() -> eyre::Result<()> {
    // 沒有 inputs 的純來源節點也要 init；events 用不到但要留著
    let (mut node, _events) = DoraNode::init_from_env()?;

    for n in 1..=6u64 {
        println!("ticker 送出 count={n}");
        node.send_output("count".into(), MetadataParameters::default(), n)?;
        sleep(Duration::from_millis(150));
    }
    println!("ticker 結束（共送出 6 次）");
    Ok(())
}
```

`src/bin/multiply.rs`：

```rust
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "value" => {
                let n = u64::try_from(&data)?;
                let scaled = n * 10;
                println!("multiply 收到 value={n} → 送出 scaled={scaled}");
                node.send_output("scaled".into(), MetadataParameters::default(), scaled)?;
            }
            Event::InputClosed { id } => {
                println!("multiply 輸入 {id} 已關閉，準備退出");
                break;
            }
            Event::Stop(cause) => {
                println!("multiply 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("multiply 其他事件：{other:?}"),
        }
    }
    println!("multiply 結束");
    Ok(())
}
```

`src/bin/accumulate.rs`：

```rust
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut sum: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "scaled" => {
                let v = u64::try_from(&data)?;
                sum += v;
                println!("accumulate 收到 scaled={v} → 累計 total={sum}");
                node.send_output("total".into(), MetadataParameters::default(), sum)?;
            }
            Event::InputClosed { id } => {
                println!("accumulate 輸入 {id} 已關閉（最終 total={sum}），準備退出");
                break;
            }
            Event::Stop(cause) => {
                println!("accumulate 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("accumulate 其他事件：{other:?}"),
        }
    }
    println!("accumulate 結束（最終 total={sum}）");
    Ok(())
}
```

`src/bin/printer.rs`：

```rust
use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    // 兩個輸入都關閉後才收工（扇入的典型結束條件）
    let mut open_inputs = 2usize;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } => match id.as_str() {
                "raw" => {
                    let n = u64::try_from(&data)?;
                    println!("printer [raw]   直接來自 ticker：{n}");
                }
                "total" => {
                    let n = u64::try_from(&data)?;
                    println!("printer [total] 管線累計結果：{n}");
                }
                other => println!("printer 未知輸入 {other}"),
            },
            Event::InputClosed { id } => {
                println!("printer 輸入 {id} 已關閉");
                open_inputs = open_inputs.saturating_sub(1);
                if open_inputs == 0 {
                    println!("printer 所有輸入都關閉了，準備退出");
                    break;
                }
            }
            Event::Stop(cause) => {
                println!("printer 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("printer 其他事件：{other:?}"),
        }
    }
    println!("printer 結束");
    Ok(())
}
```

## 怎麼跑

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄（依你的安裝方式調整），例如：
# export PATH="$HOME/dora/bin:$PATH"
cd examples/lesson-07   # 在 clone 本 repo 後，於 repo 根目錄執行

dora validate dataflow.yml
dora build dataflow.yml
dora run dataflow.yml
```

本篇節點會自行結束（ticker 送完 6 筆 → 一路 `InputClosed` 傳下去），**不需要** `timeout` 或 `--stop-after`。若你改成掛計時器的長跑版，請用 `timeout 8s dora run dataflow.yml` 或第 6 篇的 `dora up` / `start` / `stop`。

**實測輸出（2026-11-07 08:10 台北時間，`dora run` 結束碼 0；略去 dora INFO/DEBUG 行）**：

```text
ticker 送出 count=1
printer [raw]   直接來自 ticker：1
multiply 收到 value=1 → 送出 scaled=10
accumulate 收到 scaled=10 → 累計 total=10
printer [total] 管線累計結果：10
ticker 送出 count=2
multiply 收到 value=2 → 送出 scaled=20
printer [raw]   直接來自 ticker：2
accumulate 收到 scaled=20 → 累計 total=30
printer [total] 管線累計結果：30
…（3～5 同上，total 依序 60、100、150）
ticker 送出 count=6
printer [raw]   直接來自 ticker：6
multiply 收到 value=6 → 送出 scaled=60
accumulate 收到 scaled=60 → 累計 total=210
printer [total] 管線累計結果：210
ticker 結束（共送出 6 次）
printer 輸入 raw 已關閉
multiply 輸入 value 已關閉，準備退出
multiply 結束
accumulate 輸入 scaled 已關閉（最終 total=210），準備退出
accumulate 結束（最終 total=210）
printer 輸入 total 已關閉
printer 所有輸入都關閉了，準備退出
printer 結束
daemon ticker / multiply / accumulate / printer finished successfully
daemon dataflow finished
```

驗算：`10+20+30+40+50+60 = 210`。`raw` 與 `total` 交錯出現是正常的——兩條路徑長度不同，dora **不保證**跨輸入的全域順序；同一條輸入內的順序則會保留。

**validate 抓錯連線（實測）**：

```bash
# 輸出名打錯
# inputs: value: ticker/WRONG
# → output `ticker/WRONG` mapped to input `multiply/value` does not exist（結束碼 1）

# 節點 id 打錯
# inputs: raw: tickerr/count
# → source node `tickerr` mapped to input `printer/raw` does not exist（結束碼 1）
```

> 編譯備註：首次 `dora build` 約 1 分半（含下載／編譯依賴）；實驗完建議 `cargo clean` 釋放 `target/` 空間。完整執行 log **未**納入本 repo。

## 小練習

1. 把 `multiply` 的倍率從 `10` 改成 `2`，重跑。最終 `total` 應該是多少？（提示：`2×(1+…+6)`）
2. 在 YAML 拿掉 `printer` 的 `raw: ticker/count` 這一行，並把 `printer.rs` 的 `open_inputs` 改成 `1`。重跑後還看得到 `[raw]` 嗎？這示範了「配線決定拓撲，不是程式硬寫」。
3. 進階（扇入再加一站）：新增節點 `fuse`，同時訂閱 `multiply/scaled` 與另一個新來源 `offset`（固定送一次 `u64: 1000` 後結束）。`fuse` 把兩邊加總後再交給 `accumulate`。注意 `offset` 只送一次就結束時，`fuse` 要怎麼處理兩條 `InputClosed`？

下一篇：[第 8 篇：計時器頻率與背壓——上游太快、下游太慢時，佇列怎麼表現](lesson-08.md)。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 6 篇](lesson-06.md)
- 下一課：[第 8 篇 →](lesson-08.md)
- 本課範例：[examples/lesson-07/](../../examples/lesson-07/)
