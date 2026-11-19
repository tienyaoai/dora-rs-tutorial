//! 接收端：量測端到端延遲（送出時間戳 → 收到），並比較「借用」與「複製」讀取的成本。
//! READ=borrow：TryFrom 取得 &[u8]，直接讀共享記憶體，不複製
//! READ=copy  ：再呼叫 .to_vec() 複製一份到自己的堆積記憶體
use dora_node_api::{DoraNode, Event, Parameter};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Stat {
    latency: Vec<Duration>,
    read: Vec<Duration>,
    bad: u32,
    first: Option<Instant>,
    last: Option<Instant>,
}

fn pct(v: &[Duration], p: f64) -> Duration {
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let read_mode = std::env::var("READ").unwrap_or_else(|_| "borrow".into());
    let label = std::env::var("LABEL").unwrap_or_else(|_| "run".into());
    let mut stats: BTreeMap<usize, Stat> = BTreeMap::new();
    let mut keep: u64 = 0; // 讓編譯器不能把讀取最佳化掉

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "frame" => {
                // 1) 到達延遲：送出端在 metadata 裡蓋的 HLC 時間戳 → 現在
                let latency = metadata
                    .timestamp()
                    .get_time()
                    .to_system_time()
                    .elapsed()
                    .unwrap_or_default();
                let seq = match metadata.parameters.get("seq") {
                    Some(Parameter::Integer(s)) => *s as u64,
                    _ => 0,
                };
                // 2) 讀取：借用 &[u8]（零拷貝）或再複製一份
                let t0 = Instant::now();
                let bytes: &[u8] = TryFrom::try_from(&data)?;
                let size = bytes.len();
                let ok = if read_mode == "copy" {
                    let owned: Vec<u8> = bytes.to_vec();
                    keep = keep.wrapping_add(owned[size / 2] as u64);
                    check(&owned, seq)
                } else {
                    keep = keep.wrapping_add(bytes[size / 2] as u64);
                    check(bytes, seq)
                };
                let read = t0.elapsed();

                let s = stats.entry(size).or_default();
                let now = Instant::now();
                s.first.get_or_insert(now);
                s.last = Some(now);
                s.latency.push(latency);
                s.read.push(read);
                if !ok {
                    s.bad += 1;
                }
            }
            Event::InputClosed { id } => println!("sink 輸入 {id} 已關閉"),
            Event::Stop(_) => {}
            _ => {}
        }
    }

    println!("sink [{label}] READ={read_mode}（keep={keep}）");
    println!(
        "{:>8} {:>5} {:>11} {:>11} {:>11} {:>11} {:>5}",
        "size", "n", "lat p50", "lat p99", "read p50", "read MB/s", "bad"
    );
    let mut csv = String::from("label,size,n,lat_p50_us,lat_p99_us,read_p50_us,read_mb_s,bad\n");
    for (size, s) in stats.iter_mut() {
        s.latency.sort();
        s.read.sort();
        let read_p50 = pct(&s.read, 0.5);
        let mbps = if read_p50.is_zero() {
            f64::INFINITY
        } else {
            *size as f64 / read_p50.as_secs_f64() / 1e6
        };
        println!(
            "{:>8} {:>5} {:>11.1?} {:>11.1?} {:>11.1?} {:>11.0} {:>5}",
            size,
            s.latency.len(),
            pct(&s.latency, 0.5),
            pct(&s.latency, 0.99),
            read_p50,
            mbps,
            s.bad
        );
        csv += &format!(
            "{label},{size},{},{},{},{},{mbps:.0},{}\n",
            s.latency.len(),
            pct(&s.latency, 0.5).as_micros(),
            pct(&s.latency, 0.99).as_micros(),
            read_p50.as_micros(),
            s.bad
        );
    }
    if let Ok(path) = std::env::var("CSV") {
        std::fs::write(&path, csv)?;
        println!("sink 結果已寫入 {path}");
    }
    Ok(())
}

/// 抽查開頭、中間、結尾三個位元組是否符合 (seq + i) % 251
fn check(buf: &[u8], seq: u64) -> bool {
    if buf.is_empty() {
        return true;
    }
    [0, buf.len() / 2, buf.len() - 1]
        .iter()
        .all(|&i| buf[i] == ((seq as usize + i) % 251) as u8)
}
