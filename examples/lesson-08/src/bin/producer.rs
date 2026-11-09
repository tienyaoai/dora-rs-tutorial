use dora_node_api::{DoraNode, Event, MetadataParameters};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    // 送幾筆後自行結束（預設 40），由 YAML 的 env 決定
    let total: u64 = std::env::var("TOTAL").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let start = Instant::now();
    let mut seq: u64 = 0;

    while let Some(event) = events.recv() {
        match event {
            // tick 是 dora 自己產生的計時器事件，不是我們的程式
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                let t = start.elapsed().as_millis();
                node.send_output("seq".into(), MetadataParameters::default(), seq)?;
                println!("producer t={t:>5}ms 送出 seq={seq}");
                if seq >= total {
                    break; // 計時器永遠不會關閉，來源節點要自己決定何時收工
                }
            }
            Event::Stop(cause) => {
                println!("producer 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("producer 其他事件：{other:?}"),
        }
    }
    let t = start.elapsed().as_millis();
    println!("producer 結束：共送出 {seq} 筆，耗時 {t}ms");
    Ok(())
}
