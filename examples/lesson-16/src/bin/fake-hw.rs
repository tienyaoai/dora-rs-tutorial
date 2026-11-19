//! ⚠️ 模擬硬體，不是 dora 節點、也不是真的裝置。同時模擬三樣東西：
//!   1. UART 感測器：openpty 開一對虛擬終端機，從 master 端每 20 ms 送一行溫度，
//!      slave 端用 symlink 掛成 $LESSON16_DIR/ttyFAKE0（等同 socat 的 pty,link=…）。
//!      會故意送壞檢查碼，並在 --unplug-at 秒時「拔線」1.5 秒（關掉 master、換一個新的 pts）。
//!   2. 另一個行程（例如相機驅動）寫入 /dev/shm/lesson16-ring 的環形緩衝，100 fps × 60 KB。
//!   3. ZYNQ PL 端的 AXI DMA S2MM＋ADC：暫存器是一個 4 KB 檔案（mmap）、DMA buffer 是 1 MB 檔案
//!      （假裝是 udmabuf）、PL 中斷是一個 FIFO（假裝是 /dev/uio0，read 讀到 u32 中斷次數）。
//!      每 --lose-every 次傳輸故意「掉一次中斷」，讓節點走逾時→重設的路徑。
//! 用法：fake-hw [--seconds N] [--unplug-at S] [--lose-every K]
use lesson_16::{axidma::*, now_ns, proto, ring::*, sim_dir};
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd, AsRawFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering, fence};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const PHYS_BASE: u64 = 0x3F00_0000; // 假的實體位址（udmabuf 的 phys_addr）
const DMA_BUF_SIZE: usize = 1 << 20;

fn arg(name: &str, default: f64) -> f64 {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() -> std::io::Result<()> {
    let dir = sim_dir();
    fs::create_dir_all(dir.join("sys"))?;
    let seconds = arg("--seconds", 30.0);
    let unplug_at = arg("--unplug-at", 4.0);
    let lose_every = arg("--lose-every", 40.0) as u32;
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();

    let hs = [
        { let (s, d) = (stop.clone(), dir.clone()); thread::spawn(move || uart(&d, s, t0, unplug_at)) },
        { let s = stop.clone(); thread::spawn(move || shm(s)) },
        { let (s, d) = (stop.clone(), dir.clone()); thread::spawn(move || pl(&d, s, lose_every)) },
    ];
    eprintln!("[fake-hw] 啟動：dir={} 執行 {seconds}s，UART 在 {unplug_at}s 拔線，DMA 每 {lose_every} 次掉一次中斷", dir.display());
    thread::sleep(Duration::from_secs_f64(seconds));
    stop.store(true, Ordering::Relaxed);
    for h in hs {
        let _ = h.join();
    }
    let _ = fs::remove_file("/dev/shm/lesson16-ring");
    eprintln!("[fake-hw] 結束");
    Ok(())
}

/// 開一對 pty，回傳 (master, slave 的路徑)。slave 先設成 raw，避免沒人開時 echo 回 master。
fn open_pty() -> std::io::Result<(File, String)> {
    let (mut m, mut s) = (0, 0);
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    unsafe { libc::cfmakeraw(&mut t) };
    if unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), &t, std::ptr::null()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut name = [0 as libc::c_char; 64];
    unsafe { libc::ttyname_r(s, name.as_mut_ptr(), name.len()) };
    let name = unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
    unsafe {
        libc::close(s);
        libc::fcntl(m, libc::F_SETFL, libc::O_NONBLOCK); // 沒人讀時寫滿就丟，不卡住模擬器
    }
    Ok((unsafe { File::from(OwnedFd::from_raw_fd(m)) }, name))
}

fn relink(target: &str, link: &Path) {
    let tmp = link.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp).unwrap();
    fs::rename(&tmp, link).unwrap(); // rename 是原子的：讀者不會看到「連結不存在」的空窗
}

