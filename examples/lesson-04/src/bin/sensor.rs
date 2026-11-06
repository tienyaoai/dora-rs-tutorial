use dora_node_api::{DoraNode, MetadataParameters, dora_core::config::DataId};
use std::{thread::sleep, time::Duration};

fn main() -> eyre::Result<()> {
    // 沒有 inputs 的節點也要 init；events 這裡用不到，但要留著不能丟掉
    let (mut node, _events) = DoraNode::init_from_env()?;
    let out = DataId::from("reading".to_owned());

    for i in 1..=8u32 {
        let value = 20.0 + (i as f64) * 0.5; // 假裝是溫度
        println!("sensor 送出第 {i} 筆：{value}");
        // f64 實作了 IntoArrow，所以可以直接丟進 send_output
        node.send_output(out.clone(), MetadataParameters::default(), value)?;
        if i == 4 {
            println!("sensor 故意停頓 1.5 秒…");
            sleep(Duration::from_millis(1500));
        } else {
            sleep(Duration::from_millis(200));
        }
    }
    println!("sensor 結束");
    Ok(())
}
