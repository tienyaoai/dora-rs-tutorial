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
