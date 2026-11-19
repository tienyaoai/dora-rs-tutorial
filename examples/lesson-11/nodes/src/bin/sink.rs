//! 共用的接收端（一定是 Node）：印出累計和，結束時自我檢查。
//! 第 n 筆 total 應該等於 2+4+…+2n = n(n+1)。
use dora_node_api::{DoraNode, Event};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    println!("[sink] pid={}", std::process::id());
    let mut n: i64 = 0;
    let mut bad = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "total" => {
                n += 1;
                let total = i64::try_from(&data)?;
                if total != n * (n + 1) {
                    bad += 1;
                }
                if n <= 3 || n % 10 == 0 {
                    println!("[sink] 第 {n} 筆 total={total}");
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[sink] 總結：收到 {n} 筆，不符 n(n+1) 的 {bad} 筆");
    Ok(())
}
