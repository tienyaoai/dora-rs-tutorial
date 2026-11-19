//! 下游：驗證三個硬體節點送來的資料（內容、序號缺口、延遲），結束時印總表。
use dora_node_api::{DoraNode, Event, get_integer_param};
use lesson_16::{axidma::{FRAME_HDR, FRAME_MAGIC}, now_ns};

#[derive(Default)]
struct Lat(Vec<u64>);
impl Lat {
    fn summary(&mut self) -> String {
        if self.0.is_empty() { return "—".into(); }
        self.0.sort_unstable();
        let q = |f: f64| self.0[((self.0.len() - 1) as f64 * f) as usize] as f64 / 1e6;
        format!("p50 {:.2} ms / p99 {:.2} ms / max {:.2} ms", q(0.5), q(0.99), q(1.0))
    }
}

/// 這個位址落在哪個 mapping？讀 /proc/self/maps——用來確認收到的資料是不是直接指向 /dev/shm（零拷貝）。
fn mapping_of(addr: usize) -> String {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    for l in maps.lines() {
        let mut f = l.split_whitespace();
        let (Some(range), Some(perm)) = (f.next(), f.next()) else { continue };
        let Some((a, b)) = range.split_once('-') else { continue };
        let (a, b) = (usize::from_str_radix(a, 16).unwrap_or(0), usize::from_str_radix(b, 16).unwrap_or(0));
        if (a..b).contains(&addr) {
            let name = f.nth(3).unwrap_or("[匿名：heap 或執行緒 arena]");
            return format!("{perm} {name}");
        }
    }
    "找不到".into()
}

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    let (mut uart_n, mut uart_batches, mut uart_gaps, mut uart_last) = (0u64, 0u64, 0u64, 0i64);
    let (mut shm_n, mut shm_bad, mut shm_gaps, mut shm_last, mut shm_bytes) = (0u64, 0u64, 0u64, -1i64, 0usize);
    let (mut dma_n, mut dma_bad, mut dma_gaps, mut dma_last, mut dma_bytes) = (0u64, 0u64, 0u64, 0i64, 0usize);
    let (mut shm_lat, mut dma_lat) = (Lat::default(), Lat::default());
    let mut statuses = 0u64;

    while let Some(event) = events.recv() {
        let Event::Input { id, metadata, data } = event else {
            if matches!(event, Event::Stop(_)) { break; }
            continue;
        };
        let pm = &metadata.parameters;
        match id.as_str() {
            "uart_temp" => {
                let v: &[i32] = (&data).try_into()?;
                let first = get_integer_param(pm, "first_seq").unwrap_or(-1);
                let last = get_integer_param(pm, "last_seq").unwrap_or(-1);
                if uart_last > 0 && first > uart_last + 1 {
                    uart_gaps += (first - uart_last - 1) as u64; // 批次之間
                    println!("[monitor] uart 缺口：上一批到 seq {uart_last}，這一批從 {first} 開始");
                }
                uart_gaps += (last - first + 1) as u64 - v.len() as u64; // 批次之內（例如檢查碼錯被丟掉的行）
                if uart_batches % 20 == 0 {
                    println!("[monitor] uart 批次 seq {first}..={last}（{} 筆）第一筆 {:.3} °C", v.len(), v[0] as f64 / 1000.0);
                }
                uart_last = last;
                uart_batches += 1;
                uart_n += v.len() as u64;
            }
            "shm_frame" => {
                let b: &[u8] = (&data).try_into()?;
                let seq = get_integer_param(pm, "seq").unwrap_or(-1);
                let t = get_integer_param(pm, "t_ns").unwrap_or(0) as u64;
                shm_lat.0.push(now_ns().saturating_sub(t));
                if b.iter().any(|&x| x != seq as u8) { shm_bad += 1; } // 內容整塊應該都是 seq 的低 8 位
                if shm_last >= 0 && seq > shm_last + 1 { shm_gaps += (seq - shm_last - 1) as u64; }
                if shm_n % 200 == 0 {
                    println!("[monitor] shm seq={seq} {} bytes，資料位址 {:p} → {}", b.len(), b.as_ptr(), mapping_of(b.as_ptr() as usize));
                }
                shm_last = seq;
                shm_n += 1;
                shm_bytes += b.len();
            }
            "dma_frame" => {
                let b: &[u8] = (&data).try_into()?;
                let frame = get_integer_param(pm, "frame").unwrap_or(-1);
                let magic = u32::from_le_bytes(b[0..4].try_into()?);
                let n = u32::from_le_bytes(b[8..12].try_into()?) as usize;
                let t = u64::from_le_bytes(b[16..24].try_into()?);
                dma_lat.0.push(now_ns().saturating_sub(t));
                if magic != FRAME_MAGIC || b.len() != FRAME_HDR + 2 * n { dma_bad += 1; }
                if dma_last > 0 && frame > dma_last + 1 { dma_gaps += (frame - dma_last - 1) as u64; }
                if dma_n % 100 == 0 {
                    let s0 = i16::from_le_bytes(b[FRAME_HDR..FRAME_HDR + 2].try_into()?);
                    println!("[monitor] dma frame={frame} irq={} {n} 樣本，第一個樣本 {s0}，資料位址 {:p} → {}",
                             get_integer_param(pm, "irq").unwrap_or(-1), b.as_ptr(), mapping_of(b.as_ptr() as usize));
                }
                dma_last = frame;
                dma_n += 1;
                dma_bytes += b.len();
            }
            s if s.ends_with("_status") => {
                let msg: &str = (&data).try_into()?;
                println!("[monitor] 狀態 {s}: {msg}");
                statuses += 1;
            }
            _ => {}
        }
    }
    eprintln!("[monitor] UART：{uart_n} 筆／{uart_batches} 批，序號缺口 {uart_gaps}");
    eprintln!("[monitor] SHM ：{shm_n} 格（{:.1} MB），內容錯 {shm_bad}，序號缺口 {shm_gaps}，延遲 {}",
              shm_bytes as f64 / 1e6, shm_lat.summary());
    eprintln!("[monitor] DMA ：{dma_n} 塊（{:.1} MB），格式錯 {dma_bad}，frame 缺口 {dma_gaps}，延遲 {}",
              dma_bytes as f64 / 1e6, dma_lat.summary());
    eprintln!("[monitor] 狀態訊息 {statuses} 則");
    Ok(())
}
