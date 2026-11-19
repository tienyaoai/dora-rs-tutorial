# dora-rs 教學 第 14 篇：分散式部署——coordinator 與 daemon，一條 dataflow 拆到多台機器

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0）　驗證日期：2026-11-14（台北時間）。**只有一台 x86_64 Linux 虛擬機**，所以用「一個 coordinator + 兩個不同 `--machine-id` 的 daemon」在同一台電腦上模擬兩台機器（dora 官方 `examples/multiple-daemons` 也是這樣做）。coordinator 綁在虛擬機的網卡位址 172.30.0.2（不是 127.0.0.1），走的是和真實區網一樣的程式路徑，但**沒有真的跨兩台實體機器、沒有經過防火牆或 NAT**，這部分只能依官方文件說明。實測結果：`dora validate` 三份 YAML 都通過；`machine` 指定與 `labels` 指定都把節點排到正確的 daemon（用 `ps` 看父行程 PID 確認）；10 秒各傳 51／48 筆、0 遺失；指定一台沒上線的機器時 `dora start` 結束碼 1。第一次 release 建置約 2 分 10 秒。

> **關於日誌：** 本篇引用的 `run/*.log` 等完整執行日誌**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。範例可依「怎麼跑／執行」小節自行重現。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 13 篇](lesson-13.md)
- 下一課：[第 15 篇 →](lesson-15.md)
- 本課範例：[examples/lesson-14/](../../examples/lesson-14/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [小練習](#小練習)
## 重點說明

第 6 篇用過 `dora up`：它在本機同時啟動一個 coordinator 和一個 daemon。到了真正的機器人或邊緣裝置，常常是「感測器在 ZYNQ 板子上、AI 推論在有 GPU 的電腦上」，這時就要把兩個角色拆開。

### 1. 三個角色

| 角色 | 幾個 | 做什麼 | 預設埠 |
|---|---|---|---|
| `dora coordinator` | 整個叢集 1 個 | 收 CLI 指令、決定每個節點放哪台、轉發 log、記錄狀態 | TCP 6013（WebSocket） |
| `dora daemon` | **每台機器 1 個** | 啟動／監看本機節點、和其他 daemon 交換資料 | 53291（本機 local-listen）＋ zenoh 埠 |
| `dora` CLI | 任何能連到 coordinator 的機器 | `start`、`list`、`logs`、`stop` | — |

重要觀念：

- **控制訊息走 coordinator，資料不走 coordinator。** 每個 daemon 開一條 WebSocket 連到 coordinator，只傳控制訊息；節點之間的資料走 [zenoh](https://zenoh.io/)，能直連就節點對節點直連，連不上就自動退回「節點 → daemon → daemon → 節點」。
- 同一台機器上的節點之間仍然用共享記憶體（第 10 篇講的零拷貝），跨機器才走網路。
- coordinator 預設只聽 `127.0.0.1`。要讓別台機器連進來，必須用 `--interface <本機網卡IP>`。

### 2. 啟動順序（真實多機版）

假設 coordinator 那台電腦是 `192.168.1.10`、ZYNQ 板子是 `192.168.1.20`：

```bash
# 每台要下指令的機器都先設好，start/list/logs/stop 就不用每次帶 --coordinator-addr
export DORA_COORDINATOR_ADDR=192.168.1.10

# 機器 1（工作站）：coordinator + 自己的 daemon
dora coordinator --interface 192.168.1.10
dora daemon --coordinator-addr 192.168.1.10 --machine-id workstation --labels role=compute

# 機器 2（ZYNQ 板）
dora daemon --coordinator-addr 192.168.1.10 --machine-id edge

# 任何一台：啟動 dataflow
dora start dataflow.yml --name demo --detach
```

daemon 會自己從「連到 coordinator 用的那張網卡」推出要對外公布的位址，啟動時會印：

```
INFO dora_daemon::zenoh_bind: zenoh listener binding 172.30.0.2 (derived from coordinator address 172.30.0.2:6013); this port accepts connections from other hosts
```

**看到 `127.0.0.1` 就要小心**：代表 daemon 是透過 loopback 連上 coordinator 的，別台機器找不到它。官方文件也提醒：`dora up` 啟動的 daemon 沒有名字，`deploy.machine` 指不到它；要多機部署就自己分開啟動 coordinator 和 daemon。

### 3. 在 YAML 裡指定節點放哪台：`deploy`

1.0 版之後鍵名是 **`deploy`**（舊版的 `_unstable_deploy` 已經移除，寫了會直接報錯，不會被默默忽略）。

```yaml
- id: sensor
  deploy:
    machine: edge          # 對應 daemon 的 --machine-id
- id: aggregator
  deploy:
    labels:                # 對應 daemon 的 --labels，daemon 的標籤要「包含」這些
      role: compute
```

`deploy` 的欄位（讀 `libraries/message/src/descriptor.rs` 的 `Deploy` 結構確認）：

| 欄位 | 說明 |
|---|---|
| `machine` | 指定機器 ID，優先權最高 |
| `labels` | 標籤條件；coordinator 挑**第一個**標籤是超集合的 daemon |
| `working_dir` | 節點在目標機器上的工作目錄 |
| `distribute` | 執行檔怎麼送到遠端：`local`（預設，每台自己 build）、`scp`、`http` |

排程優先順序：**machine > labels > 都沒寫**。都沒寫的節點會放到「沒有 `--machine-id` 的 daemon」上；如果每個 daemon 都有名字，這種節點就沒地方放。

**路徑是在目標機器上解析的。** `path: target/release/sensor` 是相對路徑，在多機情況下每台都要有同樣的目錄結構，或改用絕對路徑／`working_dir`。dora 預設**不會**幫你把執行檔複製過去（第 15 篇交叉編譯會再處理這件事）。

### 4. `dora cluster`：用一個檔案管整個叢集

手動在每台開 daemon 適合開發。正式環境可以寫 `cluster.yml`，用 SSH 一次開關：

```yaml
coordinator:
  addr: 192.168.1.10
machines:
  - id: workstation
    host: 192.168.1.10
    labels: { role: compute }
  - id: edge
    host: 192.168.1.20
    user: petalinux
```

```bash
dora cluster up cluster.yml     # SSH 到每台啟動 daemon
dora cluster status             # 看每個 daemon 的心跳
dora cluster install cluster.yml   # 裝成 systemd 服務，開機自動啟動
dora cluster down
```

`dora cluster up/install` 需要能免密碼 SSH 到每台，這台虛擬機只有一台機器，**本篇沒有實測 `cluster up`**，只實測了 `dora cluster status`（手動開的 daemon 也看得到）。

### 5. 跨機器一定會遇到的事

- **網路**：coordinator 機器的 TCP 6013 要開；daemon 之間要能互相連 TCP（zenoh）。不同網段或 NAT 後面，官方建議先用 Tailscale／WireGuard 這類 VPN 讓每台都有可互連的位址；或用 zenoh router（`--zenoh-config-overlay`）。
- **時鐘**：本篇範例用 metadata 裡的送出時間算延遲。同一台電腦沒問題，**兩台機器要先用 chrony 或 PTP 對時**，不然算出來的延遲可能是負的。
- **coordinator 狀態**：`dora coordinator` 預設 `--store redb`，狀態存檔，重開還在；開發時可用 `--store memory`（本篇用這個）。
- **安全**：`--auth` 會讓 coordinator 產生 token，寫到 `~/.config/dora/.dora-token`，沒有 token 的 CLI 連不上。在公司或公開網路上建議打開（本篇沒有測）。

## 完整範例

位置：[examples/lesson-14/](../../examples/lesson-14/)

```
examples/lesson-14/
├── Cargo.toml
├── dataflow.yml            # sensor/logger → edge，aggregator 用 labels 排到 role=compute
├── dataflow-pinned.yml     # 對照組：aggregator 改用 machine: gpu-box
├── dataflow-missing.yml    # 故意指定沒上線的機器 zynq
├── run-demo.sh             # 依序跑三個情境
└── src/
    ├── lib.rs              # whoami()、now_ns()
    └── bin/
        ├── sensor.rs
        ├── aggregator.rs
        └── logger.rs
```

資料流向：`sensor`（edge）→ `aggregator`（compute）→ `logger`（edge），資料會跨機器來回一趟。

### Cargo.toml

```toml
[package]
name = "lesson-14"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
```

### src/lib.rs

```rust
//! 三個節點共用的小工具：印出「我在哪個 daemon 底下」。
use std::time::{SystemTime, UNIX_EPOCH};

/// 節點啟動時印一行身分：node id、PID、父行程 PID（= 啟動它的 dora daemon）。
/// 同一台電腦上跑兩個 daemon 時，看父 PID 就知道節點被排到哪一個。
pub fn whoami(name: &str, node_id: &str) {
    let host = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "?".into());
    eprintln!(
        "[{name}] 啟動 node_id={node_id} host={host} pid={} 父pid(daemon)={}",
        std::process::id(),
        std::os::unix::process::parent_id()
    );
}

/// 目前時間（Unix 奈秒）。跨機器比較前要先做時間同步（chrony/PTP）。
pub fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}
```

### src/bin/sensor.rs

```rust
//! 感測節點（部署在 edge 機器）：每個 tick 送一筆假溫度，metadata 帶 seq 和送出時間。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_14::{now_ns, whoami};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    whoami("sensor", &node.id().to_string());
    let out = DataId::from("temp".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                // 假溫度：25 度上下擺動
                let temp = 25.0 + ((seq % 10) as f64 - 5.0) * 0.3;
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                p.insert("sent_ns".into(), Parameter::Integer(now_ns()));
                node.send_output(out.clone(), p, vec![temp])?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[sensor] 結束，共送出 {seq} 筆");
    Ok(())
}
```

### src/bin/aggregator.rs

```rust
//! 彙整節點（用 labels 排到 role=compute 的機器）：算最近 5 筆平均，
//! 量「sensor 送出 → 這裡收到」的延遲，把結果字串送回 edge。
use std::collections::VecDeque;

use dora_node_api::{
    DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId, get_integer_param,
    into_vec,
};
use lesson_14::{now_ns, whoami};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    whoami("aggregator", &node.id().to_string());
    let out = DataId::from("summary".to_owned());
    let mut window: VecDeque<f64> = VecDeque::with_capacity(5);
    let (mut n, mut lat_sum_us, mut lat_max_us) = (0u64, 0i64, 0i64);

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "temp" => {
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                let sent = get_integer_param(&metadata.parameters, "sent_ns").unwrap_or(0);
                let lat_us = (now_ns() - sent) / 1000;
                n += 1;
                lat_sum_us += lat_us;
                lat_max_us = lat_max_us.max(lat_us);

                for v in into_vec::<f64>(&data)? {
                    if window.len() == 5 {
                        window.pop_front();
                    }
                    window.push_back(v);
                }
                let avg = window.iter().sum::<f64>() / window.len() as f64;
                let text = format!("seq={seq} avg5={avg:.2} 延遲={lat_us}µs");
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                node.send_output(out.clone(), p, text.as_str())?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    if n > 0 {
        eprintln!(
            "[aggregator] 收到 {n} 筆，平均延遲 {}µs，最大 {lat_max_us}µs",
            lat_sum_us / n as i64
        );
    }
    Ok(())
}
```

### src/bin/logger.rs

```rust
//! 記錄節點（部署回 edge 機器）：把 aggregator 的結果印出來。
use dora_node_api::{DoraNode, Event};
use lesson_14::whoami;

fn main() -> eyre::Result<()> {
    let (node, mut events) = DoraNode::init_from_env()?;
    whoami("logger", &node.id().to_string());
    let mut count = 0u64;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "summary" => {
                let s: &str = (&data).try_into()?;
                count += 1;
                println!("[logger] {s}");
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[logger] 結束，共收到 {count} 筆");
    Ok(())
}
```

### dataflow.yml

```yaml
# 第 14 篇：一條 dataflow 拆到兩個 daemon（edge / compute）
nodes:
  - id: sensor
    deploy:
      machine: edge            # 指定機器：daemon 用 --machine-id edge 啟動
    build: cargo build --release --bin sensor
    path: target/release/sensor
    inputs:
      tick: dora/timer/millis/200
    outputs:
      - temp

  - id: aggregator
    deploy:
      labels:                  # 用標籤挑機器：daemon 用 --labels role=compute 啟動
        role: compute
    build: cargo build --release --bin aggregator
    path: target/release/aggregator
    inputs:
      temp: sensor/temp
    outputs:
      - summary

  - id: logger
    deploy:
      machine: edge
    build: cargo build --release --bin logger
    path: target/release/logger
    inputs:
      summary: aggregator/summary
```

`dataflow-pinned.yml` 只差在 aggregator 那段改成 `deploy: { machine: gpu-box }`；`dataflow-missing.yml` 把 sensor 改成 `machine: zynq`。

### run-demo.sh

```bash
#!/usr/bin/env bash
# 假設 coordinator 與兩個 daemon（edge、gpu-box）已在跑；依序跑三個情境。
set -u
export DORA_COORDINATOR_ADDR=${DORA_COORDINATOR_ADDR:-127.0.0.1}
mkdir -p run
for f in dataflow dataflow-pinned; do
  echo "=== $f.yml ==="
  dora start $f.yml --name $f --detach
  sleep 10
  ps -o pid,ppid,comm -C sensor,aggregator,logger
  dora stop $f
  sleep 2
done
echo "=== dataflow-missing.yml（應該失敗）==="
dora start dataflow-missing.yml --name missing --detach
echo "結束碼 $?"
```

## 實際執行

```bash
cd examples/lesson-14   # 自 repo 根目錄
cargo build --release                       # 或 dora build dataflow.yml
IP=172.30.0.2                               # 這台虛擬機的網卡位址（真實環境換成區網 IP）

# 終端機 1：coordinator（狀態放記憶體）
dora coordinator --interface $IP --store memory
# 終端機 2：「edge 機器」
dora daemon --coordinator-addr $IP --machine-id edge
# 終端機 3：「GPU 機器」——同一台電腦開第二個 daemon，要換 local-listen 埠
dora daemon --coordinator-addr $IP --machine-id gpu-box \
            --labels role=compute,arch=x86_64 --local-listen-port 53292

# 終端機 4
export DORA_COORDINATOR_ADDR=$IP
dora cluster status
./run-demo.sh
```

### 實測輸出（2026-11-14，台北時間）

`dora cluster status`：兩個 daemon 都在，ID 是「machine-id + UUID」：

```
DAEMON ID                                     LAST HEARTBEAT  RESTARTS ...
edge-01a1d74d-a516-748c-a327-b9c13a4a5fed     3869ms ago      0
gpu-box-01a1d74d-a516-748c-a327-b9c25539ed04  3869ms ago      0
```

`dora start` 時 CLI 會即時印出每個 daemon 的動作，可以直接看到節點被分到哪裡：

```
INFO   logger on daemon `edge`: spawner spawning `.../target/release/logger` in `.../_work/01a1d74e-...`
INFO   sensor on daemon `edge`: spawner spawning `.../target/release/sensor` in ...
INFO   [dora] on daemon `edge`: daemon all local nodes are ready (exit before subscribe: []), waiting for remote nodes
INFO   aggregator on daemon `gpu-box`: spawner spawning `.../target/release/aggregator` in ...
```

用 `ps` 看父行程：edge daemon 的 PID 是 1984311、gpu-box daemon 是 1984312：

```
    PID    PPID COMMAND
1985664 1984311 logger
1985666 1984311 sensor
1985715 1984312 aggregator      ← labels role=compute 選中了 gpu-box
```

各節點的 log（labels 版，跑約 10 秒）：

```
[aggregator] 啟動 node_id=aggregator host=grok-bot-vm-153097534 pid=1985715 父pid(daemon)=1984312
[sensor] 啟動 node_id=sensor host=grok-bot-vm-153097534 pid=1985666 父pid(daemon)=1984311
[logger] seq=49 avg5=25.60 延遲=851µs
...
[aggregator] 收到 51 筆，平均延遲 605µs，最大 1079µs
[sensor] 結束，共送出 51 筆
[logger] 結束，共收到 51 筆
```

machine 指定版（`dataflow-pinned.yml`）：`收到 48 筆，平均延遲 588µs，最大 1098µs`，送 48 收 48。兩版延遲差不多，因為其實在同一台電腦上；真實跨機器時延遲會加上網路時間。

故意指定沒上線的機器：

```
[ERROR]
no matching daemon for machine id `zynq`
結束碼 1
```

這個錯在 `dora start` 時就擋下，不會只啟動一半。

### 實測發現的三個坑

1. **用 labels 指定時，daemon 間的直連會慢一拍。** labels 版啟動時，edge daemon 印出警告（machine 指定版沒有）：
   ```
   WARN dora_daemon::spawn::endpoint_exchange: no zenoh endpoint for remote node(s) [NodeId("aggregator")] after 1.5s;
   their outputs will reach this daemon's nodes over the daemon path instead of directly ...
   Their daemon is chosen by labels, so whether it is reachable at all cannot be told from here
   ```
   意思是：aggregator 的輸出改走「經過 daemon 轉送」的路，不是節點直連；資料沒有遺失，但多了一跳，啟動也多等 1.5 秒。在意延遲的節點建議用 `machine` 指定。
2. **dataflow 結束後，labels 排的節點 `dora logs` 讀不到。** `dora logs <UUID> --node aggregator` 在 machine 版正常；labels 版結束後會報 `no matching daemon connections for machine ID 'None'`（執行中用 `dora logs <名稱> --node aggregator` 是讀得到的）。替代方法是直接看 daemon 那台的 log 檔：`_work/<build-id>/out/<dataflow-uuid>/log_aggregator.jsonl`（每行一筆 JSON）。
3. **節點的工作目錄是 `_work/<build-id>/`，不是 YAML 所在的資料夾。** 節點要讀相對路徑的檔案（模型、設定檔）時要注意，或用 `deploy.working_dir` 指定。另外 coordinator／daemon 會在目前目錄寫 `out/`，自己的 log 不要也放 `out/`，會混在一起（本篇的 log 放在 `run/`）。

收尾：關掉 coordinator 和兩個 daemon（Ctrl+C 或 `kill`），再 `rm -rf target` 清掉約 1.4 GB 的編譯產物。

## 小練習

1. **加第三台機器**：再開一個 daemon `--machine-id display --local-listen-port 53293`，把 logger 改成 `deploy: { machine: display }`，用 `ps` 確認三個節點分別在三個 daemon 底下。
2. **標籤沒人符合**：把 aggregator 的 labels 改成 `role: gpu`（沒有 daemon 有這個標籤），先猜 `dora start` 會印什麼、結束碼是多少，再實際跑一次確認（官方文件說的訊息是 `no daemon matches labels {"gpu": "true"}` 這種格式）。
3. **進階（有兩台機器的話）**：在另一台 Linux（或第 15 篇之後的 ZYNQ 板）上跑 `dora daemon --coordinator-addr <你的IP> --machine-id edge`，把 sensor、logger 放過去，觀察 `延遲=` 變多少；兩台時間沒對準時，延遲會不會變成負數？

下一篇：[第 15 篇](lesson-15.md)講交叉編譯：把節點編成 aarch64，放到 ZYNQ 上當 edge daemon。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 13 篇](lesson-13.md)
- 下一課：[第 15 篇 →](lesson-15.md)
- 本課範例：[examples/lesson-14/](../../examples/lesson-14/)