fn uart(dir: &Path, stop: Arc<AtomicBool>, t0: Instant, unplug_at: f64) {
    let link = dir.join("ttyFAKE0");
    let (mut master, mut name) = open_pty().unwrap();
    relink(&name, &link);
    eprintln!("[fake-hw] UART：{} -> {name}", link.display());
    let (mut seq, mut overflow, mut unplugged) = (0u32, 0u64, false);
    while !stop.load(Ordering::Relaxed) {
        if !unplugged && t0.elapsed().as_secs_f64() >= unplug_at {
            unplugged = true;
            drop(master); // 拔線：關掉 master，slave 端的讀取會收到 EIO／hang-up
            let _ = fs::remove_file(&link);
            eprintln!("[fake-hw] UART：拔線（seq={seq}），1.5 s 後插回");
            thread::sleep(Duration::from_millis(1500));
            (master, name) = open_pty().unwrap();
            relink(&name, &link);
            eprintln!("[fake-hw] UART：插回 {} -> {name}", link.display());
        }
        seq += 1;
        let milli = 25_000 + ((seq as f64 / 25.0).sin() * 3000.0) as i32;
        let mut line = proto::encode(seq, milli).into_bytes();
        if seq % 37 == 0 {
            let n = line.len();
            line[n - 4] ^= 0x01; // 改一個 bit：模擬線路雜訊，檢查碼會對不上
        }
        if master.write_all(&line).is_err() {
            overflow += 1;
        }
        thread::sleep(Duration::from_millis(20));
    }
    eprintln!("[fake-hw] UART：共送 {seq} 行，寫滿丟掉 {overflow} 行");
}

fn shm(stop: Arc<AtomicBool>) {
    let ring = Ring::create(Path::new("/dev/shm/lesson16-ring"), 8, 64 * 1024).unwrap();
    let len = 60_000usize;
    let mut seq = 0u64;
    while !stop.load(Ordering::Relaxed) {
        ring.push(seq, |p| {
            p[0..8].copy_from_slice(&seq.to_le_bytes());
            p[8..16].copy_from_slice(&now_ns().to_le_bytes());
            p[16..20].copy_from_slice(&(len as u32).to_le_bytes());
            p[PAYLOAD_HDR..PAYLOAD_HDR + len].fill(seq as u8); // 內容可驗證：整塊都是 seq 的低 8 位
        });
        seq += 1;
        thread::sleep(Duration::from_millis(10));
    }
    eprintln!("[fake-hw] SHM：共寫 {seq} 格");
}

fn map_file(path: &Path, len: usize) -> memmap2::MmapRaw {
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path).unwrap();
    f.set_len(len as u64).unwrap();
    memmap2::MmapRaw::map_raw(&f).unwrap()
}

