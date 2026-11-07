use dora_node_api::{DoraNode, MetadataParameters};
use std::{thread::sleep, time::Duration};

fn main() -> eyre::Result<()> {
    // 沒有 inputs 的純來源節點也要 init；events 用不到但要留著
    let (mut node, _events) = DoraNode::init_from_env()?;

    for n in 1..=6u64 {
        println!("ticker 送出 count={n}");
        node.send_output("count".into(), MetadataParameters::default(), n)?;
        sleep(Duration::from_millis(150));
    }
    println!("ticker 結束（共送出 6 次）");
    Ok(())
}
