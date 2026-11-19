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
