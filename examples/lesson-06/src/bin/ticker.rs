use dora_node_api::{DoraNode, Event, MetadataParameters};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut n: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                n += 1;
                node.send_output("count".into(), MetadataParameters::default(), n)?;
                println!("ticker 送出 count={n}");
            }
            Event::Stop(cause) => {
                println!("ticker 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("ticker 其他事件：{other:?}"),
        }
    }
    println!("ticker 結束（共送出 {n} 次）");
    Ok(())
}
