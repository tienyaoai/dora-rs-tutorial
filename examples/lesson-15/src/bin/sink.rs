//! 接收端節點：印出收到的內容，同時印自己的架構——兩邊架構不同就代表真的跨機器／跨架構了。
use dora_node_api::{DoraNode, Event, get_integer_param};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    eprintln!("{}", lesson_15::report("sink"));
    let (mut count, mut last_seq) = (0u64, 0i64);
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "info" => {
                let s: &str = (&data).try_into()?;
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                if count == 0 || seq % 10 == 0 {
                    println!("[sink] 收到 {s}");
                }
                count += 1;
                last_seq = seq;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[sink] 結束，共收到 {count} 筆，最後 seq={last_seq}");
    Ok(())
}
