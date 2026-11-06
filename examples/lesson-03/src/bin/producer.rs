use dora_node_api::{DoraNode, Event, IntoArrow, dora_core::config::DataId};
use std::env;

fn main() -> eyre::Result<()> {
    // 1) env：YAML 的 env 會變成這個行程的環境變數
    let greeting = env::var("GREETING").unwrap_or_else(|_| "（沒設 GREETING）".into());
    let max: u32 = env::var("MAX_MSGS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    // 2) args：YAML 的 args 會變成命令列參數（以空白分隔）
    let prefix = env::args().skip_while(|a| a != "--prefix").nth(1).unwrap_or_default();
    println!("producer 啟動：greeting={greeting:?} max={max} prefix={prefix:?}");

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("text".to_owned());
    let mut n = 0u32;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, .. } if id.as_str() == "tick" => {
                n += 1;
                let msg = format!("{prefix}{greeting} #{n}");
                node.send_output(out.clone(), metadata.parameters, msg.as_str().into_arrow())?;
                if n >= max {
                    break;
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
