use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut sum: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "scaled" => {
                let v = u64::try_from(&data)?;
                sum += v;
                println!("accumulate 收到 scaled={v} → 累計 total={sum}");
                node.send_output("total".into(), MetadataParameters::default(), sum)?;
            }
            Event::InputClosed { id } => {
                println!("accumulate 輸入 {id} 已關閉（最終 total={sum}），準備退出");
                break;
            }
            Event::Stop(cause) => {
                println!("accumulate 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("accumulate 其他事件：{other:?}"),
        }
    }
    println!("accumulate 結束（最終 total={sum}）");
    Ok(())
}
