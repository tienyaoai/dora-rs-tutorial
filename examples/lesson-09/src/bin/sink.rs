use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut got: Vec<u64> = Vec::new();
    let mut restarts = 0u32;
    let mut failures = 0u32;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "result" => {
                let v = u64::try_from(&data)?;
                got.push(v / 10);
            }
            // 上游依 restart_policy 重啟成功
            Event::NodeRestarted { id } => {
                restarts += 1;
                println!("sink 通知：上游 {id} 已重啟（第 {restarts} 次），已收 {} 筆", got.len());
            }
            // 上游以非 0 結束碼退出
            Event::NodeFailed { source_node_id, error, affected_input_ids } => {
                failures += 1;
                println!("sink 通知：上游 {source_node_id} 失敗，影響輸入 {affected_input_ids:?}：{error}");
            }
            Event::InputClosed { id } => println!("sink 輸入 {id} 已關閉"),
            Event::Stop(cause) => {
                println!("sink 收到 Stop({cause:?})");
                break;
            }
            _ => {} // Event 是 non_exhaustive，不認得的事件就忽略
        }
    }
    println!("sink 總結：收到 {} 筆 {:?}；NodeRestarted {restarts} 次、NodeFailed {failures} 次", got.len(), got);
    Ok(())
}
