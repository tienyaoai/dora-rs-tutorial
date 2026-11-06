# dora-rs 教學 第 6 篇：`dora up / start / logs / down`——資料流的生命週期管理

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-06（台北時間），x86_64 Linux 上實際 `dora validate` / `dora build` / `dora up` / `dora start --detach` / `dora list` / `dora logs` / `dora stop` / `dora down` 成功；結束後 `dora status` 確認 coordinator 已關閉（結束碼 1，符合預期）。另驗證 `dora destroy` 與 `dora down` 行為相同。


**導覽：** [返回教材總覽](../../README.md) · [← 第 5 篇](lesson-05.md) · [第 7 篇（尚未發布）](../../README.md#課程大綱20-篇規劃) · **範例：** [examples/lesson-06/](../../examples/lesson-06/)
## 目錄

- [重點說明](#重點說明)
- [完整可執行範例：ticker → printer（長跑，靠 stop 收工）](#完整可執行範例ticker-printer長跑靠-stop-收工)
- [怎麼跑](#怎麼跑)
- [小練習](#小練習)
## 重點說明

前幾篇一律用 `dora run`：它在**隔離模式**下把整個資料流跑完就結束，不會留下常駐的 coordinator / daemon。本機練習很方便，但一離開終端機、或想同時管多個資料流、或要遠端查日誌，就不夠用了。

這一篇換成**長駐生命週期**：

```
dora up  →  dora start  →  dora logs / list / stop  →  dora down
```

### 1. `dora run`  vs  長駐生命週期

| | `dora run` | `dora up` + `dora start` |
|---|---|---|
| coordinator / daemon | 這次執行專用，跑完就沒了 | 先 `up` 常駐，之後可反覆 `start` |
| 終端機 | 前景跑到結束才還給你 | `start --detach` 後 CLI 立刻回來；`--attach` 則等同前景 |
| 多個資料流 | 一次一個 | 同一個 coordinator 上可同時跑多個 |
| 查日誌 / 列表 | 看當下 stdout | `dora logs`、`dora list`、`dora status` |
| 收工 | 資料流自己結束，或 Ctrl-C | `dora stop` 停資料流；`dora down` 拆掉 coordinator+daemon |
| 適合 | 第 2–5 篇這種「跑完就看結果」的練習 | 長跑、除錯、之後的分散式（第 14 篇） |

記住一句話：**`run` = 一次性；`up`/`start`/`down` = 服務式管理。**

### 2. coordinator 與 daemon 各做什麼（高階）

- **coordinator**：本機（或叢集）的「總台」。預設聽 `127.0.0.1:6013`。負責記有哪些資料流在跑、接受 `start` / `stop` / `list` / `logs` 這類 CLI 指令、把工作分派給 daemon。
- **daemon**：真正**拉起節點行程**的人。節點的環境變數、stdout 日誌、輸入輸出連線都經由它。本機 `dora up` 會在同機起一個 daemon；多機部署時每台機器各跑一個（細節留到第 14 篇）。

`dora up` = 本機模式一次把兩者都拉起來。`dora down` = 兩者一起拆掉。

### 3. 指令走一遍（會用到的選項）

**`dora up`**
本機起 coordinator + daemon。常見選項：`--interface`（綁定網卡；預設 loopback，外機連不進來）、`--auth`（寫 token 到設定檔）、`--recreate-store`（升級後 store schema 不相容時用）。

**`dora build dataflow.yml`**
跟 `dora run` 前一樣：跑 YAML 裡每個節點的 `build:`。`start` 之前通常先 build 好。

**`dora start dataflow.yml -n NAME --detach`**
在**已經 `up` 的** coordinator 上啟動資料流，並取一個好記的名字（之後 `logs` / `stop` 都用這個名字，不必背 UUID）。
- `--detach`：背景跑，CLI 印完「dataflow started: \<uuid\>」就返回。
- `--attach`（預設行為之一）：掛在前景，等資料流結束（比較像 `dora run` 的感覺）。
- `--exit-when-nodes-finish`：把計時器輸入當成「時鐘」而非「工作」——批次型資料流才需要；**長跑型（本篇範例）不要開**，否則計時器節點會在其他節點結束後被當成已完成。長跑請用 `dora stop` 收工。

**`dora list`**
列出目前 coordinator 知道的資料流（Running / Finished / Failed），含 UUID、名稱、節點數。

**`dora logs [uuid_or_name]`**
- `-n node`：只看某一個節點
- `--all-nodes`：所有節點依時間戳合併
- `--tail N`：只看最後 N 行
- `-f` / `--follow`：持續追蹤（像 `tail -f`）
- 另有 `--since` / `--until` / `--grep` / `--level` 等過濾

**`dora stop NAME`**
優雅停止：對所有節點送 `Event::Stop(Manual)`，節點應盡快跳出事件迴圈。加 `--force` 則直接殺行程；也可用 `--grace-duration` 控制等到強制殺的時間。

**`dora down` / `dora destroy`**
兩者**完全一樣**（v1.1.0 裡 `destroy` 是 `down` 的別名）。官方 help 也統一寫成 `Usage: dora down`。本教學以 **`dora down` 為正式名稱**，看到舊文件寫 `destroy` 就當同義詞。
- 若還有 Running 的資料流，沒加 `--force` 會拒絕拆台（避免誤拆別人的 coordinator，見 [dora-rs/dora#2924](https://github.com/dora-rs/dora/issues/2924)）。
- `--force`：先強制停掉仍在跑的資料流再拆。

**其他幾個順便認識**
- `dora restart NAME`：`stop` + 用當初存的 descriptor 再 `start`
- `dora clean`：清掉 Finished / Failed 的紀錄，列表比較乾淨
- `dora status`：看 coordinator / daemon 是否健康、有幾個 active dataflow。coordinator 沒在跑時會印錯誤並以**非 0**結束碼離開

### 4. 常見地雷

1. **忘了 `dora up` 就 `start`** → `could not connect to coordinator at 127.0.0.1:6013`，提示你先 `dora up`。
2. **搞混 `run` 與 `start`**：`run` 不需要（也不能依賴）常駐 coordinator；`start` 一定要先 `up`。想「跑完就走」用 `run`；想「掛著慢慢看」用 `up`+`start --detach`。
3. **6013 port 衝突**：同一台機器已經有別人的 `dora up`，再 `up` 一次會搶 port。解法是使用不同的 `DORA_COORDINATOR_PORT`；只有確認舊服務是自己的、且沒有其他工作時，才用 `dora down --force` 關閉。
4. **`dora down` 被拒**：還有 Running 資料流時要先 `dora stop`，或直接 `dora down --force`。
5. **計時器資料流不會自己結束**：`dora/timer/...` 永遠不關閉，所以本篇的 ticker/printer **不會**像第 4、5 篇那樣自己收工——一定要 `dora stop`（或 Ctrl-C 在 `--attach` 模式）。

## 完整可執行範例：ticker → printer（長跑，靠 stop 收工）

```
examples/lesson-06/
├── Cargo.toml
├── dataflow.yml
└── src/bin/
    ├── ticker.rs
    └── printer.rs
```

`Cargo.toml`：

```toml
[package]
name = "lesson-06"
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
    description: 每 500ms 送出一個遞增的 u64 計數（故意長跑，不會自己結束）
    build: cargo build --bin ticker
    path: target/debug/ticker
    inputs:
      tick: dora/timer/millis/500
    outputs:
      - count

  - id: printer
    description: 印出 ticker 的計數
    build: cargo build --bin printer
    path: target/debug/printer
    inputs:
      count: ticker/count
```

`src/bin/ticker.rs`：

```rust
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut n: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                n += 1;
                node.send_output("count".into(), MetadataParameters::default(), n)?;
                println!("ticker 送出 count={n}");
            }
            Event::Stop(cause) => {
                println!("ticker 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("ticker 其他事件：{other:?}"),
        }
    }
    println!("ticker 結束（共送出 {n} 次）");
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
            Event::Input { id, data, .. } if id.as_str() == "count" => {
                let n = u64::try_from(&data)?;
                println!("printer 收到 count={n}");
            }
            Event::InputClosed { id } => {
                println!("printer 輸入 {id} 已關閉");
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

重點：兩個節點都**沒有**「數到 N 就 break」——它們會一直跑，直到收到 `Stop(Manual)`（來自 `dora stop`）。

## 怎麼跑

**請在獨立練習環境操作。`dora down --force` 會強制停止該 coordinator 上仍在跑的資料流；若有其他人的工作，不要執行。**

```bash
export PATH=$HOME/dora/bin:$PATH   # 依你安裝 dora CLI 的位置調整
cd examples/lesson-06   # 在 clone 本 repo 後，於 repo 根目錄執行
# 僅在確認 coordinator 屬於自己的練習、且沒有其他工作時，關閉舊服務
dora down --force 2>/dev/null || true

dora validate dataflow.yml
dora build dataflow.yml

dora up
dora status
dora start dataflow.yml -n lesson06 --detach
dora list

# 等幾秒讓計數往前跑，再看日誌
sleep 4
dora logs lesson06 --all-nodes --tail 30

dora stop lesson06
dora list          # 應變成 Finished
dora down
dora status        # coordinator 應顯示 Not running（結束碼 1）
```

**實測輸出（2026-11-06 08:07 台北時間；結束碼摘要見文末）**：

```text
$ dora validate dataflow.yml
Validating dataflow.yml...
Input/output wiring OK.
Descriptor config OK.
All type annotations OK.

$ dora up
started dora coordinator
started dora daemon

$ dora status
✓ Coordinator: Running
  Address: 127.0.0.1:6013
✓ Daemon: Running
Active dataflows: 0

$ dora start dataflow.yml -n lesson06 --detach
dataflow start triggered: 01a1ae2e-e65d-7ea7-abe7-65c15bb168d8
...
dataflow started: 01a1ae2e-e65d-7ea7-abe7-65c15bb168d8

$ dora list
UUID                                  Name      Status   Nodes  CPU   Memory
01a1ae2e-e65d-7ea7-abe7-65c15bb168d8  lesson06  Running  2      0.0%  0.0 MB

$ dora logs lesson06 --all-nodes --tail 30
08:07:52 stdout ticker: ticker 送出 count=1
08:07:52 stdout printer: printer 收到 count=1
08:07:52 stdout ticker: ticker 送出 count=2
08:07:52 stdout printer: printer 收到 count=2
...（每 500ms 一對，略）...
08:07:56 stdout ticker: ticker 送出 count=9
08:07:56 stdout printer: printer 收到 count=9

$ dora stop lesson06
（無額外輸出；結束碼 0）

$ dora list
UUID                                  Name      Status    Nodes  CPU   Memory
01a1ae2e-e65d-7ea7-abe7-65c15bb168d8  lesson06  Finished  0      0.0%  0.0 MB

$ dora down
Destroying coordinator at 127.0.0.1:6013
Coordinator and daemons destroyed successfully

$ dora status
✗ Coordinator: Not running
✗ Daemon: Unknown (coordinator not available)
[ERROR] System check failed.    ← 結束碼 1，代表「現在沒有常駐服務」，是正常結果
```

`dora stop` 當下，節點 stdout（寫在 `out/<uuid>/log_*.jsonl`，也可用 `dora logs` 再查一次）會看到：

```text
ticker 收到 Stop(Manual)，準備退出
ticker 結束（共送出 9 次）
printer 收到 Stop(Manual)，準備退出
printer 結束
```

這次實測從 start 到 stop 大約跑了 4 秒、送出 9 次（500 ms × 9 ≈ 4.5 s，與 sleep 時間吻合）。

> 驗證備註（台北時間 2026-11-06 08:07）：`validate` 0、`build` 0（首次約 1 分 30 秒）、`up` 0、`status`(up 後) 0、`start --detach` 0、`list` 0、`logs` 0、`stop` 0、`list`(Finished) 0、`down` 0、`status`(down 後) **1**。另以 `dora destroy` 重跑拆台流程，行為與 `dora down` 相同（結束碼 0）。實驗完已清掉 `target/`。（完整指令抄本未納入本 repo；僅保留上文節錄。）

## 小練習

1. 把 `dora start ... --detach` 改成 `--attach`，另開一個終端機對同一個名字下 `dora stop lesson06`，觀察前景那個 `start` 什麼時候返回。
2. 故意**不要**先 `dora up` 就執行 `dora start`，把錯誤訊息記下來；再 `dora up` 一次後重試。
3. 在 `ticker` 仍 Running 時執行不帶 `--force` 的 `dora down`，確認它會拒絕；再加上 `--force` 看結果。想練習 `dora clean` 的話，先 `stop` 讓狀態變 Finished，再 `dora clean`，最後 `dora list` 應變空。

下一篇：第 7 篇尚未發布；佇列與背壓實測預計在第 8 篇登場（見 [課程大綱](../../README.md#課程大綱20-篇規劃)）。

---

**導覽：** [返回教材總覽](../../README.md) · [← 第 5 篇](lesson-05.md) · [第 7 篇（尚未發布）](../../README.md#課程大綱20-篇規劃) · **範例：** [examples/lesson-06/](../../examples/lesson-06/)