fn pl(dir: &Path, stop: Arc<AtomicBool>, lose_every: u32) {
    let regs = Regs::new(map_file(&dir.join("axi_dma_regs"), 4096));
    let buf = map_file(&dir.join("udmabuf0"), DMA_BUF_SIZE);
    // 模擬 u-dma-buf 的 sysfs：/sys/class/u-dma-buf/udmabuf0/{phys_addr,size}
    fs::write(dir.join("sys/phys_addr"), format!("{PHYS_BASE:#x}\n")).unwrap();
    fs::write(dir.join("sys/size"), format!("{DMA_BUF_SIZE}\n")).unwrap();
    let fifo = dir.join("uio0");
    let _ = fs::remove_file(&fifo);
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    // O_RDWR：Linux 允許，這樣沒有讀者時 open 不會卡住
    let mut irq = OpenOptions::new().read(true).write(true).open(&fifo).unwrap();
    unsafe { libc::fcntl(irq.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    regs.write(S2MM_DMASR, SR_HALTED);
    let (mut n_xfer, mut n_irq, mut lost, mut frame) = (0u32, 0u32, 0u32, 0u32);
    let mut phase = 0f64;
    while !stop.load(Ordering::Relaxed) {
        let cr = regs.read(S2MM_DMACR);
        if cr & CR_RESET != 0 {
            // 真硬體：寫 Reset 後 IP 自己清掉這個 bit、回到 Halted
            regs.write(S2MM_DMACR, 0);
            regs.write(S2MM_LENGTH, 0);
            regs.write(S2MM_DMASR, SR_HALTED);
            continue;
        }
        if cr & CR_RS == 0 {
            thread::sleep(Duration::from_micros(200));
            continue;
        }
        let sr = regs.read(S2MM_DMASR);
        if sr & SR_HALTED != 0 {
            regs.write(S2MM_DMASR, (sr & !SR_HALTED) | SR_IDLE);
        }
        let len = regs.read(S2MM_LENGTH) as usize;
        if len == 0 {
            thread::sleep(Duration::from_micros(200));
            continue;
        }
        // SIM：真的 AXI DMA 在「寫入 LENGTH」那一刻開始傳輸，完成後 LENGTH 讀回實際 byte 數。
        // 一般記憶體沒有「寫入副作用」，所以模擬器改成：看到 LENGTH≠0 就接下這次傳輸並把它歸零。
        regs.write(S2MM_LENGTH, 0);
        regs.write(S2MM_DMASR, regs.read(S2MM_DMASR) & !SR_IDLE);
        n_xfer += 1;
        let da = regs.read(S2MM_DA) as u64 | ((regs.read(S2MM_DA_MSB) as u64) << 32);
        let off = da.wrapping_sub(PHYS_BASE) as usize;
        if off + len > DMA_BUF_SIZE {
            // 位址超出 buffer：真硬體會是 DMADecErr（AXI DECERR），並發出錯誤中斷
            regs.write(S2MM_DMASR, regs.read(S2MM_DMASR) | SR_DMA_DEC_ERR | SR_ERR_IRQ | SR_HALTED);
            n_irq += 1;
            let _ = irq.write_all(&n_irq.to_ne_bytes());
            continue;
        }
        thread::sleep(Duration::from_millis(20)); // 模擬 ADC 取樣時間：一塊 ≈ 20 ms → 50 塊/秒
        if lose_every > 0 && n_xfer % lose_every == 0 {
            lost += 1; // 故意不完成、不發中斷：模擬 PL 端 tlast 沒送出、DMA 卡住
            continue;
        }
        frame += 1;
        let n = (len - FRAME_HDR) / 2;
        let p = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().add(off), len) };
        p[0..4].copy_from_slice(&FRAME_MAGIC.to_le_bytes());
        p[4..8].copy_from_slice(&frame.to_le_bytes());
        p[8..12].copy_from_slice(&(n as u32).to_le_bytes());
        p[16..24].copy_from_slice(&now_ns().to_le_bytes());
        for i in 0..n {
            let v = ((phase + i as f64 * 0.01).sin() * 12000.0) as i16;
            p[FRAME_HDR + 2 * i..FRAME_HDR + 2 * i + 2].copy_from_slice(&v.to_le_bytes());
        }
        phase += n as f64 * 0.01;
        fence(Ordering::Release); // 資料先落地、再亮完成旗標（真硬體由 AXI 交易順序保證）
        regs.write(S2MM_DMASR, regs.read(S2MM_DMASR) | SR_IOC_IRQ | SR_IDLE);
        if cr & CR_IOC_IRQ_EN != 0 {
            n_irq += 1;
            let _ = irq.write_all(&n_irq.to_ne_bytes()); // 等同 UIO：read() 拿到累計中斷次數
        }
    }
    eprintln!("[fake-hw] PL：收到 {n_xfer} 次傳輸請求，完成 {frame} 塊，故意掉中斷 {lost} 次，發出中斷 {n_irq} 次");
}
