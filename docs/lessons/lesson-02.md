# dora-rs 教學 第 2 篇：安裝 dora CLI，跑官方 rust-dataflow 範例

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-02（台北時間），在 x86_64 Linux 主機上實際編譯並執行成功。



> **第 2 課範例說明：** 本 repo **沒有**可獨立 `cargo build` 的 `lesson-02` 專案。請依下文在 **dora-rs 官方 monorepo**（tag `v1.1.0`）內編譯 `examples/rust-dataflow/`。步驟摘要見 [examples/lesson-02/README.md](../../examples/lesson-02/README.md)。

- [返回教材總覽](../../README.md)
- 上一課：[第 1 篇](lesson-01.md)
- 下一課：[第 3 篇 →](lesson-03.md)
- 本課範例：[取得官方 rust-dataflow](../../examples/lesson-02/README.md)

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 1 篇](lesson-01.md)
- 下一課：[第 3 篇 →](lesson-03.md)
- 本課範例：[取得官方 rust-dataflow](../../examples/lesson-02/README.md)

## 目錄

- [重點說明](#重點說明)
- [小練習](#小練習)
## 重點說明

### 1. dora CLI 是什麼

`dora` 這支命令列工具一手包辦：編譯節點（`dora build`）、在本機直接跑一個資料流（`dora run`）、管理常駐的 coordinator / daemon（`dora up`、`dora start`，第 6 篇再講）。**CLI 版本要和節點用的 `dora-node-api` crate 版本一致**，不然節點跟 daemon 溝通的訊息格式可能對不上。

### 2. 三種安裝方式

**預編譯執行檔（我們用這個）**

- 做法：從 GitHub Release 下載 v1.1.0 的 tar.gz（下文第 3 節有完整指令）
- 適合：最快，幾秒鐘搞定

**官方安裝腳本**

- 做法：

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/dora-rs/dora/releases/latest/download/dora-cli-installer.sh | sh
```

- 適合：不想自己管路徑

**cargo 編譯**

- 做法：`cargo install dora-cli --locked`
- 適合：要改 CLI 原始碼，或平台沒有預編譯檔

預編譯檔需要 glibc ≥ 2.34。官方也有 `aarch64-unknown-linux-gnu` 版本，第 15 篇上 ZYNQ 時會用到。

### 3. 實際安裝步驟

```bash
mkdir -p ~/dora && cd ~/dora

# 下載 v1.1.0 預編譯 CLI（x86_64 Linux）
curl -sLO https://github.com/dora-rs/dora/releases/download/v1.1.0/dora-cli-x86_64-unknown-linux-gnu.tar.gz
tar xzf dora-cli-x86_64-unknown-linux-gnu.tar.gz
mkdir -p bin && cp dora-cli-x86_64-unknown-linux-gnu/dora bin/
export PATH=$HOME/dora/bin:$PATH      # 建議寫進 ~/.bashrc

dora --version    # 應顯示 dora-cli 1.1.0

# 取得官方範例（淺層 clone 對應版本的標籤）
git clone --depth 1 --branch v1.1.0 https://github.com/dora-rs/dora.git repo
```

Rust 工具鏈用 `rustup` 裝好 stable 即可（我們的主機是 Rust 1.99.1）。

### 4. rust-dataflow 範例長什麼樣

位置：`repo/examples/rust-dataflow/`，三個 Rust 節點串成一條線：

```
dora/timer/millis/10 ──tick──▶ rust-node ──random(u64)──▶ rust-status-node ──status(String)──▶ rust-sink
dora/timer/millis/100 ─tick─────────────────────────────▶ rust-status-node
```

- **rust-node**：每收到一次 10 ms 的 tick，就產生一個隨機 `u64` 送出到 `random`，送滿 100 次就結束。
- **rust-status-node**：數自己收到幾次 100 ms tick，每收到一個 random 就組一句字串送到 `status`；`random` 關閉時自己退出。
- **rust-sink**：印出收到的字串，並檢查格式，不對就 `bail!` 讓節點以錯誤結束。

`dataflow.yml`（節錄 rust-node）：

```yaml
nodes:
  - id: rust-node
    build: cargo build -p rust-dataflow-example-node     # dora build 時執行
    path: ../../target/debug/rust-dataflow-example-node  # 執行檔位置
    inputs:
      tick: dora/timer/millis/10
    outputs:
      - random
    output_types:
      random: std/core/v1/UInt64
```

`output_types` / `input_types` 是 v1.x 加入的型別宣告，下一篇會細講。

### 5. 範例程式：rust-sink（完整原始碼）

這是最短的節點，正好複習第 1 篇的事件迴圈：

```rust
use dora_node_api::{self, DoraNode, Event, EventStream};
use eyre::{Context, bail};

fn main() -> eyre::Result<()> {
    // 從 daemon 設好的環境變數取得節點身分與事件串流
    let (node, events) = DoraNode::init_from_env()?;
    run(node, events)
}

fn run(_node: DoraNode, mut events: EventStream) -> eyre::Result<()> {
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata: _, data } => match id.as_str() {
                "message" => {
                    // Arrow 字串陣列 → &str
                    let received_string: &str =
                        TryFrom::try_from(&data).context("expected string message")?;
                    println!("sink received message: {received_string}");
                    if !received_string.starts_with("operator received random value ") {
                        bail!("unexpected message format");
                    }
                    if !received_string.ends_with(" ticks") {
                        bail!("unexpected message format (should end with 'ticks')");
                    }
                }
                "exit" => {
                    println!("sink received exit signal, stopping");
                    break;
                }
                other => eprintln!("Ignoring unexpected input `{other}`"),
            },
            Event::Stop(_) => println!("Received stop"),
            Event::InputClosed { id } => println!("Input `{id}` was closed"),
            other => eprintln!("Received unexpected input: {other:?}"),
        }
    }
    Ok(())
}
```

（官方原檔多了 `#[cfg(test)] mod tests;`，測試在第 13 篇介紹。）

