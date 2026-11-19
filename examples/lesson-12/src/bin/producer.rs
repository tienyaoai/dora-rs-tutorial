//! Rust 來源節點：每個 tick 送出 8 個 f32 取樣點（Float32 陣列），
//! 並在 metadata 附上 seq、unit、gain 與送出時間（Unix 奈秒）。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    println!("[producer] pid={}", std::process::id());
    let output = DataId::from("samples".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                // 第 seq 筆：seq, seq+0.5, …, seq+3.5（這些值在 f32 裡都能精確表示）
                let samples: Vec<f32> = (0..8).map(|i| seq as f32 + i as f32 * 0.5).collect();
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                p.insert("unit".into(), Parameter::String("volt".into()));
                p.insert("gain".into(), Parameter::Float(2.0));
                p.insert("t_send_ns".into(), Parameter::Integer(now_ns()));
                if seq == 1 {
                    println!("[producer] 第 1 筆 samples={samples:?}");
                }
                node.send_output(output.clone(), p, samples)?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[producer] 總共送出 {seq} 筆");
    Ok(())
}
