use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    // 兩個輸入都關閉後才收工（扇入的典型結束條件）
    let mut open_inputs = 2usize;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } => match id.as_str() {
                "raw" => {
                    let n = u64::try_from(&data)?;
                    println!("printer [raw]   直接來自 ticker：{n}");
                }
                "total" => {
                    let n = u64::try_from(&data)?;
                    println!("printer [total] 管線累計結果：{n}");
                }
                other => println!("printer 未知輸入 {other}"),
            },
            Event::InputClosed { id } => {
                println!("printer 輸入 {id} 已關閉");
                open_inputs = open_inputs.saturating_sub(1);
                if open_inputs == 0 {
                    println!("printer 所有輸入都關閉了，準備退出");
                    break;
                }
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
