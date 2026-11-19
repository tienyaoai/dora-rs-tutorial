//! 彙整節點（用 labels 排到 role=compute 的機器）：算最近 5 筆平均，
//! 量「sensor 送出 → 這裡收到」的延遲，把結果字串送回 edge。
use std::collections::VecDeque;

use dora_node_api::{
    DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId, get_integer_param,
    into_vec,
};
use lesson_14::{now_ns, whoami};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    whoami("aggregator", &node.id().to_string());
    let out = DataId::from("summary".to_owned());
    let mut window: VecDeque<f64> = VecDeque::with_capacity(5);
    let (mut n, mut lat_sum_us, mut lat_max_us) = (0u64, 0i64, 0i64);

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "temp" => {
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                let sent = get_integer_param(&metadata.parameters, "sent_ns").unwrap_or(0);
                let lat_us = (now_ns() - sent) / 1000;
                n += 1;
                lat_sum_us += lat_us;
                lat_max_us = lat_max_us.max(lat_us);

                for v in into_vec::<f64>(&data)? {
                    if window.len() == 5 {
                        window.pop_front();
                    }
                    window.push_back(v);
                }
                let avg = window.iter().sum::<f64>() / window.len() as f64;
                let text = format!("seq={seq} avg5={avg:.2} 延遲={lat_us}µs");
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                node.send_output(out.clone(), p, text.as_str())?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    if n > 0 {
        eprintln!(
            "[aggregator] 收到 {n} 筆，平均延遲 {}µs，最大 {lat_max_us}µs",
            lat_sum_us / n as i64
        );
    }
    Ok(())
}
