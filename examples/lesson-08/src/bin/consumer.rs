use dora_node_api::{DoraNode, Event, StopCause};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    // 每筆模擬處理時間（毫秒）與預期總筆數，都由 YAML 的 env 決定
    let work_ms: u64 = std::env::var("WORK_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let expected: u64 = std::env::var("TOTAL").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let start = Instant::now();
    let mut received: Vec<u64> = Vec::new();
    let mut heartbeats = 0u32;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "data" => {
                let seq = u64::try_from(&data)?;
                let skipped = match received.last() {
                    Some(&prev) => seq.saturating_sub(prev + 1),
                    None => seq - 1,
                };
                let t = start.elapsed().as_millis();
                if skipped > 0 {
                    println!("consumer t={t:>5}ms 處理 seq={seq}（前面跳過 {skipped} 筆）");
                } else {
                    println!("consumer t={t:>5}ms 處理 seq={seq}");
                }
                received.push(seq);
                sleep(Duration::from_millis(work_ms)); // 模擬很慢的下游
            }
            Event::Input { id, .. } if id.as_str() == "heartbeat" => {
                heartbeats += 1;
                let t = start.elapsed().as_millis();
                println!("consumer t={t:>5}ms 心跳 #{heartbeats}（目前已收 {} 筆）", received.len());
            }
            Event::InputClosed { id } => {
                println!("consumer 輸入 {id} 已關閉");
            }
            Event::Stop(StopCause::AllInputsClosed) => {
                // 控制事件會「插隊」排在已緩衝的資料前面：此時佇列裡可能還有資料，
                // 所以不要 break，繼續 recv() 直到回傳 None 為止
                println!("consumer 收到 Stop(AllInputsClosed)，繼續清空佇列");
            }
            Event::Stop(cause) => {
                println!("consumer 收到 Stop({cause:?})，準備退出");
                break;
            }
            other => println!("consumer 其他事件：{other:?}"),
        }
    }

    // ---- 總結：用序號缺口看出丟了哪些 ----
    let mut gaps: Vec<String> = Vec::new();
    let mut prev = 0u64;
    for &s in &received {
        if s > prev + 1 {
            let (a, b) = (prev + 1, s - 1);
            gaps.push(if a == b { format!("{a}") } else { format!("{a}-{b}") });
        }
        prev = s;
    }
    if prev < expected {
        let (a, b) = (prev + 1, expected);
        gaps.push(if a == b { format!("{a}") } else { format!("{a}-{b}") });
    }
    let missing = expected - received.len() as u64;
    let t = start.elapsed().as_millis();
    println!(
        "consumer 總結：收到 {}/{expected} 筆，第一筆 seq={:?}，最後一筆 seq={:?}，遺失 {missing} 筆，缺口 [{}]，耗時 {t}ms",
        received.len(),
        received.first(),
        received.last(),
        gaps.join(", ")
    );
    Ok(())
}
