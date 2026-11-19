//! 與 op-double 相同邏輯，但寫成獨立行程的 Node。
use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("[node double] pid={}", std::process::id());
    let mut count: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                count += 1;
                node.send_output("value".into(), MetadataParameters::default(), count * 2)?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
