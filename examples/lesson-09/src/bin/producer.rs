use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let total: u64 = std::env::var("TOTAL").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let mut seq: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                node.send_output("seq".into(), MetadataParameters::default(), seq)?;
                println!("producer 送出 seq={seq}");
                if seq >= total {
                    break; // 計時器不會關閉，來源節點自己收工
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("producer 結束：共送出 {seq} 筆");
    Ok(())
}
