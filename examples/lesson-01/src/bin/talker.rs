use dora_node_api::{DoraNode, Event, IntoArrow, dora_core::config::DataId};

fn main() -> eyre::Result<()> {
    // 從 dora daemon 設定的環境變數初始化節點，拿到「發送端」與「事件串流」
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let output = DataId::from("greeting".to_owned());
    let mut count = 0u32;

    // 事件迴圈：一直收事件，直到資料流結束（recv 回傳 None）
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, .. } if id.as_str() == "tick" => {
                count += 1;
                let msg = format!("你好，dora！第 {count} 則");
                node.send_output(output.clone(), metadata.parameters, msg.as_str().into_arrow())?;
                if count == 5 {
                    break; // 送完 5 則就結束，listener 會收到 InputClosed
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
