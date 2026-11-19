//! 邊緣端節點（要放到 ZYNQ）：每個 tick 送出一行「我是誰」的文字，metadata 帶 seq。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let me = lesson_15::report("sysinfo");
    eprintln!("{me}");
    let out = DataId::from("info".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                node.send_output(out.clone(), p, format!("seq={seq} {me}").as_str())?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[sysinfo] 結束，共送出 {seq} 筆");
    Ok(())
}
