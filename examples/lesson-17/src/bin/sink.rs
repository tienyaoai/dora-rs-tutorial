//! 下游：每個推論節點各算一份統計——張數、正確率、序號缺口（被丟掉的幀）、端到端延遲（擷取→收到結果）。
use dora_node_api::{DoraNode, Event, Parameter, get_integer_param, get_string_param};
use lesson_17::{Lat, now_ns};
use std::collections::BTreeMap;

#[derive(Default)]
struct Stat { n: u64, correct: u64, gaps: u64, last_seq: i64, e2e: Lat, infer_us: u64, batch_sum: u64 }

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    while let Some(event) = events.recv() {
        let Event::Input { id, metadata, data } = event else {
            if matches!(event, Event::Stop(_)) { break; }
            continue;
        };
        let pm = &metadata.parameters;
        let probs: &[f32] = (&data).try_into()?;
        let (seq, label, t_ns) = (get_integer_param(pm, "seq").unwrap_or(-1),
            get_integer_param(pm, "label").unwrap_or(-1), get_integer_param(pm, "t_ns").unwrap_or(0));
        let pred = get_integer_param(pm, "pred").unwrap_or(-1);
        let score = match pm.get("score") { Some(Parameter::Float(f)) => *f, _ => f64::NAN };
        let s = stats.entry(id.to_string()).or_insert_with(|| Stat { last_seq: -1, ..Default::default() });
        if s.n < 3 {
            println!("[sink] {id} 第 {} 筆：seq={seq} label={label} pred={pred} score={score:.3} backend={} 資料 {} 個 f32",
                s.n + 1, get_string_param(pm, "backend").unwrap_or("?"), probs.len());
        }
        if s.last_seq >= 0 && seq > s.last_seq + 1 { s.gaps += (seq - s.last_seq - 1) as u64; }
        s.last_seq = seq;
        s.n += 1;
        if pred == label { s.correct += 1; }
        s.e2e.push(now_ns().saturating_sub(t_ns as u64));
        s.infer_us += get_integer_param(pm, "infer_us").unwrap_or(0) as u64;
        s.batch_sum += get_integer_param(pm, "batch").unwrap_or(1) as u64;
    }
    for (id, s) in stats.iter_mut() {
        println!("[sink] {id:<14}：{} 張，正確 {}（{:.2}%），序號缺口 {}，平均批次 {:.2}，端到端 {}",
            s.n, s.correct, s.correct as f64 * 100.0 / s.n.max(1) as f64, s.gaps,
            s.batch_sum as f64 / s.n.max(1) as f64, s.e2e.summary());
    }
    Ok(())
}
