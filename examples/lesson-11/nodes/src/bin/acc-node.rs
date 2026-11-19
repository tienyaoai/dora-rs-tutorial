//! 與 op-acc 相同邏輯，寫成 Node。
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("[node acc] pid={}", std::process::id());
    let mut total: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "value" => {
                total += i64::try_from(&data)?;
                node.send_output("total".into(), MetadataParameters::default(), total)?;
            }
            Event::InputClosed { id } if id.as_str() == "value" => break,
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