rust-status-node 收 `u64` 的寫法也值得記：

```rust
"random" => {
    let value = u64::try_from(&data).context("unexpected data type")?;
    let output = format!("operator received random value {value:#x} after {ticks} ticks");
    node.send_output(status_output.clone(), metadata.parameters, output.into_arrow())?;
}
```

### 6. 編譯與執行

```bash
export PATH=$HOME/dora/bin:$PATH
cd ~/dora/repo

# 方法 A：直接用 cargo 一次編三個節點（共用 workspace 的 target/）
cargo build -p rust-dataflow-example-node \
            -p rust-dataflow-example-status-node \
            -p rust-dataflow-example-sink

# 方法 B：讓 dora 依 YAML 的 build 欄位編譯
cd examples/rust-dataflow
dora build dataflow.yml

# 執行（dora run 會在本機臨時起 daemon，資料流結束就收掉）
dora run dataflow.yml
```

**實測（2026-11-02 08:04–08:06 台北時間）**：首次 cargo 編譯 1 分 30 秒（8 核），`dora run` 結束碼 0，sink 收到 **100 則**訊息。輸出節錄：

```
08:06:34 DEBUG  rust-node: daemon::spawner spawning node
08:06:35 INFO   rust-node: rust_dataflow_example_node tick 1 with data DoraArray(NullArray(0)), sending 0x7e5ba61552085fc6
08:06:35 stdout rust-sink: sink received message: operator received random value 0x7e5ba61552085fc6 after 1 ticks
...
08:06:36 stdout rust-status-node: input `random` was closed
08:06:36 stdout rust-status-node: `random` input was closed -> exiting
08:06:36 INFO   rust-node: daemon rust-node finished successfully
08:06:36 stdout rust-sink: Input `message` was closed
08:06:36 stdout rust-sink: Received stop
08:06:36 INFO   rust-status-node: daemon rust-status-node finished successfully
08:06:36 INFO   rust-sink: daemon rust-sink finished successfully
08:06:36 INFO   [dora]: daemon dataflow finished on machine `01a19994-...`
```

觀察重點：

1. **timer 的資料是空的**：`DoraArray(NullArray(0))`，tick 只是「時間到了」的訊號。
2. **結束是連鎖的**：rust-node 跑完 100 次離開 → status-node 收到 `InputClosed(random)` 主動退出 → sink 的輸入全關，收到 `Stop` → daemon 判定整個資料流完成。
3. **節點的 println 會被 daemon 收集**，前面標上 `stdout 節點名`；`tracing::info!` 則顯示成 `INFO`。
4. 這次 `dora run` 也執行了 YAML 裡的 `build` 指令（log 中可以看到 `Compiling ...`），而且部分相依套件被重新編譯了一次，整個 `dora run` 花了將近 1 分鐘才真正開始跑資料流。實務上建議先 `dora build`、再 `dora run`，比較好分辨是編譯慢還是執行有問題。

### 7. 常見問題

- `dora: command not found` → PATH 沒設好。
- 節點啟動失敗、找不到執行檔 → `path:` 是相對於 YAML 檔所在目錄；用了 `CARGO_TARGET_DIR` 就要確認執行檔真的在 `../../target/debug/` 下。
- 版本不符 → `dora --version` 和 `Cargo.toml` 裡 `dora-node-api` 要同為 1.1.0。
- 硬碟：debug 編譯產物這次約 **7.4 GB**，玩完記得 `cargo clean`（或刪掉 `target/debug`）。

## 小練習

1. 把 `rust-node` 的 timer 從 `millis/10` 改成 `millis/50`，重跑。sink 印出的 `after N ticks` 會變成多少？為什麼？（提示：status-node 的 tick 是 100 ms。）
2. 在 `dataflow.yml` 加一個節點 `rust-sink-2`，`path` 指向同一個 sink 執行檔，也訂閱 `rust-status-node/status`。確認兩個 sink 都收到 100 則。
3. 進階：故意把 status-node 送出的字串改壞（例如拿掉結尾的 `ticks`），重新編譯後執行，觀察 sink 用 `bail!` 結束時，`dora run` 的輸出和結束碼有什麼不同。

下一篇：[第 3 篇：逐行讀懂 dataflow.yml](lesson-03.md)。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 1 篇](lesson-01.md)
- 下一課：[第 3 篇 →](lesson-03.md)
- 本課範例：[取得官方 rust-dataflow](../../examples/lesson-02/README.md)
