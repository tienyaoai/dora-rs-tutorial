//! 測試用來源節點：每個 tick 送一個 Float64 值 seq*2（2, 4, 6, …），metadata 帶 seq。
//! 第 4 筆刻意送字串 "oops"，讓 smoother 練習「型別不對」的處理。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("value".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                if seq == 4 {
                    node.send_output(out.clone(), p, "oops")?;
                } else {
                    node.send_output(out.clone(), p, vec![seq as f64 * 2.0])?;
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[source] 送出 {seq} 筆");
    Ok(())
}
