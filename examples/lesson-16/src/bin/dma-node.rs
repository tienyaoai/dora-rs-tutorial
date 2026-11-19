//! ZYNQ PL → DDR（AXI DMA S2MM，direct register mode）硬體節點的「形狀」。
//! ⚠️ 本篇在 x86 虛擬機上跑，暫存器／buffer／中斷全由 fake-hw 用檔案模擬（設 DMA_SIM=1）。
//! 換到真板子時要改的只有路徑（環境變數）和標了 SIM 的兩處。
//!
//!   I/O 執行緒：設定 DMA、等中斷（poll 有逾時）、檢查錯誤、ack、cache 同步，
//!               用 ping-pong 兩塊 buffer：一塊給 DMA 寫、一塊給 dora 讀。
//!   主執行緒：收到「第 h 塊好了」→ send_output_raw 從 DMA buffer 直接複製進 dora sample
//!             → 把第 h 塊還給 I/O 執行緒（credit）。主執行緒太慢時 I/O 執行緒拿不到空 buffer，
//!             DMA 就暫停（背壓往上游傳；真系統上 PL 端 FIFO 會滿、要在 FPGA 那邊計數 overflow）。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};
use lesson_16::axidma::*;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

const XFER: usize = 32 * 1024; // 每次 DMA 的 byte 數（= FRAME_HDR + 16372 個 i16 樣本）
const HALF: usize = 64 * 1024; // ping-pong 兩塊的間距

struct Ready { half: usize, frame: u32, irq_count: u32, len: usize }

#[derive(Default)]
struct Stats { frames: AtomicU64, timeouts: AtomicU64, errors: AtomicU64, stalls: AtomicU64, resets: AtomicU64 }

struct Hw {
    regs: Regs,
    buf: Arc<memmap2::MmapRaw>,
    phys: u64,
    uio: File,
    sysfs: PathBuf,
    sim: bool,
}

impl Hw {
    fn open() -> eyre::Result<(Hw, Arc<memmap2::MmapRaw>)> {
        let d = lesson_16::sim_dir();
        let sim = std::env::var("DMA_SIM").is_ok_and(|v| v == "1");
        // 真板子（UIO）：DMA_REGS=/dev/uio0（map0 = AXI DMA 暫存器，mmap offset 0）
        //   或 /dev/mem 加 offset=0x40400000（ZYNQ-7000 Vivado 預設位址）、O_SYNC 才是不快取的映射。
        // 真板子（u-dma-buf）：DMA_BUF=/dev/udmabuf0，SYSFS=/sys/class/u-dma-buf/udmabuf0
        let env = |k: &str, def: PathBuf| std::env::var_os(k).map(PathBuf::from).unwrap_or(def);
        let regs_path = env("DMA_REGS", d.join("axi_dma_regs"));
        let buf_path = env("DMA_BUF", d.join("udmabuf0"));
        let uio_path = env("DMA_UIO", d.join("uio0"));
        let sysfs = env("DMA_SYSFS", d.join("sys"));

        let f = OpenOptions::new().read(true).write(true).open(&regs_path)?;
        let regs = Regs::new(memmap2::MmapOptions::new().len(4096).map_raw(&f)?);
        let f = OpenOptions::new().read(true).write(true).open(&buf_path)?;
        let buf = Arc::new(memmap2::MmapRaw::map_raw(&f)?);
        let phys_s = fs::read_to_string(sysfs.join("phys_addr"))?;
        let phys = u64::from_str_radix(phys_s.trim().trim_start_matches("0x"), 16)?;
        let uio = OpenOptions::new().read(true).write(true).open(&uio_path)?;
        eprintln!("[dma] regs={} buf={} ({} bytes, phys={phys:#x}) irq={} sim={sim}",
                  regs_path.display(), buf_path.display(), buf.len(), uio_path.display());
        Ok((Hw { regs, buf: buf.clone(), phys, uio, sysfs, sim }, buf))
    }

