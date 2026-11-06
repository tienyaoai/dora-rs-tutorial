use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut got = 0u32;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } => {
                let text: &str = TryFrom::try_from(&data)?;
                got += 1;
                println!("consumer 收到 [{id}]：{text}");
            }
            Event::InputClosed { id } => println!("輸入 {id} 已關閉（共收到 {got} 則）"),
            Event::Stop(_) => println!("收到停止訊號"),
            _ => {}
        }
    }
    Ok(())
}
