# dora-rs 教學 第 12 篇：混用 Python 節點——Rust 與 Python 節點在同一個 dataflow 裡互傳 Arrow 資料

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0；Python 端 `dora-rs` 1.1.0、pyarrow 26.0.1、numpy 2.5.4、Python 3.13.5）　驗證日期：2026-11-12（台北時間），只在這台 x86_64 Linux 虛擬機上實測。2 份 YAML 的 `dora validate` 都通過；`node.yml`（Python 寫成 node）和 `operator.yml`（Python 寫成 operator）用 `dora run --stop-after 3s` 跑，結束碼都是 0，Python 端各處理 25 筆，Rust 接收端 `scaled`、`summary` 各收到 25 筆、自我檢查 0 筆不符。另外**刻意示範失敗**：不啟用 venv 直接跑 `node.yml`，結束碼 1（`pip install` 被系統 Python 擋下）。第一次 release 建置約 1 分 59 秒。

> **本 repo 說明：** Python venv 建在 `examples/lesson-12/.venv/`（已在 `.gitignore`，不納入版控）。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 11 篇](lesson-11.md)
- 下一課：[第 13 篇 →](lesson-13.md)
- 本課範例：[examples/lesson-12/](../../examples/lesson-12/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [小練習](#小練習)
## 重點說明

前 11 篇全部是 Rust。但實際專案裡常常有一段是 Python：現成的 numpy／OpenCV 前處理、PyTorch 模型、同事寫好的分析腳本。dora 的設計是**節點之間只傳 Arrow 陣列**，所以 Rust 節點和 Python 節點可以直接接在一起，中間不用 JSON、不用自己序列化。這篇我們做一條來回的管線：

```
dora/timer(100ms) → producer（Rust） → scale（Python, numpy） → sink（Rust, 自我檢查）
                     Float32×8 + metadata      Float32×8 + metadata
                                               StructArray（int64/float64/字串/List）
```

### 1. YAML 裡的 Python 節點

```yaml
  - id: scale
    build: pip install -r py/requirements.txt   # 可省略；dora run/build 時會先跑
    path: py/scale.py                            # 副檔名是 .py → 用 python 執行
    inputs:
      samples: producer/samples
    outputs:
      - scaled
      - summary
```

- `path:` 指向 `.py` 檔，dora 就會用 `python -u 檔案` 啟動它（`-u` 讓 stdout 不緩衝，log 才會即時出現）。Rust 節點與 Python 節點在 YAML 裡寫法完全一樣，連線語法也一樣（`節點/輸出`）。
- **用哪一個 python？** 讀 v1.1.0 原始碼（`libraries/core/src/lib.rs` 的 `get_python_path`）：先問 `uv python find`（如果機器上有 uv；它會尊重 `VIRTUAL_ENV`），找不到才用 `PATH` 上的 `python`。這台機器有 uv，啟用 venv 後實測 log 是：
  ```
  scale: spawner spawning: ".../.venv/bin/python" -u .../examples/lesson-12/py/scale.py
  ```
  沒啟用 venv 時，`uv python find` 回的是 `/usr/bin/python3`，那裡沒有裝 `dora` 套件。所以**最簡單穩定的做法：先 `source venv/bin/activate` 再 `dora run`**。
- `build: pip install ...` 用的是當下 `PATH` 上的 `pip`。啟用 venv 時它只會印一串 `Requirement already satisfied`；沒啟用 venv 時，Debian 系的系統 Python 會用 PEP 668 拒絕安裝，整個 dataflow 在建置階段就失敗（見實測輸出的反例）。
- 另外有 `dora run --uv`／`dora build --uv`，讓 dora 用 uv 幫節點建管理式環境。**這個模式本篇沒有實測**。

### 2. Python Node API（`dora-rs` 1.1.0）

```python
from dora import Node
import pyarrow as pa

node = Node()                       # 等同 Rust 的 DoraNode::init_from_env()
for event in node:                  # 等同 while let Some(event) = events.recv()
    if event["type"] == "INPUT":    # 另有 "STOP"、"INPUT_CLOSED"、"ERROR"
        event["id"]                 # 輸入名稱（str），例如 "samples"
        event["value"]              # pyarrow.Array（不是 numpy、也不是 list）
        event["metadata"]           # dict
        node.send_output("scaled", pa.array([...]), {"seq": 1})
    elif event["type"] == "STOP":
        break
```

- `send_output(output_id, data, metadata=None)`：`data` 給 pyarrow 陣列（`pa.array(numpy陣列)`、`pa.StructArray` 都可以）；metadata 是一般 dict。
- 想用 numpy：`event["value"].to_numpy(zero_copy_only=True)`。數值型別、沒有 null 時可以不複製；本篇的 Float32 陣列用 `zero_copy_only=True` 實測沒有拋錯。
- 還有 `node.next(timeout=…)`、`node.drain()`、`recv_async()` 與零拷貝寫入的 `send_output_raw()`（只送位元組，見原始碼文件），本篇沒用到。

### 3. Rust ↔ Python 型別對照（本篇實測）

**資料（Arrow 陣列）**

| Rust 端 | Arrow 型別 | Python 端 |
|---|---|---|
| `Vec<f32>`（`send_output` 直接給） | `Float32` | `pyarrow.FloatArray`，`arr.type` 印出 `float` |
| 讀 `&[f32]`（`TryFrom`） | `Float32` | `pa.array(np.float32 陣列)` |
| `Int64Array` | `Int64` | `pa.array([1], type=pa.int64())`（Python int 預設就是 int64） |
| `Float64Array` | `Float64` | `pa.array([2.75])`（Python float = f64） |
| `StringArray` | `Utf8` | `pa.array(["volt#1"])` |
| `ListArray` → 子陣列 `Int64Array` | `List(Int64)` | `pa.array([np.int64 陣列], type=pa.list_(pa.int64()))` |
| `StructArray` | `Struct([...])` | `pa.StructArray.from_arrays([...], names=[...])` |

注意兩個坑：

1. **型別要完全吻合**（第 5 篇講過，Rust 不會幫你轉型）。numpy 很容易不知不覺變成 `float64`：本篇用 `x * np.float32(gain)` 保持 float32；如果寫成 `np.array(python_list) * 2.0`，你會得到 `Float64`，Rust 端用 `&[f32]` 讀就會失敗（這是小練習）。
2. pyarrow 建出來的欄位預設 `nullable: true`（sink 印出的 Struct 型別可以看到），Rust 端解開時要記得可能有 null；本篇資料沒有 null，所以直接 `.value(0)`、`.values()`。

**metadata（`Parameter` ↔ Python）**

| Rust `Parameter` | Python（實測收到的型別） |
|---|---|
| `Integer(i64)` | `int` |
| `Float(f64)` | `float` |
| `String` | `str` |
| `Bool`、`ListInt`、`ListFloat`、`ListString`、`Timestamp` | `bool`、`list`、`datetime`（讀原始碼得知，本篇沒逐一實測） |

- Python → Rust 方向也一樣：本篇 Python 送出的 `{"seq": int, "mean": float, "unit": str, "t_send_ns": int, "by": str}`，Rust 用 `get_integer_param`、`get_string_param`、`match Parameter::Float` 都讀得到。
- 原始碼（`apis/python/operator/src/lib.rs` 的 `pydict_to_metadata`）顯示：list 的型別是**看第一個元素**決定（int → `ListInt`、float → `ListFloat`、str → `ListString`），空 list 會變成 `ListString([])`。所以 `[1, 2.5]` 這種混型清單要小心。
- Python 端收到的 metadata dict **多一個 `timestamp` 鍵**（實測型別 `datetime`，由 dora 的 HLC 時間戳轉來）。這就是為什麼 `scale.py` 只挑需要的鍵轉送，而不是把整包 `event["metadata"]` 丟回去——整包轉送時 `timestamp` 會被當成你自己的參數再送一次（依原始碼會變成 `Parameter::Timestamp`；我沒有實測這條路）。

### 4. 效能與 GIL

- **每個 Python node 是獨立行程**，各有自己的直譯器與 GIL，不會和別的節點搶 GIL。但在同一個節點裡，`for event in node` 是單執行緒的：一筆沒處理完，下一筆就排隊。
- **Python operator 不是「多個共用一個直譯器」**：v1.1.0 原始碼（`binaries/daemon/src/spawn/runtime_registry.rs`）明寫一個 runtime 只支援一個 Python operator，理由是 PyO3 還不支援 sub-interpreter。所以和第 11 篇的結論一樣：operator 在 v1.1.0 也是「一個一行程」。
- 實測的記憶體（`node.yml` 跑的時候 `ps` 單次快照）：`producer` 17616 KB、`sink` 17740 KB、Python 的 `scale.py` **85840 KB**（直譯器＋pyarrow＋numpy）。在 ZYNQ 這類記憶體有限的板子上，一個 Python 節點大約抵五個 Rust 節點。
- 來回延遲（Rust 送出 → Python 算完送回 → Rust 收到，用 metadata 裡的 `t_send_ns` 計算，100 ms 一筆、各 25 筆、單次量測）：node 版中位數 1667 µs、最大 13360 µs；operator 版中位數 2048 µs、最大 11490 µs。之前的 PoC（官方 benchmark）量到節點對節點小訊息延遲中位數約 60～200 µs，兩者量法不同，不能直接相除，但量級差很明顯。
- operator 版的 runtime 行程又出現第 11 篇那行 `failed to create zenoh SHM provider ... falling back to heap-buffered publishes` 警告，Python node 版沒有。原因一樣**還沒查清楚**；本篇每筆資料都小於 4096 位元組零拷貝門檻，結果不受影響。

### 5. 什麼時候把熱路徑留在 Rust

- **留在 Rust**：每筆都要處理、頻率高（>100 Hz）、資料大（影像、點雲）、要求穩定延遲、或要跑在記憶體小的嵌入式板上。
- **放 Python 沒關係**：頻率低（幾 Hz～幾十 Hz）、核心計算本來就在 numpy／PyTorch 的 C 程式裡、還在快速試驗演算法、或是只有 Python 才有的套件。
- 實用的切法：**Rust 做「收資料、切幀、過濾、打包」**，把整批資料一次丟給 Python；Python 用 numpy 向量化一次算完，不要在 Python 裡對每個元素寫 `for` 迴圈。等演算法穩定了，再把這個節點用 Rust 重寫——因為介面就是 Arrow＋metadata，上下游完全不用改。

## 完整範例

位置：[examples/lesson-12/](../../examples/lesson-12/)。`producer` 每 100 ms 送出第 seq 筆 `[seq, seq+0.5, …, seq+3.5]`（8 個 f32），metadata 帶 `seq`、`unit`、`gain=2.0`、`t_send_ns`；Python 用 numpy 乘上 gain、算平均，送回兩個輸出；`sink` 檢查：

- `scaled[i] == 2 × (seq + 0.5i)`、metadata 的 `mean == seq + 1.75`、`by == "python"`、seq 連號；
- `summary` 的 `seq`、`mean`、`label == "volt#seq"`、`rounded == [2seq, 2seq+1, …, 2seq+7]`。

```
examples/lesson-12/
├── Cargo.toml
├── src/bin/
│   ├── producer.rs       # Rust 來源節點
│   └── sink.rs           # Rust 接收端（自我檢查）
├── py/
│   ├── requirements.txt
│   ├── scale.py          # Python node
│   └── op_scale.py       # 同邏輯的 Python operator
├── node.yml              # producer → scale.py（node）→ sink
└── operator.yml          # producer → op_scale.py（operator）→ sink
```

### 環境準備（Python venv）

```bash
cd examples/lesson-12   # 自 repo 根目錄
python3 -m venv .venv
./.venv/bin/pip install dora-rs==1.1.0 pyarrow numpy
# 實測裝到：dora-rs 1.1.0、pyarrow 26.0.1、numpy 2.5.4
```

> `pip install dora-rs` 的版本一定要和 CLI 一致（1.1.0），版本不合時節點和 daemon 之間的訊息格式可能不相容。

### Cargo.toml

```toml
[package]
name = "lesson-12"
version = "0.1.0"
edition = "2024"

[dependencies]
# arrow-v59：讓 sink 能用 dora 內部那一份 Arrow 解開 StructArray / ListArray（第 5 篇）
dora-node-api = { version = "1.1.0", features = ["arrow-v59"] }
eyre = "0.6"
```

### src/bin/producer.rs

```rust
//! Rust 來源節點：每個 tick 送出 8 個 f32 取樣點（Float32 陣列），
//! 並在 metadata 附上 seq、unit、gain 與送出時間（Unix 奈秒）。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("[producer] pid={}", std::process::id());
    let output = DataId::from("samples".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                // 第 seq 筆：seq, seq+0.5, …, seq+3.5（這些值在 f32 裡都能精確表示）
                let samples: Vec<f32> = (0..8).map(|i| seq as f32 + i as f32 * 0.5).collect();
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                p.insert("unit".into(), Parameter::String("volt".into()));
                p.insert("gain".into(), Parameter::Float(2.0));
                p.insert("t_send_ns".into(), Parameter::Integer(now_ns()));
                if seq == 1 {
                    println!("[producer] 第 1 筆 samples={samples:?}");
                }
                node.send_output(output.clone(), p, samples)?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[producer] 總共送出 {seq} 筆");
    Ok(())
}
```

### src/bin/sink.rs

```rust
//! Rust 接收端：收 Python 算完的 scaled（Float32）與 summary（StructArray），
//! 逐筆自我檢查，結束時印出總結與來回延遲。
use dora_node_api::{
    DoraNode, Event, Parameter,
    arrow_v59::array::{Array, Float64Array, Int64Array, ListArray, StringArray, StructArray},
    get_integer_param, get_string_param,
};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    println!("[sink] pid={}", std::process::id());
    let (mut n_scaled, mut n_summary, mut bad) = (0i64, 0i64, 0i64);
    let mut rtt_us: Vec<f64> = Vec::new();

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "scaled" => {
                n_scaled += 1;
                let p = &metadata.parameters;
                let seq = get_integer_param(p, "seq").unwrap_or(-1);
                let x: &[f32] = TryFrom::try_from(&data)?; // 零拷貝借用 Float32 陣列
                let mean = match p.get("mean") {
                    Some(Parameter::Float(m)) => *m,
                    _ => f64::NAN,
                };
                let mut ok = seq == n_scaled && x.len() == 8;
                for (i, v) in x.iter().enumerate() {
                    ok &= *v == 2.0 * (seq as f32 + i as f32 * 0.5);
                }
                ok &= mean == seq as f64 + 1.75;
                ok &= get_string_param(p, "by") == Some("python");
                if let Some(t0) = get_integer_param(p, "t_send_ns") {
                    rtt_us.push((now_ns() - t0) as f64 / 1000.0);
                }
                if !ok {
                    bad += 1;
                    println!("[sink] ✗ scaled 第 {n_scaled} 筆不符：seq={seq} x={x:?} mean={mean}");
                }
                if n_scaled == 1 {
                    println!("[sink] scaled 型別={} 長度={}", data.type_name(), data.len());
                    let mut keys: Vec<&String> = p.keys().collect();
                    keys.sort();
                    println!("[sink] scaled 的 metadata 鍵={keys:?}");
                }
                if n_scaled <= 2 || n_scaled % 10 == 0 {
                    println!("[sink] 第 {n_scaled} 筆 seq={seq} mean={mean} scaled={x:?}");
                }
            }
            Event::Input { id, data, .. } if id.as_str() == "summary" => {
                n_summary += 1;
                // Python 的 pa.StructArray → Rust 的 StructArray
                let s = StructArray::from(data.as_array().to_data());
                let col = |name: &str| s.column_by_name(name).unwrap().clone();
                let seq = col("seq").as_any().downcast_ref::<Int64Array>().unwrap().value(0);
                let mean = col("mean").as_any().downcast_ref::<Float64Array>().unwrap().value(0);
                let label_col = col("label");
                let label = label_col.as_any().downcast_ref::<StringArray>().unwrap().value(0);
                let list_col = col("rounded");
                let list = list_col.as_any().downcast_ref::<ListArray>().unwrap();
                let inner = list.value(0); // 第 0 列的子陣列（Int64）
                let rounded: Vec<i64> = inner
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .unwrap()
                    .values()
                    .to_vec();
                let expect: Vec<i64> = (0..8).map(|i| (2 * seq) + i).collect();
                let ok = seq == n_summary
                    && mean == seq as f64 + 1.75
                    && label == format!("volt#{seq}")
                    && rounded == expect;
                if !ok {
                    bad += 1;
                    println!("[sink] ✗ summary 第 {n_summary} 筆不符：{seq} {mean} {label} {rounded:?}");
                }
                if n_summary == 1 {
                    println!("[sink] summary 型別={}", data.type_name());
                    println!("[sink] summary 第 1 筆 seq={seq} mean={mean} label={label} rounded={rounded:?}");
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    rtt_us.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = rtt_us.get(rtt_us.len() / 2).copied().unwrap_or(f64::NAN);
    let max = rtt_us.last().copied().unwrap_or(f64::NAN);
    println!(
        "[sink] 總結：scaled {n_scaled} 筆、summary {n_summary} 筆、不符 {bad} 筆；\
         Rust→Python→Rust 來回延遲 中位數 {med:.0} µs、最大 {max:.0} µs"
    );
    Ok(())
}
```

### py/requirements.txt

```
dora-rs==1.1.0
pyarrow
numpy
```

### py/scale.py（Python node）

```python
"""Python 節點：收 Rust 送來的 Float32 陣列，用 numpy 放大、算平均，再送回 Rust。"""

import os

import numpy as np
import pyarrow as pa
from dora import Node


def main():
    node = Node()
    print(f"[scale.py] pid={os.getpid()} pyarrow={pa.__version__} numpy={np.__version__}")
    n = 0
    for event in node:
        if event["type"] == "INPUT" and event["id"] == "samples":
            n += 1
            arr = event["value"]  # pyarrow.FloatArray（Arrow Float32）
            meta = event["metadata"]  # dict：Rust 的 Parameter 已轉成 Python 型別
            if n == 1:
                print(f"[scale.py] value 型別={type(arr).__name__} arrow={arr.type} 長度={len(arr)}")
                print(
                    "[scale.py] metadata="
                    + str({k: (type(v).__name__, v) for k, v in sorted(meta.items()) if k != "timestamp"})
                )
                print(f"[scale.py] 另有 timestamp 鍵，型別={type(meta['timestamp']).__name__}")

            x = arr.to_numpy(zero_copy_only=True)  # float32、無 null → 不複製
            scaled = x * np.float32(meta["gain"])  # 保持 float32（NumPy 2 規則）
            mean = float(x.mean())  # Python float = f64
            seq = meta["seq"]

            # ① 陣列 + metadata（只挑要轉傳的鍵，不要整包 meta 丟回去）
            node.send_output(
                "scaled",
                pa.array(scaled),  # numpy float32 → Arrow Float32
                {"seq": seq, "mean": mean, "unit": meta["unit"],
                 "t_send_ns": meta["t_send_ns"], "by": "python"},
            )
            # ② 一列的結構化資料：int64 / float64 / 字串 / List<int64>
            summary = pa.StructArray.from_arrays(
                [
                    pa.array([seq], type=pa.int64()),
                    pa.array([mean], type=pa.float64()),
                    pa.array([f"{meta['unit']}#{seq}"]),
                    pa.array([np.rint(scaled).astype(np.int64)], type=pa.list_(pa.int64())),
                ],
                names=["seq", "mean", "label", "rounded"],
            )
            node.send_output("summary", summary, {"seq": seq})
        elif event["type"] == "STOP":
            break
    print(f"[scale.py] 總共處理 {n} 筆")


if __name__ == "__main__":
    main()
```

### py/op_scale.py（同邏輯的 Python operator）

```python
"""同一段邏輯寫成 Python operator（呼應第 11 篇）：由 dora runtime 載入，呼叫 on_event。"""

import os

import numpy as np
import pyarrow as pa
from dora import DoraStatus


class Operator:
    def __init__(self):
        self.n = 0
        print(f"[op_scale.py] pid={os.getpid()}")

    def on_event(self, dora_event, send_output) -> DoraStatus:
        if dora_event["type"] == "INPUT" and dora_event["id"] == "samples":
            self.n += 1
            meta = dora_event["metadata"]
            x = dora_event["value"].to_numpy(zero_copy_only=True)
            scaled = x * np.float32(meta["gain"])
            mean = float(x.mean())
            seq = meta["seq"]
            send_output(
                "scaled",
                pa.array(scaled),
                {"seq": seq, "mean": mean, "unit": meta["unit"],
                 "t_send_ns": meta["t_send_ns"], "by": "python"},
            )
            summary = pa.StructArray.from_arrays(
                [
                    pa.array([seq], type=pa.int64()),
                    pa.array([mean], type=pa.float64()),
                    pa.array([f"{meta['unit']}#{seq}"]),
                    pa.array([np.rint(scaled).astype(np.int64)], type=pa.list_(pa.int64())),
                ],
                names=["seq", "mean", "label", "rounded"],
            )
            send_output("summary", summary, {"seq": seq})
        return DoraStatus.CONTINUE

    def __del__(self):
        print(f"[op_scale.py] 總共處理 {self.n} 筆")
```

和第 11 篇的 Rust operator 一樣：沒有 `Node()`、沒有事件迴圈，dora runtime 建立 `Operator()` 後對每個事件呼叫 `on_event(dora_event, send_output)`，回傳 `DoraStatus.CONTINUE`／`STOP`。

### node.yml

```yaml
nodes:
  - id: producer
    build: cargo build --release --bin producer
    path: target/release/producer
    inputs:
      tick: dora/timer/millis/100
    outputs:
      - samples

  # Python 節點：path 指向 .py，dora 會用找到的 python 直譯器 `python -u` 執行它
  - id: scale
    build: pip install -r py/requirements.txt
    path: py/scale.py
    inputs:
      samples: producer/samples
    outputs:
      - scaled
      - summary

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    inputs:
      scaled: scale/scaled
      summary: scale/summary
```

### operator.yml

```yaml
nodes:
  - id: producer
    build: cargo build --release --bin producer
    path: target/release/producer
    inputs:
      tick: dora/timer/millis/100
    outputs:
      - samples

  # Python operator：一個 runtime 節點一個 operator（v1.1.0 限制，見第 11 篇）
  - id: scale-rt
    operators:
      - id: scale
        python: py/op_scale.py
        inputs:
          samples: producer/samples
        outputs:
          - scaled
          - summary

  - id: sink
    build: cargo build --release --bin sink
    path: target/release/sink
    inputs:
      scaled: scale-rt/scale/scaled      # 三段式：節點/operator/輸出
      summary: scale-rt/scale/summary
```

Python operator 在 YAML 裡用 `python:`（取代 Rust operator 的 `shared-library:`）。

### 執行

```bash
export PATH="$HOME/dora/bin:$PATH"   # 依你的 dora CLI 安裝位置調整
source .venv/bin/activate      # 重要：讓 dora 找到裝了 dora-rs 的 python
cd examples/lesson-12   # 自 repo 根目錄
cargo build --release
dora validate node.yml                            # 兩份都顯示 OK
dora validate operator.yml
dora run node.yml     --stop-after 3s; echo "exit=$?"
dora run operator.yml --stop-after 3s; echo "exit=$?"
```

### 實測輸出（節錄）

`dora validate`（兩份相同）：

```
Validating node.yml...
Input/output wiring OK.
Descriptor config OK.
All type annotations OK.
```

`node.yml`（結束碼 0，整個 run 約 3.9 秒）：

```
scale: stdout   Requirement already satisfied: dora-rs==1.1.0 in .../.venv/lib/python3.13/site-packages ...
INFO   scale: spawner spawning: ".../.venv/bin/python" -u .../examples/lesson-12/py/scale.py
stdout producer: [producer] pid=1796906
stdout producer: [producer] 第 1 筆 samples=[1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5]
stdout sink: [sink] pid=1796918
stdout scale: [scale.py] pid=1796907 pyarrow=26.0.1 numpy=2.5.4
stdout scale: [scale.py] value 型別=FloatArray arrow=float 長度=8
stdout scale: [scale.py] metadata={'gain': ('float', 2.0), 'seq': ('int', 1), 't_send_ns': ('int', 1794440932452211307), 'unit': ('str', 'volt')}
stdout scale: [scale.py] 另有 timestamp 鍵，型別=datetime
stdout sink: [sink] scaled 型別=Float32 長度=8
stdout sink: [sink] scaled 的 metadata 鍵=["by", "mean", "seq", "t_send_ns", "unit"]
stdout sink: [sink] 第 1 筆 seq=1 mean=2.75 scaled=[2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]
stdout sink: [sink] summary 型別=Struct([Field { name: "seq", data_type: Int64, nullable: true }, Field { name: "mean", data_type: Float64, nullable: true }, Field { name: "label", data_type: Utf8, nullable: true }, Field { name: "rounded", data_type: List(Field { data_type: Int64, nullable: true }), nullable: true }])
stdout sink: [sink] summary 第 1 筆 seq=1 mean=2.75 label=volt#1 rounded=[2, 3, 4, 5, 6, 7, 8, 9]
stdout sink: [sink] 第 2 筆 seq=2 mean=3.75 scaled=[4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]
stdout sink: [sink] 第 10 筆 seq=10 mean=11.75 scaled=[20.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0, 27.0]
stdout sink: [sink] 第 20 筆 seq=20 mean=21.75 scaled=[40.0, 41.0, 42.0, 43.0, 44.0, 45.0, 46.0, 47.0]
INFO dora_daemon: stop-after timeout reached (3s) -> stopping dataflow
stdout producer: [producer] 總共送出 25 筆
stdout scale: [scale.py] 總共處理 25 筆
stdout sink: [sink] 總結：scaled 25 筆、summary 25 筆、不符 0 筆；Rust→Python→Rust 來回延遲 中位數 1667 µs、最大 13360 µs
INFO   scale: daemon scale finished successfully
```

`operator.yml`（結束碼 0）：

```
INFO dora_daemon::spawn::runtime_registry: spawning: python -uc import dora; dora.start_runtime() # scale-rt
INFO   scale-rt: spawner spawning `.../.venv/bin/python` in `.../examples/lesson-12`
stdout scale-rt: [op_scale.py] pid=1797117
stdout scale-rt: WARN dora_node_api::node: failed to create zenoh SHM provider (Unable to create POSIX shm segment: OS error 12 ...); falling back to heap-buffered publishes
...
stdout sink: [sink] 總結：scaled 25 筆、summary 25 筆、不符 0 筆；Rust→Python→Rust 來回延遲 中位數 2048 µs、最大 11490 µs
stdout producer: [producer] 總共送出 25 筆
stdout scale-rt: WARN dora_runtime_api: runtime received Stop; forwarding to 1 operator(s) (dora-rs/dora#2742 diagnostic)
stdout scale-rt: WARN dora_runtime_api: forwarded Stop to operator `scale` (dora-rs/dora#2742 diagnostic)
stdout scale-rt: [op_scale.py] 總共處理 25 筆
```

反例：**沒有啟用 venv**（`PATH` 裡只有系統 Python）直接跑 `node.yml`，結束碼 1：

```
scale: stdout   error: externally-managed-environment
...
[ERROR]
failed to build dataflow before run
Caused by:
   0: failed to build node `scale`
   1: build command failed
   2: build command `pip install -r py/requirements.txt` returned exit status: 1
```

（如果把 `build:` 拿掉，建置會過，但節點會用沒有 `dora` 套件的 `/usr/bin/python3` 啟動——這個變化我沒有另外跑，留作練習思考。）

觀察重點：

1. Rust 的 `Vec<f32>` 到 Python 是 `pyarrow.FloatArray`（Arrow `Float32`），Python 用 `pa.array(np.float32 陣列)` 送回，Rust 用 `&[f32]` 零拷貝借用——**兩邊都沒寫任何序列化程式碼**。
2. metadata 雙向都通：Rust 的 `Integer/Float/String` 在 Python 是 `int/float/str`；Python 的 dict 回到 Rust 又是 `Parameter`。Python 端多出來的 `timestamp` 鍵是系統加的，我們沒有轉送，所以 sink 看到的鍵只有 5 個。
3. Python node 與 Python operator 結果完全相同（25 筆、0 筆不符），operator 版的 Python 由 `python -uc "import dora; dora.start_runtime()"` 啟動，再載入 `op_scale.py`。
4. Python 行程的 RSS 約是 Rust 節點的 5 倍，來回延遲在毫秒級——低頻的計算放 Python 完全沒問題，但高頻熱路徑請留在 Rust。

## 小練習

把 `py/scale.py` 裡的

```python
scaled = x * np.float32(meta["gain"])
```

改成「先轉 Python list 再算」的常見寫法：

```python
scaled = np.array(x.tolist()) * meta["gain"]
```

然後：

1. 跑 `dora run node.yml --stop-after 3s`，看 `sink` 印出什麼錯誤、`dora run` 的結束碼是多少？（提示：`np.array(list_of_float)` 預設是 `float64`。）
2. 不改 Python，改 Rust：讓 `sink` 先檢查 `data.data_type()`，`Float64` 時用 `Vec<f64>` 讀、`Float32` 時用 `&[f32]` 讀，兩種都能通過檢查。哪一種改法比較好？為什麼「在介面上講好型別」比「接收端什麼都吃」更適合機器人系統？
3. 進階：把 timer 改成 `dora/timer/millis/10`（100 Hz），比較 node 版的來回延遲中位數與最大值有沒有變化。

下一篇：[第 13 篇](lesson-13.md)：測試與除錯——幫節點寫單元測試、讀懂 dora 的 log 與常見錯誤。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 11 篇](lesson-11.md)
- 下一課：[第 13 篇 →](lesson-13.md)
- 本課範例：[examples/lesson-12/](../../examples/lesson-12/)
