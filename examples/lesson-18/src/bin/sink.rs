//! 收尾節點：量端到端延遲（producer 蓋的 t_ns → 收到）、吞吐、丟幀。前 WARMUP 張不計。
use dora_node_api::{DoraNode, Event, Parameter};
use lesson_18::{env_or, now_ns, pct};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let label = std::env::var("LABEL").unwrap_or_else(|_| "run".into());
    let warmup: usize = env_or("WARMUP", 100);
    let (mut lat, mut got, mut max_seq) = (Vec::<i64>::new(), 0usize, 0i64);
    let (mut first, mut last): (Option<Instant>, Option<Instant>) = (None, None);
    let mut bytes = 0usize;
    while let Some(ev) = events.recv() {
        if let Event::Input { metadata, data, .. } = ev {
            let arrive = now_ns();
            let d: &[u8] = TryFrom::try_from(&data)?;
            got += 1;
            if let Some(Parameter::Integer(s)) = metadata.parameters.get("seq") {
                max_seq = max_seq.max(*s);
            }
            if got > warmup {
                if let Some(Parameter::Integer(t)) = metadata.parameters.get("t_ns") {
                    lat.push(arrive - t);
                }
                first.get_or_insert(Instant::now());
                last = Some(Instant::now());
                bytes += d.len();
            }
        }
    }
    lat.sort();
    let secs = match (first, last) {
        (Some(a), Some(b)) => (b - a).as_secs_f64(),
        _ => 0.0,
    };
    let counted = lat.len();
    println!(
        "sink [{label}] 收到 {got} 張 / producer 最大 seq {max_seq}（丟 {}）；計入 {counted} 張：延遲 p50={:.1} µs  p99={:.1} µs  max={:.1} µs；吞吐 {:.1} 張/秒、{:.1} MB/s",
        max_seq - got as i64,
        pct(&lat, 0.5) as f64 / 1e3,
        pct(&lat, 0.99) as f64 / 1e3,
        pct(&lat, 1.0) as f64 / 1e3,
        counted as f64 / secs,
        bytes as f64 / secs / 1e6
    );
    Ok(())
}
