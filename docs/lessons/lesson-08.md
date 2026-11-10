# dora-rs 教學 第 8 篇：計時器頻率與背壓——上游太快、下游太慢時，佇列怎麼表現

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-08（台北時間），僅在 x86_64 Linux 上實測。7 份 YAML 的 `dora validate` 全部結束碼 0，`dora build` 成功；6 份資料流以 `dora run` 自然結束（結束碼 0），另 1 份「沒開 `exit_when_nodes_finish`」的對照組用 `timeout 15s` 停下（結束碼 124，符合預期）。同一組 producer（每 50ms 送一個序號，共 40 筆）＋ consumer（每筆處理 200ms）：`queue_size: 1` 收到 11/40、預設佇列收到 20/40、`backpressure` 收到 40/40；頻率對齊（4 Hz）時也是 40/40。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 7 篇](lesson-07.md)
- 下一課：[第 9 篇 →](lesson-09.md)
- 本課範例：[examples/lesson-08/](../../examples/lesson-08/)

## 目錄

- [重點說明](#重點說明)
- [完整可執行範例：快 producer × 慢 consumer，七種配線](#完整可執行範例快-producer-慢-consumer七種配線)
- [怎麼跑](#怎麼跑)
- [小練習](#小練習)
## 重點說明

第 4、6 篇用過 `dora/timer/millis/N`，第 7 篇讓資料一站接一站流動。本篇回答兩個實務問題：

1. **計時器到底怎麼跳？**——誰產生、有哪些單位、你的程式慢了它會不會等你。
2. **上游太快、下游太慢時，多出來的訊息去哪了？**——`queue_size` 與 `queue_policy`（`drop_oldest` / `backpressure`）怎麼決定「丟」還是「排隊」。

我們用一個很好量測的設計：producer 每個 tick 送一個遞增的 `u64` 序號，consumer 把收到的序號記下來，最後列出**缺口**。有缺口＝有訊息被丟掉，一目了然。

### 1. 計時器語法：時間由 dora 產生，不是你的程式

計時器是 YAML 裡的「虛擬節點」，寫在 `inputs` 的右邊：

```yaml
inputs:
  tick: dora/timer/millis/50   # 每 50 毫秒
  slow: dora/timer/secs/1      # 每 1 秒
  fast: dora/timer/hz/4        # 每秒 4 次 = 每 250 毫秒
```

| 單位 | 寫法範例 | 值的規則（依 v1.1.0 原始碼的解析器） |
|---|---|---|
| `secs` | `dora/timer/secs/1` | 整數秒 |
| `millis` | `dora/timer/millis/50` | 整數毫秒（最常用） |
| `hz` | `dora/timer/hz/4`、`dora/timer/hz/0.5` | 正的有限數字，可以有小數；間隔 = 1/hz 秒 |
| `micros`、`nanos` | `dora/timer/micros/500` | 整數；原始碼支援，但官方 YAML 規格表只舉 `millis`、`hz` |

另外，`millis/0`、`secs/0` 這類「零間隔」在載入時就會被拒絕。

幾個觀念一定要建立：

- **tick 是 dora daemon 產生的**，用 `tokio` 的 interval 計時，再當成一般的 `Event::Input` 送進你的節點。你的程式只負責「收到 tick 要做什麼」。
- **計時器不會等你的事件迴圈。** daemon 照表操課；你的節點處理得慢，tick 就會在你的輸入佇列裡堆起來（計時器輸入同樣受 `queue_size` 限制，預設 10、`drop_oldest`）。daemon 的計時器設定為 `MissedTickBehavior::Skip`，錯過的 tick 會被跳過，不會事後補發。
- **第一個 tick 幾乎立刻就來。** 實測 producer 的第一筆在 `t=0ms` 就送出，之後大約每 50ms 一筆，40 筆總共約 1950ms（≈ 39 個間隔）。
- **計時器永遠不會關閉**（第 4、6 篇講過）。它沒有上游節點，所以不會有 `InputClosed`。只吃計時器的來源節點必須**自己決定何時 `break`**，本篇的 producer 就是送滿 40 筆後自行結束。

### 2. 頻率算術：先算，再設定

本篇的主角組合：

| 角色 | 速度 | 每秒 |
|---|---|---|
| producer | 每 50ms 一筆（`dora/timer/millis/50`） | 20 筆 |
| consumer | 每筆處理 200ms（`sleep` 模擬） | 5 筆 |

上游比下游快 **4 倍**。producer 約 2 秒就送完 40 筆，這段期間 consumer 只處理得完約 10 筆，剩下約 30 筆一定要有個去處：**丟掉**，或**排隊**。這就是佇列策略要決定的事。

> 經驗法則：**計時器間隔 ≥ 下游每筆處理時間**，就不會累積。本篇 matched 組用 `hz/4`（250ms）對上 200ms 的處理時間，40 筆全收。

### 3. 長寫法 input：`queue_size` 與 `queue_policy`

```yaml
inputs:
  data: producer/seq                # 簡寫：等同 queue_size 10、drop_oldest

  data:                             # 長寫法（兩者擇一）
    source: producer/seq
    queue_size: 1
    queue_policy: drop_oldest       # 或 backpressure
```

| 選項 | 預設 | 意義（摘自官方 `docs/yaml-spec.md`） |
|---|---|---|
| `source` | 必填 | `上游id/輸出名`，或計時器路徑 |
| `queue_size` | `10` | 這條輸入的緩衝大小 |
| `queue_policy` | `drop_oldest` | `drop_oldest`：滿了就丟最舊的一筆。`backpressure`：最多緩衝到 10 × `queue_size` 筆而不丟；到了硬上限才丟，並記一筆 ERROR log |

佇列是**每條輸入各自一個**，設在**接收端**（下游節點的 `inputs` 底下），不是設在 producer 那邊。

### 4. `drop_oldest`：永遠留最新的，舊的默默丟掉

`drop_oldest` 適合「只在乎最新值」的資料，例如相機影格、感測器讀數：慢的下游拿到的永遠是**最近的** N 筆。

實測（producer 50ms／consumer 200ms，40 筆）：

| 設定 | 收到 | 遺失 | 規律 |
|---|---|---|---|
| `queue_size: 1` | **11/40** | 29 | 1, 5, 9, 13 … 每次跳過 3 筆，最後一筆收到 40 |
| 預設（`queue_size: 10`） | **20/40** | 20 | 先收 1～4，接著 8, 12, 16 …（每次跳過 3 筆），producer 結束後一口氣收 31～40 |

一個很好記的估算：**收到筆數 ≈ producer 送完前下游處理掉的筆數 ＋ 佇列裡最後留著的 `queue_size` 筆**。本例前者約 10 筆，所以 `queue_size: 1` 得 11、`queue_size: 10` 得 20，和實測一致。

另外兩件事：

- **producer 完全沒被拖慢。** 所有組別的 producer 都在約 1950ms 送完 40 筆；`drop_oldest` 是在接收端丟，不會回頭卡住上游。
- **丟資料是安靜的。** 預設 `dora run` 的輸出裡看不到任何「丟了幾筆」的警告行，本篇只能靠**序號缺口**看出來。需要可觀測性時，請自己在資料裡帶序號或時間戳。

### 5. `backpressure`：不丟，排隊等你

`queue_policy: backpressure` 適合「每一筆都重要」的資料，例如指令、交易、設定更新。

實測同樣的 4 倍速差：

| 設定 | 10× 上限 | 收到 | 遺失 | ERROR log | producer 耗時 | consumer 耗時 |
|---|---|---|---|---|---|---|
| `queue_size: 5` ＋ backpressure | 50 | **40/40** | 0 | 0 行 | 1950ms | 8017ms |
| `queue_size: 2` ＋ backpressure | 20 | **40/40** | 0 | 0 行 | 1950ms | 8019ms |

代價很直觀：**延遲**。consumer 要花約 8 秒（40 × 200ms）才消化完，最後一筆比它被送出的時間晚了約 6 秒。

關於「10 倍硬上限」，我刻意用 `queue_size: 2`（上限 20 筆）去挑戰約 30 筆的積壓，**結果沒有觸發上限、也沒有 ERROR，40 筆全收**。從 v1.1.0 原始碼看，走 daemon 路徑的事件在進入節點的排程佇列之前，還會先經過節點的接收通道（容量為各輸入 `queue_size` 加總、至少 64），而 daemon 那一側也有自己的緩衝，所以這個規模的積壓停在上游，沒有塞爆節點端的排程佇列。這是「本機、40 筆」的觀察，**不代表上限永遠不會被碰到**；官方文件仍明確寫著硬上限與以下會丟資料的情況。

官方規格裡另外幾個重點（本篇沒有逐一實測，照文件轉述）：

- `backpressure` 輸入會讓**該 producer 的整個輸出**改走 daemon 路徑，而不是直接的 zenoh 路徑；這條輸出的**所有**消費者都會離開零拷貝路徑，並受每則訊息 64 MiB 的上限限制。所以本篇把各種策略拆成**不同的 YAML 檔**，同一份資料流裡不會有一個 backpressure 消費者去影響其他消費者的結果。
- 接收端的每節點通道（1000 個事件）也滿了時，daemon 會**擋住 producer 的 `send_output`**，讓它阻塞而不是丟訊息。
- 仍會丟資料的情況：跨機器轉送到已滿的接收端（遠端 producer 擋不住）；接收端 60 秒完全沒騰出空間（卡死，或 backpressure 形成環）時，daemon 會停止擋住 producer，之後送往該已滿通道的訊息會被丟棄。

### 6. 隱藏大地雷：`Stop(AllInputsClosed)` 會插隊

這是本篇實測時真的踩到的坑，值得獨立一節。

dora 的節點排程器會**優先送出非輸入事件**（`InputClosed`、`Stop` 等控制事件），好讓節點在佇列很滿時也能快速反應。結果是：producer 一結束，consumer 會**先**收到 `InputClosed` 與 `Stop(AllInputsClosed)`，而佇列裡**還沒處理的資料排在後面**。

如果你照第 7 篇的寫法，一收到就 `break`：

```rust
Event::Stop(cause) => {
    println!("consumer 收到 Stop({cause:?})，準備退出");
    break; // ← 佇列裡剩下的資料全部沒處理到
}
```

實測（舊版 consumer，一收到 Stop 就 break 的對照；完整 log 未納入本 repo）：

| 設定 | 提早 `break` 的結果 | 改成清空佇列後 |
|---|---|---|
| `queue_size: 1` ＋ drop_oldest | 10/40，最後一筆是 37 | 11/40，最後一筆是 40 |
| `queue_size: 5` ＋ backpressure | **10/40，缺口 [11-40]** | **40/40** |

backpressure 好不容易排隊保住的 30 筆，全被自己的 `break` 扔掉了。正確做法：

- `Stop(AllInputsClosed)`：**不要 break**，繼續 `events.recv()`，直到它回傳 `None`（事件流關閉）為止。
- `Stop(Manual)`（`dora stop`、Ctrl-C、`--stop-after`、`timeout`）：才是「請盡快退出」，這時 `break`。

> 第 7 篇的節點都很快、佇列裡幾乎沒有積壓，所以沒有踩到；但只要下游慢，這個差別就會變成資料遺失。

### 7. 永不關閉的計時器 vs. `exit_when_nodes_finish`

如果 consumer 除了資料，還掛了一個計時器（例如每秒印一次心跳）：

```yaml
inputs:
  data: producer/seq
  heartbeat: dora/timer/secs/1    # 永遠不會關閉
```

預設情況下，節點要等**所有**輸入都關閉才會收到 `AllInputsClosed`，而計時器永遠不會關。所以 producer 結束、`data` 也關了，consumer 仍然一直收到心跳，整個資料流**不會自己結束**。實測：`timeout 15s dora run clock-no-exit.yml` 一路跑到被 `timeout` 停下（`timeout` 的結束碼是 124），consumer 最後收到的是 `Stop(Manual)`。

三種收工方式：

| 方式 | 寫法 | 本篇實測 |
|---|---|---|
| YAML 頂層開關 | `exit_when_nodes_finish: true` | `clock.yml`：資料輸入關閉後收到 `Stop(AllInputsClosed)`，結束碼 0 |
| CLI 旗標 | `dora run --exit-when-nodes-finish clock-no-exit.yml` | 同上，結束碼 0（旗標會覆寫 YAML 的設定，也可寫 `=false` 強制關閉） |
| 外部停止 | `dora stop`（第 6 篇）、`dora run --stop-after 15s`、`timeout 15s …` | 對長跑資料流才是正常做法 |

注意：`exit_when_nodes_finish` 只影響「**有資料輸入**」的節點。只吃計時器的來源節點（本篇的 producer）沒有可以「完成」的上游，dora 不會叫它停——它還是要自己 `break`。官方也提醒，這個開關通常只適合批次型的執行；長期運作的資料流本來就是靠計時器維持活著，用 `dora stop` 結束。

### 8. 常見地雷

1. **以為計時器會等你** → 它不會。處理時間 > 計時器間隔，tick 或下游資料就會堆積或被丟。
2. **以為 `drop_oldest` 會告訴你丟了多少** → 預設輸出裡看不到；請在資料裡帶序號自己檢查。
3. **一收到 `Stop(AllInputsClosed)` 或 `InputClosed` 就 `break`** → 控制事件會插隊，佇列裡的資料直接被丟（實測 backpressure 從 40/40 掉到 10/40）。
4. **把 `queue_size` 設在 producer 底下** → 它是**接收端**每條輸入的設定。
5. **在同一份資料流裡混用 backpressure 消費者，卻拿另一個消費者量零拷貝效能** → backpressure 會把整個 producer 輸出改走 daemon 路徑。
6. **把 backpressure 當成無限佇列** → 有 10 × `queue_size` 的硬上限，跨機器與接收端卡死 60 秒時也會丟；而且排隊就是延遲，下游永遠追不上時，積壓只會越來越長。
7. **只吃計時器的節點寫成等 `InputClosed`** → 永遠等不到；要自己決定何時收工。
8. **用 `timeout` 跑完就以為失敗** → GNU `timeout` 時間到一律回 124，即使 dora 自己乾淨收工也一樣。

## 完整可執行範例：快 producer × 慢 consumer，七種配線

一個 Cargo 套件、兩個執行檔，搭配七份共用同一組執行檔的 YAML：

```
examples/lesson-08/
├── Cargo.toml
├── drop-latest.yml        # 50ms × queue_size 1 × drop_oldest
├── drop-default.yml       # 50ms × 預設（10, drop_oldest）
├── backpressure.yml       # 50ms × queue_size 5 × backpressure
├── backpressure-cap.yml   # 50ms × queue_size 2 × backpressure（挑戰 10× 上限）
├── matched.yml            # 4 Hz（250ms）× 預設佇列
├── clock.yml              # matched ＋ 心跳計時器 ＋ exit_when_nodes_finish: true
├── clock-no-exit.yml      # 同上但沒有 exit_when_nodes_finish（對照組）
└── src/bin/
    ├── producer.rs
    └── consumer.rs
```

`Cargo.toml`：

```toml
[package]
name = "lesson-08"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

`drop-latest.yml`（只留最新的一筆）：

```yaml
nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/millis/50
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data:
        source: producer/seq
        queue_size: 1
        queue_policy: drop_oldest   # 預設值，寫出來比較清楚
```

`drop-default.yml`（簡寫＝預設佇列）：

```yaml
nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/millis/50
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data: producer/seq          # 簡寫 = queue_size 10、drop_oldest
```

`backpressure.yml`：

```yaml
nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/millis/50
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data:
        source: producer/seq
        queue_size: 5               # backpressure 上限 = 10 × 5 = 50 筆
        queue_policy: backpressure
```

`backpressure-cap.yml`：

```yaml
nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/millis/50
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data:
        source: producer/seq
        queue_size: 2               # backpressure 上限 = 10 × 2 = 20 筆
        queue_policy: backpressure
```

`matched.yml`（頻率對齊）：

```yaml
nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/hz/4
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data: producer/seq          # 4 Hz = 每 250ms 一筆，下游 200ms 處理得完
```

`clock.yml`（計時器當時鐘）：

```yaml
# 把計時器當「時鐘」而不是「工作」：資料輸入都關閉後就讓節點收工
exit_when_nodes_finish: true

nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/hz/4
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data: producer/seq
      heartbeat: dora/timer/secs/1  # 時鐘：永遠不會關閉
```

`clock-no-exit.yml`（對照組：和 `clock.yml` 相同，只是拿掉最上面的 `exit_when_nodes_finish`）：

```yaml
nodes:
  - id: producer
    description: 快速來源：每個 tick 送一個遞增序號，送滿 TOTAL 筆後自行結束
    build: cargo build --bin producer
    path: target/debug/producer
    env:
      TOTAL: "40"
    inputs:
      tick: dora/timer/hz/4
    outputs:
      - seq

  - id: consumer
    description: 慢速下游：每筆處理 WORK_MS 毫秒，最後列出序號缺口
    build: cargo build --bin consumer
    path: target/debug/consumer
    env:
      WORK_MS: "200"
      TOTAL: "40"
    inputs:
      data: producer/seq
      heartbeat: dora/timer/secs/1  # 時鐘：永遠不會關閉
```

`src/bin/producer.rs`：

```rust
use dora_node_api::{DoraNode, Event, MetadataParameters};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    // 送幾筆後自行結束（預設 40），由 YAML 的 env 決定
    let total: u64 = std::env::var("TOTAL").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let start = Instant::now();
    let mut seq: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            // tick 是 dora 自己產生的計時器事件，不是我們的程式
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                let t = start.elapsed().as_millis();
                node.send_output("seq".into(), MetadataParameters::default(), seq)?;
                println!("producer t={t:>5}ms 送出 seq={seq}");
                if seq >= total {
                    break; // 計時器永遠不會關閉，來源節點要自己決定何時收工
                }
            }
            Event::Stop(cause) => {
                println!("producer 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("producer 其他事件：{other:?}"),
        }
    }
    let t = start.elapsed().as_millis();
    println!("producer 結束：共送出 {seq} 筆，耗時 {t}ms");
    Ok(())
}
```

`src/bin/consumer.rs`：

```rust
use dora_node_api::{DoraNode, Event, StopCause};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    // 每筆模擬處理時間（毫秒）與預期總筆數，都由 YAML 的 env 決定
    let work_ms: u64 = std::env::var("WORK_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let expected: u64 = std::env::var("TOTAL").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let start = Instant::now();
    let mut received: Vec<u64> = Vec::new();
    let mut heartbeats = 0u32;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "data" => {
                let seq = u64::try_from(&data)?;
                let skipped = match received.last() {
                    Some(&prev) => seq.saturating_sub(prev + 1),
                    None => seq - 1,
                };
                let t = start.elapsed().as_millis();
                if skipped > 0 {
                    println!("consumer t={t:>5}ms 處理 seq={seq}（前面跳過 {skipped} 筆）");
                } else {
                    println!("consumer t={t:>5}ms 處理 seq={seq}");
                }
                received.push(seq);
                sleep(Duration::from_millis(work_ms)); // 模擬很慢的下游
            }
            Event::Input { id, .. } if id.as_str() == "heartbeat" => {
                heartbeats += 1;
                let t = start.elapsed().as_millis();
                println!("consumer t={t:>5}ms 心跳 #{heartbeats}（目前已收 {} 筆）", received.len());
            }
            Event::InputClosed { id } => {
                println!("consumer 輸入 {id} 已關閉");
            }
            Event::Stop(StopCause::AllInputsClosed) => {
                // 控制事件會「插隊」排在已緩衝的資料前面：此時佇列裡可能還有資料，
                // 所以不要 break，繼續 recv() 直到回傳 None 為止
                println!("consumer 收到 Stop(AllInputsClosed)，繼續清空佇列");
            }
            Event::Stop(cause) => {
                println!("consumer 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("consumer 其他事件：{other:?}"),
        }
    }

    // ---- 總結：用序號缺口看出丟了哪些 ----
    let mut gaps: Vec<String> = Vec::new();
    let mut prev = 0u64;
    for &s in &received {
        if s > prev + 1 {
            let (a, b) = (prev + 1, s - 1);
            gaps.push(if a == b { format!("{a}") } else { format!("{a}-{b}") });
        }
        prev = s;
    }
    if prev < expected {
        let (a, b) = (prev + 1, expected);
        gaps.push(if a == b { format!("{a}") } else { format!("{a}-{b}") });
    }
    let missing = expected - received.len() as u64;
    let t = start.elapsed().as_millis();
    println!(
        "consumer 總結：收到 {}/{expected} 筆，第一筆 seq={:?}，最後一筆 seq={:?}，遺失 {missing} 筆，缺口 [{}]，耗時 {t}ms",
        received.len(),
        received.first(),
        received.last(),
        gaps.join(", ")
    );
    Ok(())
}
```

## 怎麼跑

```bash
# export PATH="$HOME/dora/bin:$PATH"   # 依你的 dora CLI 安裝路徑調整
cd examples/lesson-08   # 在 clone 本 repo 後，於 repo 根目錄執行

for f in *.yml; do dora validate "$f"; done # 七份都應該結束碼 0
dora build drop-latest.yml                  # 兩個執行檔共用，build 一次即可

dora run drop-latest.yml
dora run drop-default.yml
dora run backpressure.yml
dora run backpressure-cap.yml
dora run matched.yml
dora run clock.yml

# 對照組：計時器讓它停不下來
timeout 15s dora run clock-no-exit.yml      # 預期結束碼 124
dora run --exit-when-nodes-finish clock-no-exit.yml   # 用旗標補救，結束碼 0
```

除了 `clock-no-exit.yml` 的 `timeout` 對照組，其餘都會自己結束：producer 送滿 40 筆 → consumer 的 `data` 關閉 → consumer 清空佇列 → 資料流結束。

**實測結果總表（2026-11-08 08:13～08:15 台北時間，皆為 producer 40 筆、consumer 每筆 200ms）**：

| YAML | 計時器 | 佇列設定 | `dora run` 結束碼 | 收到 | 遺失 | 缺口 | producer 耗時 | consumer 耗時 |
|---|---|---|---|---|---|---|---|---|
| `drop-latest.yml` | `millis/50` | 1, drop_oldest | 0 | 11/40 | 29 | 2-4, 6-8, 10-12, 14-16, 18-20, 22-24, 26-28, 30-32, 34-35, 37-39 | 1946ms | 2211ms |
| `drop-default.yml` | `millis/50` | 10, drop_oldest（預設） | 0 | 20/40 | 20 | 5-7, 9-11, 13-15, 17-19, 21-23, 25-27, 29-30 | 1945ms | 4013ms |
| `backpressure.yml` | `millis/50` | 5, backpressure | 0 | 40/40 | 0 | 無 | 1950ms | 8017ms |
| `backpressure-cap.yml` | `millis/50` | 2, backpressure | 0 | 40/40 | 0 | 無（未觸發硬上限，無 ERROR） | 1950ms | 8019ms |
| `matched.yml` | `hz/4` | 10, drop_oldest（預設） | 0 | 40/40 | 0 | 無 | 9744ms | 9951ms |
| `clock.yml` | `hz/4` ＋ 心跳 `secs/1` | 預設 | 0 | 40/40 | 0 | 無 | 9747ms | 9953ms |
| `clock-no-exit.yml`（`timeout 15s`） | 同上 | 預設 | **124** | 40/40 | 0 | 無 | 9749ms | 14504ms（被停下才結束） |
| `clock-no-exit.yml` ＋ `--exit-when-nodes-finish` | 同上 | 預設 | 0 | 40/40 | 0 | 無 | 9747ms | 9953ms |

**實測輸出：`drop-latest.yml`（略去 producer 每筆的送出行與 dora INFO/DEBUG 行）**：

```text
consumer t=    8ms 處理 seq=1
consumer t=  208ms 處理 seq=5（前面跳過 3 筆）
consumer t=  408ms 處理 seq=9（前面跳過 3 筆）
consumer t=  609ms 處理 seq=13（前面跳過 3 筆）
…（每 200ms 一筆，每次跳過 3 筆）
consumer t= 1610ms 處理 seq=33（前面跳過 3 筆）
consumer t= 1811ms 處理 seq=36（前面跳過 2 筆）
producer 結束：共送出 40 筆，耗時 1946ms
consumer 輸入 data 已關閉
consumer 收到 Stop(AllInputsClosed)，繼續清空佇列
consumer t= 2011ms 處理 seq=40（前面跳過 3 筆）
consumer 總結：收到 11/40 筆，第一筆 seq=Some(1)，最後一筆 seq=Some(40)，遺失 29 筆，缺口 [2-4, 6-8, 10-12, 14-16, 18-20, 22-24, 26-28, 30-32, 34-35, 37-39]，耗時 2211ms
```

**實測輸出：`drop-default.yml`（節錄）**：

```text
consumer t=    8ms 處理 seq=1
consumer t=  208ms 處理 seq=2
consumer t=  408ms 處理 seq=3
consumer t=  608ms 處理 seq=4
consumer t=  809ms 處理 seq=8（前面跳過 3 筆）
consumer t= 1009ms 處理 seq=12（前面跳過 3 筆）
…
consumer 輸入 data 已關閉
consumer 收到 Stop(AllInputsClosed)，繼續清空佇列
consumer t= 2011ms 處理 seq=31（前面跳過 2 筆）
consumer t= 2211ms 處理 seq=32
…（33～39 連續）
consumer t= 3813ms 處理 seq=40
consumer 總結：收到 20/40 筆，第一筆 seq=Some(1)，最後一筆 seq=Some(40)，遺失 20 筆，缺口 [5-7, 9-11, 13-15, 17-19, 21-23, 25-27, 29-30]，耗時 4013ms
```

producer 結束時，佇列裡留著的正好是最後 10 筆（31～40）——這就是 `queue_size: 10` 的意思。

**實測輸出：`backpressure.yml`（節錄）**：

```text
consumer t=    1ms 處理 seq=1
consumer t=  201ms 處理 seq=2
…（每 200ms 一筆，連續不跳號）
consumer t= 1805ms 處理 seq=10
producer 結束：共送出 40 筆，耗時 1950ms
consumer 輸入 data 已關閉
consumer 收到 Stop(AllInputsClosed)，繼續清空佇列
consumer t= 2005ms 處理 seq=11
consumer t= 2205ms 處理 seq=12
…（13～39 連續）
consumer t= 7817ms 處理 seq=40
consumer 總結：收到 40/40 筆，第一筆 seq=Some(1)，最後一筆 seq=Some(40)，遺失 0 筆，缺口 []，耗時 8017ms
```

**實測輸出：`clock-no-exit.yml` ＋ `timeout 15s`（節錄）**：

```text
consumer t= 9001ms 心跳 #10（目前已收 36 筆）
producer 結束：共送出 40 筆，耗時 9749ms
consumer 輸入 data 已關閉
consumer t=10001ms 心跳 #11（目前已收 40 筆）
consumer t=11001ms 心跳 #12（目前已收 40 筆）
…（資料早就收完了，心跳還在跳）
consumer t=14001ms 心跳 #15（目前已收 40 筆）
INFO dora_daemon: received termination signal -> stopping dataflow
consumer 收到 Stop(Manual)，準備退出
consumer 總結：收到 40/40 筆，第一筆 seq=Some(1)，最後一筆 seq=Some(40)，遺失 0 筆，缺口 []，耗時 14504ms
```

對照 `clock.yml`（開了 `exit_when_nodes_finish: true`）：`data` 關閉後 consumer 立刻收到 `Stop(AllInputsClosed)`，在 9953ms 印出總結並結束，`dora run` 結束碼 0。

> 編譯備註：首次 `dora build` 約 1 分 24 秒（含下載與編譯依賴）；實驗完建議 `cargo clean` 釋放 `target/` 空間。完整執行 log **未**納入本 repo；上文節錄已含主要對照。

## 小練習

1. **先預測，再實測**：把 `drop-default.yml` 的 `queue_size` 改成 `5`（用長寫法），其他不變。用「producer 送完前約處理 10 筆 ＋ 佇列留下的 `queue_size` 筆」估算，你預期收到幾筆？最後收到的連續區段會是哪幾號？再把 producer 的計時器改成 `dora/timer/millis/100`（速差變 2 倍），預測又會變成多少？
2. **找出「剛好不累積」的頻率**：把 `matched.yml` 的 `hz/4` 改成 `hz/5`（200ms，剛好等於處理時間）和 `hz/6`（約 167ms），觀察缺口何時開始出現。提示：consumer 的 `sleep` 之外還有印字與事件傳遞的開銷。
3. **重現插隊地雷**：在 `consumer.rs` 把 `Stop(AllInputsClosed)` 那一支改回 `break`，重跑 `backpressure.yml`。收到幾筆？缺口是哪一段？再想想：如果你的節點是「把每一筆寫進資料庫」，這個 bug 在正式環境會長什麼樣子？

下一篇：[第 9 篇：錯誤處理與重啟](lesson-09.md)。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 7 篇](lesson-07.md)
- 下一課：[第 9 篇 →](lesson-09.md)
- 本課範例：[examples/lesson-08/](../../examples/lesson-08/)
