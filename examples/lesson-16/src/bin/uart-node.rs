//! 序列埠（UART）硬體節點。
//! 執行緒分工：
//!   I/O 執行緒：開埠、阻塞讀取（有逾時）、切行、驗檢查碼、斷線重連（指數退避）。
//!   主執行緒（dora 事件迴圈）：只處理 dora 事件；每個 tick 把 channel 裡累積的讀值一次送出。
//! 兩邊只靠有界 channel（背壓：滿了丟最新的一筆並計數）和 Arc<Stats> 溝通。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_16::proto::{self, ParseError};
use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Stats {
    lines: AtomicU64,
    framing: AtomicU64,
    checksum: AtomicU64,
    dropped: AtomicU64, // 背壓：channel 滿了丟掉的
    reconnects: AtomicU64,
}

enum Msg {
    Reading { seq: u32, milli_c: i32 },
    Status(String), // 錯誤／狀態：走另一條無界 channel，不會被背壓丟掉
}

struct Cfg {
    path: String,
    baud: u32,
    read_timeout: Duration, // 單次 read 最多等多久（讓執行緒能定期檢查 stop）
    silence: Duration,      // 多久沒收到完整一行就當作裝置失聯、主動重連
}

fn io_thread(cfg: Cfg, data: SyncSender<Msg>, status: mpsc::Sender<Msg>, st: Arc<Stats>, stop: Arc<AtomicBool>) {
    let mut backoff = Duration::from_millis(100);
    let mut first = true;
    while !stop.load(Relaxed) {
        let mut port = match serialport::new(&cfg.path, cfg.baud).timeout(cfg.read_timeout).open() {
            Ok(p) => p,
            Err(e) => {
                let _ = status.send(Msg::Status(format!("開啟 {} 失敗：{e}；{backoff:?} 後重試", cfg.path)));
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(2));
                continue;
            }
        };
        if !first {
            st.reconnects.fetch_add(1, Relaxed);
        }
        first = false;
        backoff = Duration::from_millis(100);
        let _ = status.send(Msg::Status(format!("已連線 {} @ {} baud", cfg.path, cfg.baud)));
        let _ = port.clear(serialport::ClearBuffer::Input); // 丟掉開埠前殘留的舊資料
        let (mut acc, mut buf) = (Vec::with_capacity(256), [0u8; 256]);
        let mut synced = false; // 第一個 '\n' 之前可能是半行，丟掉
        let mut last_good = Instant::now();
        let reason = loop {
            if stop.load(Relaxed) {
                break "stop".to_string();
            }
            if last_good.elapsed() > cfg.silence {
                break format!("{:?} 沒有收到有效資料", cfg.silence);
            }
            let n = match port.read(&mut buf) {
                Ok(0) => break "讀到 EOF（裝置關閉）".into(),
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::TimedOut => continue,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => break format!("讀取錯誤：{e}"),
            };
            acc.extend_from_slice(&buf[..n]);
            while let Some(nl) = acc.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = acc.drain(..=nl).collect();
                if !synced {
                    synced = true;
                    if !line.starts_with(b"$") {
                        continue;
                    }
                }
                let line = line.strip_suffix(b"\r\n").or(line.strip_suffix(b"\n")).unwrap_or(&line);
                match proto::parse(line) {
                    Ok((seq, milli_c)) => {
                        st.lines.fetch_add(1, Relaxed);
                        last_good = Instant::now();
                        if let Err(TrySendError::Full(_)) = data.try_send(Msg::Reading { seq, milli_c }) {
                            st.dropped.fetch_add(1, Relaxed);
                        }
                    }
                    Err(ParseError::Checksum) => _ = st.checksum.fetch_add(1, Relaxed),
                    Err(_) => _ = st.framing.fetch_add(1, Relaxed),
                }
            }
            if acc.len() > 1024 {
                acc.clear(); // 一直沒有換行：雜訊或鮑率設錯，別讓緩衝無限長大
                st.framing.fetch_add(1, Relaxed);
            }
        };
        if reason != "stop" {
            let _ = status.send(Msg::Status(format!("斷線：{reason}；重新連線")));
        }
        // port 在這裡 drop → close(fd)，釋放 TIOCEXCL／flock，重連才開得起來
    }
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let cfg = Cfg {
        path: std::env::var("SERIAL_PORT").unwrap_or_else(|_| lesson_16::sim_dir().join("ttyFAKE0").display().to_string()),
        baud: std::env::var("BAUD").ok().and_then(|b| b.parse().ok()).unwrap_or(115_200),
        read_timeout: Duration::from_millis(100),
        silence: Duration::from_millis(500),
    };
    let cap: usize = std::env::var("QUEUE").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
    let (data_tx, data_rx): (SyncSender<Msg>, Receiver<Msg>) = mpsc::sync_channel(cap);
    let (status_tx, status_rx) = mpsc::channel();
    let st = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let io = {
        let (st, stop) = (st.clone(), stop.clone());
        std::thread::Builder::new().name("uart-io".into()).spawn(move || io_thread(cfg, data_tx, status_tx, st, stop))?
    };
    let (out_temp, out_status) = (DataId::from("temp".to_owned()), DataId::from("status".to_owned()));
    let mut sent = 0u64;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                for m in status_rx.try_iter() {
                    if let Msg::Status(s) = m {
                        eprintln!("[uart] {s}");
                        node.send_output(out_status.clone(), MetadataParameters::new(), s.as_str())?;
                    }
                }
                let (mut seqs, mut vals) = (Vec::new(), Vec::new());
                for m in data_rx.try_iter() {
                    if let Msg::Reading { seq, milli_c } = m {
                        seqs.push(seq);
                        vals.push(milli_c);
                    }
                }
                if !vals.is_empty() {
                    let mut p = MetadataParameters::new();
                    p.insert("first_seq".into(), Parameter::Integer(seqs[0] as i64));
                    p.insert("last_seq".into(), Parameter::Integer(*seqs.last().unwrap() as i64));
                    sent += vals.len() as u64;
                    node.send_output(out_temp.clone(), p, vals)?; // 一個 tick 一批，Arrow Int32 陣列
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    stop.store(true, Relaxed);
    let _ = io.join(); // read 逾時 100 ms，最多等這麼久
    eprintln!(
        "[uart] 結束：有效 {} 行、送出 {sent} 筆、檢查碼錯 {}、半行/格式錯 {}、背壓丟棄 {}、重連 {} 次",
        st.lines.load(Relaxed), st.checksum.load(Relaxed), st.framing.load(Relaxed),
        st.dropped.load(Relaxed), st.reconnects.load(Relaxed)
    );
    Ok(())
}
