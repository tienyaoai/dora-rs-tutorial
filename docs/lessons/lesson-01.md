# dora-rs 教學 第 1 篇：dora-rs 是什麼？跟 ROS 2 差在哪

> 系列：Rust 為主、從基礎到進階，共 20 篇。驗證環境：dora CLI / dora-node-api v1.1.0、Rust 1.99.1、x86_64 Linux。


**導覽**

- [返回教材總覽](../../README.md)
- 下一課：[第 2 篇 →](lesson-02.md)
- 本課範例：[examples/lesson-01/](../../examples/lesson-01/)

## 目錄

- [重點說明](#重點說明)
- [完整範例：talker → listener](#完整範例talker-listener)
- [小練習](#小練習)
## 重點說明

**dora-rs（Dataflow-Oriented Robotic Architecture）** 是用 Rust 寫的機器人與 AI 應用框架。你把程式拆成很多「節點（node）」，再用一個 YAML 檔描述資料怎麼在節點之間流動，這個 YAML 就叫 **dataflow**。

幾個核心概念：

- **Node（節點）**：一支獨立的程式（Rust、Python、C/C++ 都可以），各自是一個行程。
- **Input / Output**：節點宣告自己有哪些輸出，別的節點用 `節點id/輸出名` 訂閱它。
- **Dataflow（dataflow.yml）**：整張圖的定義，誰接誰一目了然。
- **Timer**：內建的時間來源，例如 `dora/timer/millis/500` 每 500 毫秒送一個 tick。
- **Daemon 與 Coordinator**：daemon 在每台機器上負責啟動節點、搬運資料；coordinator 管理整個（可跨機器的）資料流。第 14 篇會細講。
- **Apache Arrow**：所有訊息都是 Arrow 格式，跨語言不用重新序列化；同機器上的大訊息走共享記憶體，可以做到零拷貝（第 5、10 篇）。

### 跟 ROS 2 比較

下列逐項對照（直向閱讀，小螢幕不必橫向捲動）。

**系統連線方式**

- **ROS 2：** 執行時用 topic 名稱彼此發現（DDS）
- **dora-rs：** 事先在 dataflow.yml 寫死整張圖

**訊息格式**

- **ROS 2：** `.msg` 定義、需要產生程式碼
- **dora-rs：** Apache Arrow，不需要產生程式碼

**大資料傳輸**

- **ROS 2：** 一般要序列化；零拷貝需要特定設定
- **dora-rs：** 同機器自動走共享記憶體

**建置工具**

- **ROS 2：** colcon / ament，需要安裝 ROS 發行版
- **dora-rs：** 一般的 `cargo` 專案＋一個 `dora` 執行檔

**生態系**

- **ROS 2：** 非常龐大（導航、MoveIt、驅動程式…）
- **dora-rs：** 較小，但有 ROS 2 bridge 可以互通

**適合情境**

- **ROS 2：** 需要大量現成套件的完整機器人系統
- **dora-rs：** 重視低延遲、AI 管線、想用純 Rust 開發

一句話：ROS 2 是「大家在網路上互相找」，dora 是「先畫好流程圖再跑」。流程圖寫死的好處是好除錯、好部署，也讓框架能幫你做最佳化。

我們 10/31 在主機上的實測（官方 benchmark 範例）：小訊息（0 B～16 KB）節點對節點延遲中位數約 60～200 µs，400 KB 約 1.1 ms。官方 repo 也有 `examples/ros2-comparison` 可以拿兩者跑同一份負載比較，但我們手上沒有 ROS 2 環境，所以**還沒有自己量過 ROS 2 的數字**。

## 完整範例：talker → listener

第一篇先感受一下 dora 長什麼樣子，細節會在第 3、4 篇拆開講。

專案結構：

```
examples/lesson-01/
├── Cargo.toml
├── dataflow.yml
└── src/bin/
    ├── talker.rs
    └── listener.rs
```

`Cargo.toml`：

```toml
[package]
name = "lesson-01"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

`src/bin/talker.rs`（每收到一次 tick 就送一句問候，送 5 句後結束）：

```rust
use dora_node_api::{DoraNode, Event, IntoArrow, dora_core::config::DataId};

fn main() -> eyre::Result<()> {
    // 從 dora daemon 設定的環境變數初始化節點，拿到「發送端」與「事件串流」
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let output = DataId::from("greeting".to_owned());
    let mut count = 0u32;

    // 事件迴圈：一直收事件，直到資料流結束（recv 回傳 None）
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, .. } if id.as_str() == "tick" => {
                count += 1;
                let msg = format!("你好，dora！第 {count} 則");
                node.send_output(output.clone(), metadata.parameters, msg.as_str().into_arrow())?;
                if count == 5 {
                    break; // 送完 5 則就結束，listener 會收到 InputClosed
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
```

`src/bin/listener.rs`（印出收到的每一則訊息）：

```rust
use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } => {
                // data 是 Arrow 陣列，這裡轉回 &str
                let text: &str = TryFrom::try_from(&data)?;
                println!("listener 收到 [{id}]：{text}");
            }
            Event::InputClosed { id } => println!("輸入 {id} 已關閉"),
            Event::Stop(_) => println!("收到停止訊號"),
            _ => {}
        }
    }
    println!("listener 結束");
    Ok(())
}
```

`dataflow.yml`：

```yaml
nodes:
  - id: talker
    build: cargo build --bin talker
    path: target/debug/talker
    inputs:
      tick: dora/timer/millis/500   # 每 500 毫秒觸發一次
    outputs:
      - greeting

  - id: listener
    build: cargo build --bin listener
    path: target/debug/listener
    inputs:
      greeting: talker/greeting
```

執行（`dora run` 會在本機自動起 daemon、跑完就收掉）：

```bash
export PATH=$HOME/dora/bin:$PATH   # 依你安裝 dora CLI 的位置調整
cd examples/lesson-01              # 在 clone 本 repo 後，於 repo 根目錄執行
dora build dataflow.yml            # 依照每個節點的 build 欄位編譯
dora run dataflow.yml
```

實際輸出（2026-11-01 在主機上跑的，節錄）：

```
08:06:28 stdout listener: listener 收到 [greeting]：你好，dora！第 1 則
08:06:29 stdout listener: listener 收到 [greeting]：你好，dora！第 2 則
08:06:29 stdout listener: listener 收到 [greeting]：你好，dora！第 3 則
08:06:30 stdout listener: listener 收到 [greeting]：你好，dora！第 4 則
08:06:30 stdout listener: listener 收到 [greeting]：你好，dora！第 5 則
08:06:30 stdout listener: 輸入 greeting 已關閉
08:06:30 stdout listener: 收到停止訊號
08:06:30 stdout listener: listener 結束
08:06:30 INFO   talker: daemon talker finished successfully
08:06:30 INFO   listener: daemon listener finished successfully
```

觀察重點：

1. 節點程式裡**沒有任何 topic 名稱或網路設定**，連線全在 YAML。
2. `events.recv()` 是阻塞式的事件迴圈，回傳 `None` 代表這個節點該結束了。
3. talker 結束後，listener 先收到 `InputClosed`，接著因為所有輸入都關了而收到 `Stop`，整個資料流自然結束。
4. 字串用 `into_arrow()` 變成 Arrow 陣列，收端用 `TryFrom` 轉回 `&str`。

小提醒：第一次 `dora build` 要從 crates.io 編譯 dora-node-api 的所有相依套件（包含 zenoh），在我們主機上約 1 分 12 秒，debug 版的 `target/` 約 3.6 GB，練習完記得 `cargo clean`。

## 小練習

1. 把 timer 改成 `dora/timer/millis/200`，確認訊息變快。
2. 新增第二個 listener 節點（例如 id 叫 `listener-2`），也訂閱 `talker/greeting`，看看兩個節點是不是都收到同樣的 5 則訊息。提示：只要改 `dataflow.yml`，不用改 Rust 程式。
3. 想一想：如果用 ROS 2 做同一件事，「誰訂閱誰」會寫在哪裡？

下一篇：[第 2 篇：安裝 dora CLI，跑官方 rust-dataflow 範例](lesson-02.md)。

---

**導覽**

- [返回教材總覽](../../README.md)
- 下一課：[第 2 篇 →](lesson-02.md)
- 本課範例：[examples/lesson-01/](../../examples/lesson-01/)