    /// 軟體重設 DMA，等 Reset bit 自己清掉（有逾時）。
    fn reset(&self) -> eyre::Result<()> {
        self.regs.write(S2MM_DMACR, CR_RESET);
        for _ in 0..100 {
            if self.regs.read(S2MM_DMACR) & CR_RESET == 0 {
                self.regs.write(S2MM_DMACR, CR_RS | CR_IOC_IRQ_EN | CR_ERR_IRQ_EN);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        eyre::bail!("DMA reset 100 ms 內沒有完成（DMASR={:#x}）", self.regs.read(S2MM_DMASR))
    }

    /// 寫 1 清除（W1C）狀態位元。SIM：一般記憶體沒有 W1C 行為，只好直接把 bit 清成 0。
    fn ack(&self, bits: u32) {
        if self.sim {
            self.regs.write(S2MM_DMASR, self.regs.read(S2MM_DMASR) & !bits);
        } else {
            self.regs.write(S2MM_DMASR, bits);
        }
    }

    /// 開始一次 S2MM：先寫目的位址，最後寫 LENGTH（寫 LENGTH 就是「開始」）。
    fn start(&self, half: usize) {
        let da = self.phys + (half * HALF) as u64;
        self.regs.write(S2MM_DA, da as u32);
        self.regs.write(S2MM_DA_MSB, (da >> 32) as u32); // 只有 >32 位元位址寬度的設定才有這個暫存器
        self.regs.write(S2MM_LENGTH, XFER as u32);
    }

    /// UIO 的中斷：每次 read 前寫 1 重新致能（irqcontrol），read 回傳 u32 累計次數。用 poll 做逾時。
    fn wait_irq(&mut self, timeout_ms: i32) -> std::io::Result<Option<u32>> {
        if !self.sim {
            self.uio.write_all(&1u32.to_ne_bytes())?; // SIM：FIFO 寫進去會被自己讀回來，所以略過
        }
        let mut pfd = libc::pollfd { fd: self.uio.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        match unsafe { libc::poll(&mut pfd, 1, timeout_ms) } {
            0 => Ok(None),
            n if n < 0 => Err(std::io::Error::last_os_error()),
            _ => {
                let mut b = [0u8; 4];
                self.uio.read_exact(&mut b)?;
                Ok(Some(u32::from_ne_bytes(b)))
            }
        }
    }

    /// CPU 要讀 DMA 寫好的資料前：把這段的 cache 作廢（non-coherent 埠，例如 ZYNQ-7000 的 HP0～3）。
    /// u-dma-buf 的做法是寫 sysfs；在 coherent 埠（ACP、ZynqMP 的 HPC＋CCI）上可以省略。
    fn sync_for_cpu(&self, off: usize, len: usize) -> std::io::Result<()> {
        fs::write(self.sysfs.join("sync_offset"), off.to_string())?;
        fs::write(self.sysfs.join("sync_size"), len.to_string())?;
        fs::write(self.sysfs.join("sync_direction"), "2")?; // 2 = DMA_FROM_DEVICE
        fs::write(self.sysfs.join("sync_for_cpu"), "1")
    }
}

fn io_thread(mut hw: Hw, ready: Sender<Ready>, credit: Receiver<usize>, status: Sender<String>,
             st: Arc<Stats>, stop: Arc<AtomicBool>) {
    let mut free = [true, true];
    let mut cur: Option<usize> = None;
    if let Err(e) = hw.reset() {
        let _ = status.send(format!("初始化失敗：{e}"));
        return;
    }
    while !stop.load(Relaxed) {
        for h in credit.try_iter() { free[h] = true; }
        if cur.is_none() {
            match free.iter().position(|&f| f) {
                Some(h) => { free[h] = false; hw.start(h); cur = Some(h); }
                None => {
                    // 兩塊都在 dora 手上：等主執行緒還 buffer（背壓）
                    st.stalls.fetch_add(1, Relaxed);
                    if let Ok(h) = credit.recv_timeout(Duration::from_millis(50)) { free[h] = true; }
                    continue;
                }
            }
        }
        let irq = match hw.wait_irq(200) {
            Ok(Some(n)) => n,
            Ok(None) => {
                st.timeouts.fetch_add(1, Relaxed);
                let _ = status.send(format!("中斷逾時 200 ms（DMASR={:#x}），重設 DMA", hw.regs.read(S2MM_DMASR)));
                free[cur.take().unwrap()] = true; // 那次傳輸作廢，buffer 收回
                if let Err(e) = hw.reset() { let _ = status.send(format!("重設失敗：{e}")); return; }
                st.resets.fetch_add(1, Relaxed);
                continue;
            }
            Err(e) => { let _ = status.send(format!("讀中斷失敗：{e}")); return; }
        };
        let sr = hw.regs.read(S2MM_DMASR);
        if sr & SR_ERRORS != 0 {
            st.errors.fetch_add(1, Relaxed);
            let _ = status.send(format!("DMA 錯誤 DMASR={sr:#x}，重設"));
            free[cur.take().unwrap()] = true;
            let _ = hw.reset();
            st.resets.fetch_add(1, Relaxed);
            continue;
        }
        if sr & SR_IOC_IRQ == 0 { continue; } // 不是完成中斷（共用中斷線時會發生）
        hw.ack(SR_IOC_IRQ);
        let h = cur.take().unwrap();
        // ping-pong：馬上讓 DMA 寫另一塊，CPU 這邊同時處理剛寫好的這塊
        if let Some(n) = free.iter().position(|&f| f) { free[n] = false; hw.start(n); cur = Some(n); }
        if let Err(e) = hw.sync_for_cpu(h * HALF, XFER) { let _ = status.send(format!("cache 同步失敗：{e}")); }
        let p = unsafe { std::slice::from_raw_parts(hw.buf.as_ptr().add(h * HALF), FRAME_HDR) };
        let magic = u32::from_le_bytes(p[0..4].try_into().unwrap());
        let frame = u32::from_le_bytes(p[4..8].try_into().unwrap());
        let n = u32::from_le_bytes(p[8..12].try_into().unwrap()) as usize;
        if magic != FRAME_MAGIC || FRAME_HDR + 2 * n > XFER {
            st.errors.fetch_add(1, Relaxed);
            let _ = status.send(format!("buffer {h} 的 header 不對（magic={magic:#x}）——cache 沒作廢？"));
            free[h] = true;
            continue;
        }
        st.frames.fetch_add(1, Relaxed);
        if ready.send(Ready { half: h, frame, irq_count: irq, len: FRAME_HDR + 2 * n }).is_err() { return; }
    }
    hw.regs.write(S2MM_DMACR, 0); // 停 DMA：RS=0
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let (out, out_status) = (DataId::from("frame".to_owned()), DataId::from("status".to_owned()));
    let (hw, buf) = Hw::open()?;
    let (ready_tx, ready_rx) = mpsc::channel::<Ready>(); // 最多 2 筆在途（只有兩塊 buffer），不必有界
    let (credit_tx, credit_rx) = mpsc::channel::<usize>();
    let (status_tx, status_rx) = mpsc::channel::<String>();
    let st = Arc::new(Stats::default());
    let stop = Arc::new(AtomicBool::new(false));
    let io = {
        let (st, stop) = (st.clone(), stop.clone());
        std::thread::Builder::new().name("dma-io".into())
            .spawn(move || io_thread(hw, ready_tx, credit_rx, status_tx, st, stop))?
    };
    let mut sent = 0u64;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                for s in status_rx.try_iter() {
                    eprintln!("[dma] {s}");
                    node.send_output(out_status.clone(), MetadataParameters::new(), s.as_str())?;
                }
                for r in ready_rx.try_iter() {
                    let mut p = MetadataParameters::new();
                    p.insert("frame".into(), Parameter::Integer(r.frame as i64));
                    p.insert("irq".into(), Parameter::Integer(r.irq_count as i64));
                    let src = unsafe { std::slice::from_raw_parts(buf.as_ptr().add(r.half * HALF), r.len) };
                    node.send_output_raw(out.clone(), p, r.len, |dst| dst.copy_from_slice(src))?;
                    let _ = credit_tx.send(r.half); // 複製完就能還：下游拿到的是 dora 的 shm，不是 DMA buffer
                    sent += 1;
                }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    stop.store(true, Relaxed);
    let _ = io.join();
    eprintln!("[dma] 結束：完成 {} 塊、送出 {sent} 塊、中斷逾時 {}、DMA 錯誤 {}、重設 {}、等 buffer {} 次",
        st.frames.load(Relaxed), st.timeouts.load(Relaxed), st.errors.load(Relaxed),
        st.resets.load(Relaxed), st.stalls.load(Relaxed));
    Ok(())
}
