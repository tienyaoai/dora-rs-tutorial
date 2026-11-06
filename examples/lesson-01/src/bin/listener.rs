use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } => {
                // data 是 Arrow 陣列，這裡轉回 &str
                let text: &str = TryFrom::try_from(&data)?;
                println!("listener 收到 [{id}]：{text}");
            }
            Event::InputClosed { id } => println!("輸入 {id} 已關閉"),
            Event::Stop(_) => println!("收到停止訊號"),
            _ => {}
        }
    }
    println!("listener 結束");
    Ok(())
}
