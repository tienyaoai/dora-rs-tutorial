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
