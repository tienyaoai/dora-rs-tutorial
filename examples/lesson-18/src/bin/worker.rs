//! 處理節點：對每張影格做 filter，再送出同樣大小的結果。
//! MODE=alloc：每則訊息 vec![0; n]（配置）→ filter → send_output（Arrow 轉換，再複製一次）
//! MODE=raw  ：send_output_raw，filter 直接寫進 dora 配好的輸出緩衝區（沒有額外配置、沒有額外複製）
//! SLOW_US   ：每張額外忙等多少微秒，模擬更重的運算
use dora_node_api::{DoraNode, Event, MetadataParameters, dora_core::config::DataId};
use lesson_18::{busy_us, env_or, filter, pct};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mode = std::env::var("MODE").unwrap_or_else(|_| "raw".into());
    let slow: u64 = env_or("SLOW_US", 0);
    let warmup: usize = env_or("WARMUP", 100);
    let out = DataId::from("result".to_owned());
    let mut cost: Vec<i64> = Vec::new(); // 每張的處理時間（奈秒）
    let mut n = 0usize;
    while let Some(ev) = events.recv() {
        match ev {
            Event::Input { metadata, data, .. } => {
                let src: &[u8] = TryFrom::try_from(&data)?;
                let params: MetadataParameters = metadata.parameters.clone(); // seq、t_ns 原樣傳下去
                let t0 = Instant::now();
                match mode.as_str() {
                    "raw" => node.send_output_raw(out.clone(), params, src.len(), |dst| {
                        filter(src, dst)
                    })?,
                    "alloc" => {
                        let mut v = vec![0u8; src.len()];
                        filter(src, &mut v);
                        node.send_output(out.clone(), params, v)?
                    }
                    other => eyre::bail!("未知的 MODE={other}"),
                }
                busy_us(slow);
                n += 1;
                if n > warmup {
                    cost.push(t0.elapsed().as_nanos() as i64);
                }
            }
            _ => {}
        }
    }
    cost.sort();
    println!(
        "worker MODE={mode} 處理 {n} 張（暖機 {warmup} 張不計）：處理時間 p50={:.1} µs  p99={:.1} µs  max={:.1} µs",
        pct(&cost, 0.5) as f64 / 1e3,
        pct(&cost, 0.99) as f64 / 1e3,
        pct(&cost, 1.0) as f64 / 1e3
    );
    Ok(())
}
