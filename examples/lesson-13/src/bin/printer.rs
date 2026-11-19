//! 把 avg / alarm 印出來，dora logs 看得到。
use dora_node_api::{DoraNode, Event, get_integer_param, into_vec};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => {
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                match id.as_str() {
                    "avg" => println!("[printer] seq={seq} avg={:?}", into_vec::<f64>(&data)?),
                    "alarm" => {
                        let s: &str = (&data).try_into()?;
                        println!("[printer] ALARM {s}");
                    }
                    other => println!("[printer] 其他輸入 {other}"),
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    Ok(())
}
