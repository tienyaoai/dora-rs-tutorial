//! 記錄節點（部署回 edge 機器）：把 aggregator 的結果印出來。
use dora_node_api::{DoraNode, Event};
use lesson_14::whoami;

fn main() -> eyre::Result<()> {
    let (node, mut events) = DoraNode::init_from_env()?;
    whoami("logger", &node.id().to_string());
    let mut count = 0u64;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "summary" => {
                let s: &str = (&data).try_into()?;
                count += 1;
                println!("[logger] {s}");
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[logger] 結束，共收到 {count} 筆");
    Ok(())
}
