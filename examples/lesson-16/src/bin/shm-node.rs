//! 共享記憶體節點：把「別的行程」寫進 /dev/shm 環形緩衝的大塊資料接進 dora。
//! 讀 ring 不會阻塞（只是讀記憶體），所以不另開執行緒：每個 5 ms tick 把新的格子讀完。
//! 每一格只複製一次：ring（外部 shm）→ send_output_raw 給的 dora sample（dora 的 zenoh shm）。
//! 之後 dora 把那塊 shm 交給下游，下游讀到的就是同一塊記憶體（零拷貝）。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_16::ring::{PAYLOAD_HDR, Ring};
use std::path::Path;
use std::time::{Duration, Instant};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let path = std::env::var("RING").unwrap_or_else(|_| "/dev/shm/lesson16-ring".into());
    eprintln!("[shm] zero_copy_threshold = {} bytes", node.zero_copy_threshold());
    let out = DataId::from("frame".to_owned());
    let out_status = DataId::from("status".to_owned());
    let mut ring: Option<Ring> = None;
    let mut last_try = Instant::now() - Duration::from_secs(1);
    let (mut next, mut sent, mut overrun, mut torn) = (0u64, 0u64, 0u64, 0u64);

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                if ring.is_none() {
                    if last_try.elapsed() < Duration::from_millis(100) {
                        continue;
                    }
                    last_try = Instant::now();
                    match Ring::open(Path::new(&path)) {
                        Ok(r) => {
                            next = r.write_seq().load(std::sync::atomic::Ordering::Acquire); // 從最新的開始，不追舊帳
                            let s = format!("附掛 {path}：{} 格 × {} bytes，從 seq={next} 開始", r.n_slots, r.slot_size);
                            eprintln!("[shm] {s}");
                            node.send_output(out_status.clone(), MetadataParameters::new(), s.as_str())?;
                            ring = Some(r);
                        }
                        Err(e) => {
                            eprintln!("[shm] 還不能附掛 {path}：{e}");
                            continue;
                        }
                    }
                }
                let r = ring.as_ref().unwrap();
                let w = r.write_seq().load(std::sync::atomic::Ordering::Acquire);
                if w < next {
                    // 寫者重啟（seq 歸零）：重新同步
                    next = w;
                }
                if w - next > r.n_slots as u64 {
                    overrun += w - next - r.n_slots as u64; // 讀太慢，舊格子已被蓋掉
                    next = w - r.n_slots as u64;
                }
                while next < w {
                    let seq = next;
                    next += 1;
                    // 先看 header 拿長度與時間戳
                    let mut meta = (0u64, 0usize);
                    if !r.read(seq, |p| {
                        meta = (u64::from_le_bytes(p[8..16].try_into().unwrap()),
                                u32::from_le_bytes(p[16..20].try_into().unwrap()) as usize)
                    }) {
                        torn += 1;
                        continue;
                    }
                    let (t_ns, len) = meta;
                    let mut ok = true;
                    let mut p = MetadataParameters::new();
                    p.insert("seq".into(), Parameter::Integer(seq as i64));
                    p.insert("t_ns".into(), Parameter::Integer(t_ns as i64));
                    // send_output_raw 的 closure 拿到的 dst 就是 dora 的 shm sample：只有這一次複製
                    node.send_output_raw(out.clone(), p, len, |dst| {
                        ok = r.read(seq, |src| dst.copy_from_slice(&src[PAYLOAD_HDR..PAYLOAD_HDR + len]));
                    })?;
                    // 注意：send_output_raw 一旦呼叫就送出了。若複製途中被蓋掉（ok=false），
                    // 下游會收到一筆壞資料——所以 monitor 會驗內容；更嚴謹的做法見本篇「常見錯誤」。
                    if ok { sent += 1 } else { torn += 1 }
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[shm] 結束：送出 {sent} 格，讀太慢被覆蓋 {overrun} 格，讀到一半被改 {torn} 格");
    Ok(())
}
