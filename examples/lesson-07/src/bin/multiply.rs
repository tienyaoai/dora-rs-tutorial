use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "value" => {
                let n = u64::try_from(&data)?;
                let scaled = n * 10;
                println!("multiply 收到 value={n} → 送出 scaled={scaled}");
                node.send_output("scaled".into(), MetadataParameters::default(), scaled)?;
            }
            Event::InputClosed { id } => {
                println!("multiply 輸入 {id} 已關閉，準備退出");
                break;
            }
            Event::Stop(cause) => {
                println!("multiply 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("multiply 其他事件：{other:?}"),
        }
    }
    println!("multiply 結束");
    Ok(())
}
