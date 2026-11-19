//! 假攝影機：每個 tick 送一張 SIZE 位元組的影格，metadata 帶 seq 與送出時間 t_ns。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_18::{env_or, now_ns};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let size: usize = env_or("SIZE", 262_144);
    let total: i64 = env_or("N", 2000);
    let out = DataId::from("frame".to_owned());
    println!("producer SIZE={size} N={total} zero_copy_threshold={}", node.zero_copy_threshold());
    let mut seq = 0i64;
    while let Some(ev) = events.recv() {
        if let Event::Input { .. } = ev {
            seq += 1;
            let mut p = MetadataParameters::default();
            p.insert("seq".into(), Parameter::Integer(seq));
            p.insert("t_ns".into(), Parameter::Integer(now_ns()));
            node.send_output_raw(out.clone(), p, size, |buf| {
                // 填入隨 seq 變動的內容（簡單的遞增樣式）
                let base = seq as u8;
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = base.wrapping_add(i as u8);
                }
            })?;
            if seq >= total {
                break;
            }
        }
    }
    println!("producer 送出 {seq} 張");
    Ok(())
}
