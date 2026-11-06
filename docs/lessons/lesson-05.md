# dora-rs 教學 第 5 篇：Arrow 資料與 metadata——除了純量，還能送什麼？

> 版本：dora v1.1.0（CLI 與 `dora-node-api` 同版）　驗證日期：2026-11-05（台北時間），x86_64 Linux 上實際 `dora validate` / `dora build` / `dora run` 成功（結束碼 0）。


**導覽：** [返回教材總覽](../../README.md) · [← 第 4 篇](lesson-04.md) · [第 6 篇 →](lesson-06.md) · **範例：** [examples/lesson-05/](../../examples/lesson-05/)
## 目錄

- [重點說明](#重點說明)
- [完整可執行範例：producer → consumer](#完整可執行範例producer-consumer)
- [小練習](#小練習)
## 重點說明

前幾篇我們送的都是一個數字或一句字串。真實的節點要送的是一整段波形、一張影像、一張偵測結果表。這一篇講清楚兩件事：**資料（data）怎麼用 Arrow 表示**，以及**附帶說明（metadata）怎麼用**。

### 1. 為什麼是 Arrow

dora 節點之間傳的 payload 一律是 [Apache Arrow](https://arrow.apache.org/) 陣列：一種**有型別、欄式（columnar）、跨語言**的記憶體格式。好處是 Rust、Python、C++ 節點讀同一份資料不用再做格式轉換；資料夠大時，還可以走共享記憶體零拷貝（第 10 篇）。

兩個觀念先記住：

- **每則訊息就是「一個」Arrow 陣列**。一個 `f64` 純量是長度 1 的 `Float64` 陣列；`Vec<f32>` 是長度 N 的 `Float32` 陣列；一張表是 `StructArray`（每個欄位各是一個等長陣列）。
- dora v1.1 在 API 上刻意**不直接暴露 Arrow 型別**：收到的是 `DoraArray`，送的時候只要實作 `IntoArrow`。需要碰 Arrow 本體（例如自己建 `StructArray`）才要開 feature。

### 2. 三層用法：從簡單到完整

| 需求 | 送的時候 | 收的時候 |
|---|---|---|
| 純量、字串 | 直接丟 `f64`、`u32`、`bool`、`&str`、`String`… | `f64::try_from(&data)?`、`&str::try_from(&data)?` |
| 一整段同型別數值 | 丟 `Vec<T>`（`T` 是 `u8`…`u64`、`i8`…`i64`、`f32`、`f64`） | `Vec<T>::try_from(&data)?`（複製），或 `&[T]::try_from(&data)?`（**零拷貝借用**） |
| 結構化資料（表格、偵測框） | 自己建 `StructArray`，用 `DoraArray::from_array(...)` 包起來 | `StructArray::from(data.as_array().to_data())`，再依欄名取出各欄 |
| 只想傳 metadata、沒有資料 | 丟 `()` | 長度 0 的 `Null` 陣列，忽略 data 即可 |

要使用第三列，`Cargo.toml` 要開 feature：

```toml
dora-node-api = { version = "1.1.0", features = ["arrow-v59"] }
```

開了之後就能用 `dora_node_api::arrow_v59::…`（它是 dora 內部那一份 Arrow 的重新匯出，不會多編一份，也不會與你專案裡另一版 `arrow` 打架）。

### 3. 讀取時的「嚴格規則」

`TryFrom<&DoraArray>` 全部回 `Result`，**型別不對不會 panic**，這讓節點可以自己決定要警告還是丟棄：

- **純量**（`f64`、`i32`、`bool`、`&str`…）要求陣列**剛好 1 個元素且沒有 null**，否則是錯誤。
- **`Vec<T>` / `&[T]`** 長度不拘，但**有 null 一律拒絕**。
- **型別必須完全吻合**：`Float64` 陣列不能當 `i32` 讀（不會幫你轉型）。
- 字串三種 Arrow 編碼（`Utf8`、`LargeUtf8`、`Utf8View`）都能讀，所以 Python（pyarrow／polars）送來的字串也 OK。
- `data.type_name()`、`data.len()`、`data.null_count()` 可以在不解開資料的情況下檢查，很適合做入口驗證與除錯日誌。

### 4. metadata：資料的「便條紙」

每則訊息除了 data，還帶一個 `Metadata`：

- **系統填的**：`timestamp()`（混合邏輯時鐘 HLC 時間戳，跨機器可比較先後）、`metadata_version()`（線上格式版本，v1.1.0 是 2）。
- **你填的**：`metadata.parameters`，型別是 `BTreeMap<String, Parameter>`。

`Parameter` 目前有這些變體：`Bool`、`Integer(i64)`、`Float(f64)`、`String`、`ListInt`、`ListFloat`、`ListString`、`Timestamp`（UTC 時間）。

什麼該放 metadata、什麼該放 data？經驗法則：**每一筆都會變、而且量大的 → data；描述這筆資料的小標籤 → metadata**。例如影像的寬、高、編碼格式、幀編號、單位、感測器 ID，都適合放 metadata。

讀取有三個「型別安全」的 getter：`get_string_param`、`get_integer_param`、`get_bool_param`——**型別不符與鍵不存在都回 `None`**，所以你分不出是哪一種，需要區分時請直接 `p.get("key")` 再 `match` 變體。清單類（`ListFloat` 等）沒有 getter，也是直接 `match`。

另外 dora 有幾個**約定俗成的鍵**，自己取名時要避開或善加利用：`seq`、`fin`、`session_id`、`segment_id`（串流分段用）、`request_id`、`goal_id`、`goal_status`（service / action 模式用）。還有以 `__dora_` 開頭的鍵是系統保留的。

### 5. 常見地雷

- **把 `Vec<f64>` 長度 1 當純量讀 → 可以**（長度 1 就是純量）；但**長度 ≠ 1 的當純量讀 → 錯誤**。協定上最好固定：這個輸出永遠是純量，或永遠是 Vec。
- 結構化資料要把「欄名」當作介面契約，雙方約好；欄位型別（`UInt32` vs `Int64`）寫錯，`downcast_ref` 會回 `None`，記得處理而不是 `unwrap`（範例為了簡短用了 `unwrap`，正式程式請改成 `ok_or_else`）。
- 沒開 `arrow-v59` feature 就找不到 `arrow_v59`，編譯器會直接告訴你。

## 完整可執行範例：producer → consumer

```
examples/lesson-05/
├── Cargo.toml
├── dataflow.yml
└── src/bin/
    ├── producer.rs
    └── consumer.rs
```

`Cargo.toml`：

```toml
[package]
name = "lesson-05"
version = "0.1.0"
edition = "2024"

[dependencies]
# arrow-v59 這個 feature 會把 dora 內部使用的 Arrow 以 dora_node_api::arrow_v59 重新匯出
dora-node-api = { version = "1.1.0", features = ["arrow-v59"] }
eyre = "0.6"
```

`dataflow.yml`：

```yaml
nodes:
  - id: producer
    description: 一次示範四種送法：純量＋metadata、Vec<f32>、結構化表格、只有 metadata
    build: cargo build --bin producer
    path: target/debug/producer
    outputs:
      - temperature
      - waveform
      - table
      - ping

  - id: consumer
    description: 把四種資料各自讀回來，並展示型別不對時的錯誤
    build: cargo build --bin consumer
    path: target/debug/consumer
    inputs:
      temperature: producer/temperature
      waveform: producer/waveform
      table: producer/table
      ping: producer/ping
```

`src/bin/producer.rs`：

```rust
use dora_node_api::{
    DoraArray, DoraNode, MetadataParameters, Parameter,
    arrow_v59::{
        array::{Float64Array, StringArray, StructArray, UInt32Array},
        datatypes::{DataType, Field},
    },
    dora_core::config::DataId,
};
use std::sync::Arc;

fn id(s: &str) -> DataId {
    DataId::from(s.to_owned())
}

fn main() -> eyre::Result<()> {
    let (mut node, _events) = DoraNode::init_from_env()?;

    // ① 純量 + metadata：值放 data，「附帶說明」放 metadata
    let mut params = MetadataParameters::new();
    params.insert("unit".into(), Parameter::String("celsius".into()));
    params.insert("seq".into(), Parameter::Integer(1));
    params.insert("valid".into(), Parameter::Bool(true));
    params.insert("calibration".into(), Parameter::ListFloat(vec![0.98, 1.5]));
    params.insert("tags".into(), Parameter::ListString(vec!["lab".into(), "bench-2".into()]));
    node.send_output(id("temperature"), params, 23.5f64)?;
    println!("producer ①：送出純量 23.5（附 5 個 metadata 參數）");

    // ② 一整段數值：Vec<f32> 就是一個長度 8 的 Float32 陣列
    let wave: Vec<f32> = (0..8).map(|i| (i as f32 * 0.8).sin()).collect();
    node.send_output(id("waveform"), MetadataParameters::new(), wave)?;
    println!("producer ②：送出 Vec<f32>（8 個取樣點）");

    // ③ 結構化資料：3 欄 × 3 列的表格（StructArray，每個欄位是一個 Arrow 陣列）
    let columns: Vec<(Arc<Field>, Arc<dyn dora_node_api::arrow_v59::array::Array>)> = vec![
        (
            Arc::new(Field::new("id", DataType::UInt32, false)),
            Arc::new(UInt32Array::from(vec![1, 2, 3])),
        ),
        (
            Arc::new(Field::new("temp", DataType::Float64, false)),
            Arc::new(Float64Array::from(vec![21.5, 23.0, 25.5])),
        ),
        (
            Arc::new(Field::new("label", DataType::Utf8, false)),
            Arc::new(StringArray::from(vec!["door", "desk", "window"])),
        ),
    ];
    let table = StructArray::from(columns);
    // StructArray 不是 Rust 純量，要先包成 DoraArray 再送
    node.send_output(id("table"), MetadataParameters::new(), DoraArray::from_array(table))?;
    println!("producer ③：送出 StructArray（3 列 × 3 欄）");

    // ④ 只有 metadata、沒有資料：用 () 當 payload（空的 Null 陣列）
    let mut p = MetadataParameters::new();
    p.insert("note".into(), Parameter::String("我還活著".into()));
    node.send_output(id("ping"), p, ())?;
    println!("producer ④：送出 ()（只有 metadata）");

    Ok(())
}
```

`src/bin/consumer.rs`：

```rust
use dora_node_api::{
    DoraNode, Event, Parameter, arrow_v59::array::{Array, Float64Array, StringArray, StructArray, UInt32Array},
    get_bool_param, get_integer_param, get_string_param,
};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => {
                // 不用解開資料，就能看到它的 Arrow 型別與長度
                println!(
                    "── 輸入 {id}：型別 {}，長度 {}，null 數 {}",
                    data.type_name(),
                    data.len(),
                    data.null_count()
                );
                let p = &metadata.parameters;
                match id.as_str() {
                    "temperature" => {
                        let t = f64::try_from(&data)?;
                        println!("  值 = {t}");
                        // 型別安全的 getter：型別不符或鍵不存在都回 None
                        println!("  unit  = {:?}", get_string_param(p, "unit"));
                        println!("  seq   = {:?}", get_integer_param(p, "seq"));
                        println!("  valid = {:?}", get_bool_param(p, "valid"));
                        println!("  seq 當字串讀 = {:?}（型別不符 → None）", get_string_param(p, "seq"));
                        println!("  不存在的鍵   = {:?}", get_integer_param(p, "nope"));
                        // 清單類參數沒有 getter，直接 match 變體
                        if let Some(Parameter::ListFloat(c)) = p.get("calibration") {
                            println!("  calibration = {c:?}");
                        }
                        if let Some(Parameter::ListString(t)) = p.get("tags") {
                            println!("  tags = {t:?}");
                        }
                        // 型別錯誤示範：把 Float64 當 i32 讀
                        match i32::try_from(&data) {
                            Ok(v) => println!("  （意外）當 i32 讀到 {v}"),
                            Err(e) => println!("  把 Float64 當 i32 讀 → 錯誤（不是 panic）：{e}"),
                        }
                        // 發送時間戳（HLC）也在 metadata 裡
                        println!("  metadata 版本 = {}", metadata.metadata_version());
                    }
                    "waveform" => {
                        let v: Vec<f32> = Vec::try_from(&data)?; // 複製成 Vec
                        let s: &[f32] = TryFrom::try_from(&data)?; // 或零拷貝借用 slice
                        println!("  Vec 長度 {}，slice 長度 {}，最大值 {:.3}", v.len(), s.len(),
                            s.iter().cloned().fold(f32::MIN, f32::max));
                        // 多元素陣列不能當純量讀
                        println!("  當純量 f32 讀 → {:?}", f32::try_from(&data).is_err());
                    }
                    "table" => {
                        // as_array() 借出底層 Arrow 陣列；StructArray::from 由 ArrayData 轉換
                        let table = StructArray::from(data.as_array().to_data());
                        println!("  欄位：{:?}", table.column_names());
                        let ids = table.column_by_name("id").unwrap()
                            .as_any().downcast_ref::<UInt32Array>().unwrap();
                        let temps = table.column_by_name("temp").unwrap()
                            .as_any().downcast_ref::<Float64Array>().unwrap();
                        let labels = table.column_by_name("label").unwrap()
                            .as_any().downcast_ref::<StringArray>().unwrap();
                        for i in 0..table.len() {
                            println!("  列 {i}: id={} temp={} label={}", ids.value(i), temps.value(i), labels.value(i));
                        }
                    }
                    "ping" => {
                        println!("  note = {:?}", get_string_param(p, "note"));
                    }
                    _ => {}
                }
            }
            Event::InputClosed { id } => println!("（輸入 {id} 已關閉）"),
            Event::Stop(c) => { println!("consumer 停止：{c:?}"); break; }
            _ => {}
        }
    }
    println!("consumer 結束");
    Ok(())
}
```

執行：

```bash
export PATH=$HOME/dora/bin:$PATH   # 依你安裝 dora CLI 的位置調整
cd examples/lesson-05              # 在 clone 本 repo 後，於 repo 根目錄執行
dora validate dataflow.yml
dora build dataflow.yml
dora run dataflow.yml
```

**實測輸出（2026-11-05 08:07 台北時間，`dora run` 結束碼 0；略去 dora 自己的 INFO 行與時間前綴，各節點輸出交錯順序每次可能略有不同）**：

```text
producer: producer ①：送出純量 23.5（附 5 個 metadata 參數）
consumer: ── 輸入 temperature：型別 Float64，長度 1，null 數 0
consumer:   值 = 23.5
consumer:   unit  = Some("celsius")
consumer:   seq   = Some(1)
consumer:   valid = Some(true)
consumer:   seq 當字串讀 = None（型別不符 → None）
consumer:   不存在的鍵   = None
consumer:   calibration = [0.98, 1.5]
consumer:   tags = ["lab", "bench-2"]
consumer:   把 Float64 當 i32 讀 → 錯誤（不是 panic）：not a primitive Int32Type array
consumer:   metadata 版本 = 2
producer: producer ②：送出 Vec<f32>（8 個取樣點）
consumer: ── 輸入 waveform：型別 Float32，長度 8，null 數 0
producer: producer ③：送出 StructArray（3 列 × 3 欄）
consumer:   Vec 長度 8，slice 長度 8，最大值 1.000
consumer:   當純量 f32 讀 → true
producer: producer ④：送出 ()（只有 metadata）
consumer: ── 輸入 table：型別 Struct([Field { name: "id", data_type: UInt32 }, Field { name: "temp", data_type: Float64 }, Field { name: "label", data_type: Utf8 }])，長度 3，null 數 0
consumer:   欄位：["id", "temp", "label"]
consumer:   列 0: id=1 temp=21.5 label=door
consumer:   列 1: id=2 temp=23 label=desk
consumer:   列 2: id=3 temp=25.5 label=window
consumer: ── 輸入 ping：型別 Null，長度 0，null 數 0
consumer:   note = Some("我還活著")
consumer: （輸入 ping 已關閉）
consumer: （輸入 table 已關閉）
consumer: （輸入 temperature 已關閉）
consumer: （輸入 waveform 已關閉）
consumer: consumer 停止：AllInputsClosed
consumer: consumer 結束
```

幾個值得看的點：

- `table` 的型別字串是 `Struct([...])`，可以看出每個欄位的名稱與型別；`ping` 的型別是 `Null`、長度 0，但 metadata 照樣送達。
- 把 `Float64` 當 `i32` 讀得到 `not a primitive Int32Type array`，是個普通的 `Err`。
- 四個輸入都關閉後，`consumer` 收到 `Stop(AllInputsClosed)`——因為這個資料流沒有計時器，所以不必自己 `break`（對照第 4 篇）。

> 編譯備註：第一次 `dora build` 約 1 分 20 秒（8 核；`arrow-v59` 不會多編一份 Arrow）。實驗完我已清掉 `target/`；你在自己的機器上練習完也建議 `cargo clean`。

## 小練習

1. 在 `producer` 把 `temperature` 改成送 `23i32`（整數），不改 `consumer`，執行後會發生什麼？錯誤訊息出現在哪個節點？（提示：`f64::try_from(&data)?` 的 `?` 會讓節點 `main` 回傳錯誤。）
2. 在 `table` 再加一欄 `ok`（`BooleanArray`，記得 `Field::new("ok", DataType::Boolean, false)`），並在 `consumer` 印出。
3. 進階：把 `calibration` 改成 `Parameter::Timestamp(...)`（需要加 `chrono` 依賴取得 `Utc::now()`），在 `consumer` 比較它與 `metadata.timestamp()` 的差異，想想為什麼 dora 還要自己維護一個 HLC 時間戳。

下一篇：[第 6 篇：`dora up / start / logs / down`——資料流的生命週期管理](lesson-06.md)。

---

**導覽：** [返回教材總覽](../../README.md) · [← 第 4 篇](lesson-04.md) · [第 6 篇 →](lesson-06.md) · **範例：** [examples/lesson-05/](../../examples/lesson-05/)
