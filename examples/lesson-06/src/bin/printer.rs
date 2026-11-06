use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "count" => {
                let n = u64::try_from(&data)?;
                println!("printer 收到 count={n}");
            }
            Event::InputClosed { id } => {
                println!("printer 輸入 {id} 已關閉");
            }
            Event::Stop(cause) => {
                println!("printer 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("printer 其他事件：{other:?}"),
        }
    }
    println!("printer 結束");
    Ok(())
}
