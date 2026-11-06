use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { data, .. } => {
                let text: &str = TryFrom::try_from(&data)?;
                println!("printer 摘要：{text}");
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
