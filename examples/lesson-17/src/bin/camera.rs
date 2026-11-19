//! 假攝影機：每個 tick 從 MNIST 測試集取下一張 28×28 灰階圖送出（Arrow UInt8，784 byte）。
//! metadata 帶 seq（序號）、label（正確答案，只為了讓 sink 算正確率；真的攝影機當然沒有）、
//! t_ns（擷取時間，算端到端延遲用）、width/height。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_17::{H, Mnist, PIXELS, W, env_or, now_ns};

fn main() -> eyre::Result<()> {
    let dir: String = env_or("DATASET", "data".to_string());
    let ds = Mnist::load(&dir, "t10k")?;
    let max: u64 = env_or("MAX_FRAMES", u64::MAX);
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("image".to_owned());
    let mut seq = 0u64;
    println!("[camera] 載入 {} 張測試圖，MAX_FRAMES={max}", ds.len());
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                if seq >= max { continue; }
                let i = (seq as usize) % ds.len();
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq as i64));
                p.insert("label".into(), Parameter::Integer(ds.labels[i] as i64));
                p.insert("t_ns".into(), Parameter::Integer(now_ns() as i64));
                p.insert("width".into(), Parameter::Integer(W as i64));
                p.insert("height".into(), Parameter::Integer(H as i64));
                let img = ds.image(i);
                node.send_output_raw(out.clone(), p, PIXELS, |dst| dst.copy_from_slice(img))?;
                seq += 1;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[camera] 結束：送出 {seq} 張");
    Ok(())
}
