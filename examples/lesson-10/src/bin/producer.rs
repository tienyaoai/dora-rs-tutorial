//! 大資料來源：依序送出不同大小的「影格」，量測送出那一刻要花多少時間。
//! MODE=raw ：send_output_raw，直接把資料寫進 dora 配好的（共享記憶體）緩衝區
//! MODE=vec ：先在自己的 Vec<u8> 準備好資料，再交給 send_output（會多複製一次）
use dora_node_api::{
    DoraNode, MetadataParameters, Parameter, dora_core::config::DataId,
};
use std::time::{Duration, Instant};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 每筆資料的內容：第 i 個位元組 = (seq + i) % 251，接收端可以抽查驗證。
/// 先做一份比最大影格多 251 位元組的樣板，之後每筆只要從 seq % 251 開始切一段複製，
/// 這樣「產生資料」就只是一次 memcpy，量到的時間主要是搬資料本身。
fn template(max: usize) -> Vec<u8> {
    (0..max + 251).map(|i| (i % 251) as u8).collect()
}

fn main() -> eyre::Result<()> {
    let (mut node, _events) = DoraNode::init_from_env()?;
    let mode = std::env::var("MODE").unwrap_or_else(|_| "raw".into());
    let n: u64 = env_or("N", 100);
    let sizes: Vec<usize> = std::env::var("SIZES")
        .unwrap_or_else(|_| "1024,4096,65536,1048576,4194304".into())
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<Result<_, _>>()?;
    let frame = DataId::from("frame".to_owned());
    let tpl = template(*sizes.iter().max().unwrap_or(&0));

    println!(
        "producer MODE={mode} N={n} zero_copy_threshold={} 位元組",
        node.zero_copy_threshold()
    );

    let mut seq = 0u64;
    for &size in &sizes {
        let mut send_times = Vec::with_capacity(n as usize);
        for _ in 0..n {
            seq += 1;
            let mut params = MetadataParameters::default();
            params.insert("seq".into(), Parameter::Integer(seq as i64));
            let src = &tpl[(seq % 251) as usize..][..size];
            // 計時範圍＝「產生這一筆資料」＋「交給 dora」，兩種模式做的事一樣，只差搬幾次
            let t0 = Instant::now();
            match mode.as_str() {
                // 直接寫進 dora 配好的緩衝區（≥ 門檻時就是共享記憶體）：只搬 1 次
                "raw" => node.send_output_raw(frame.clone(), params, size, |out| {
                    out.copy_from_slice(src)
                })?,
                "vec" => {
                    // 先放進自己的 Vec（配置＋搬 1 次），send_output 再編碼進共享記憶體（再搬 1 次）
                    let owned: Vec<u8> = src.to_vec();
                    node.send_output(frame.clone(), params, owned)?
                }
                other => eyre::bail!("未知的 MODE={other}，請用 raw 或 vec"),
            }
            send_times.push(t0.elapsed());
            std::thread::sleep(Duration::from_millis(5)); // 間隔 5ms，量延遲而不是塞爆佇列
        }
        send_times.sort();
        let avg = send_times.iter().sum::<Duration>() / n as u32;
        println!(
            "producer size={:>8}  send() avg={:>10.1?}  p50={:>10.1?}  max={:>10.1?}",
            size,
            avg,
            send_times[send_times.len() / 2],
            send_times[send_times.len() - 1]
        );
        std::thread::sleep(Duration::from_millis(300)); // 讓下游清空再換大小
    }
    println!("producer 全部 {seq} 筆送完，結束");
    Ok(())
}
