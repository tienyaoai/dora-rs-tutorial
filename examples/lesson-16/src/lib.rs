//! 第 16 篇共用程式：UART 文字協定、/dev/shm 環形緩衝的版面、AXI DMA 暫存器表、路徑設定。
//! 注意：本專案的「硬體」全部是 fake-hw 模擬出來的（pty、/dev/shm 檔案、一般檔案 mmap、FIFO），
//! 不是真的 UART／FPGA。程式結構刻意寫成換成真裝置時只改路徑與少數標了 SIM 的地方。
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 模擬硬體的檔案都放這裡（可用環境變數 LESSON16_DIR 改）。
pub fn sim_dir() -> PathBuf {
    std::env::var_os("LESSON16_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/lesson16"))
}

/// 牆上時鐘（ns），用來量「裝置寫入 → monitor 收到」的延遲（同一台機器才有意義）。
pub fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

// ───────────────────────── UART 協定 ─────────────────────────
/// 一行一筆：`$T,<seq>,<毫攝氏度>*<XOR 檢查碼兩位十六進位>\r\n`（NMEA 風格）。
pub mod proto {
    pub fn xor_cs(body: &[u8]) -> u8 {
        body.iter().fold(0u8, |a, b| a ^ b)
    }

    pub fn encode(seq: u32, milli_c: i32) -> String {
        let body = format!("T,{seq},{milli_c}");
        format!("${body}*{:02X}\r\n", xor_cs(body.as_bytes()))
    }

    #[derive(Debug, PartialEq)]
    pub enum ParseError {
        Framing,  // 沒有 $ 或 *，通常是開機／重連時接到半行
        Checksum, // 線路雜訊
        Field,    // 格式對但欄位不是數字
    }

    /// 不含行尾 \r\n 的一行。
    pub fn parse(line: &[u8]) -> Result<(u32, i32), ParseError> {
        let line = line.strip_prefix(b"$").ok_or(ParseError::Framing)?;
        let star = line.iter().rposition(|&b| b == b'*').ok_or(ParseError::Framing)?;
        let (body, cs) = (&line[..star], &line[star + 1..]);
        let cs = std::str::from_utf8(cs).ok().and_then(|s| u8::from_str_radix(s, 16).ok());
        if cs != Some(xor_cs(body)) {
            return Err(ParseError::Checksum);
        }
        let s = std::str::from_utf8(body).map_err(|_| ParseError::Field)?;
        let mut it = s.split(',');
        match (it.next(), it.next(), it.next(), it.next()) {
            (Some("T"), Some(seq), Some(v), None) => Ok((
                seq.parse().map_err(|_| ParseError::Field)?,
                v.parse().map_err(|_| ParseError::Field)?,
            )),
            _ => Err(ParseError::Field),
        }
    }
}

// ───────────────────── /dev/shm 環形緩衝 ─────────────────────
/// 單一寫者、多讀者的 seqlock 環形緩衝。檔案版面：
/// ```text
/// 0x00 magic u64 | 0x08 n_slots u32 | 0x0C slot_size u32 | 0x10 write_seq AtomicU64 | …填到 64
/// 每個 slot（slot_size bytes）：0x00 slot_seq AtomicU64 | 0x08 payload
/// payload：0x00 frame_seq u64 | 0x08 t_ns u64 | 0x10 len u32 | 0x14 pad | 0x18 資料…
/// ```
pub mod ring {
    use memmap2::MmapRaw;
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering, fence};

    pub const MAGIC: u64 = 0x4C31_3652_494E_4701; // "L16RING" + 版本 1
    pub const HDR: usize = 64;
    pub const SLOT_HDR: usize = 8;
    pub const PAYLOAD_HDR: usize = 24;
    const WRITING: u64 = u64::MAX;

    pub struct Ring {
        map: MmapRaw,
        pub n_slots: usize,
        pub slot_size: usize,
    }

    impl Ring {
        /// 寫者（模擬的驅動／相機程式）建立並清空。
        pub fn create(path: &Path, n_slots: usize, slot_size: usize) -> std::io::Result<Ring> {
            let f = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
            f.set_len((HDR + n_slots * slot_size) as u64)?;
            let map = MmapRaw::map_raw(&f)?;
            let r = Ring { map, n_slots, slot_size };
            unsafe {
                let p = r.map.as_mut_ptr();
                (p.add(0x08) as *mut u32).write(n_slots as u32);
                (p.add(0x0C) as *mut u32).write(slot_size as u32);
            }
            r.write_seq().store(0, Ordering::Relaxed);
            fence(Ordering::Release);
            // magic 最後寫：讀者看到 magic 才代表版面已經初始化好
            unsafe { (r.map.as_mut_ptr() as *const AtomicU64).as_ref().unwrap() }.store(MAGIC, Ordering::Release);
            Ok(r)
        }

        /// 讀者（dora 節點）附掛。檔案不在或還沒初始化 → Err，呼叫端稍後重試。
        pub fn open(path: &Path) -> std::io::Result<Ring> {
            let f = OpenOptions::new().read(true).write(true).open(path)?;
            if f.metadata()?.len() < HDR as u64 {
                return Err(std::io::Error::other("ring 太小（還在建立中？）"));
            }
            let map = MmapRaw::map_raw(&f)?;
            let p = map.as_ptr();
            let magic = unsafe { (p as *const AtomicU64).as_ref().unwrap() }.load(Ordering::Acquire);
            if magic != MAGIC {
                return Err(std::io::Error::other(format!("magic 不對：{magic:#x}")));
            }
            let (n_slots, slot_size) = unsafe {
                ((p.add(0x08) as *const u32).read() as usize, (p.add(0x0C) as *const u32).read() as usize)
            };
            if map.len() < HDR + n_slots * slot_size {
                return Err(std::io::Error::other("檔案長度和 header 不符"));
            }
            Ok(Ring { map, n_slots, slot_size })
        }

        fn atomic(&self, off: usize) -> &AtomicU64 {
            // SAFETY: off 是 8 的倍數、在 mapping 內；mmap 起點按頁對齊。
            unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU64) }
        }
        pub fn write_seq(&self) -> &AtomicU64 {
            self.atomic(0x10)
        }
        fn slot_off(&self, seq: u64) -> usize {
            HDR + (seq as usize % self.n_slots) * self.slot_size
        }
        pub fn max_payload(&self) -> usize {
            self.slot_size - SLOT_HDR
        }

        /// 寫者：寫入第 seq 筆（seq 從 0 遞增）。`fill` 拿到整個 payload 區、回傳實際用掉幾 byte。
        pub fn push(&self, seq: u64, fill: impl FnOnce(&mut [u8])) {
            let off = self.slot_off(seq);
            let slot_seq = self.atomic(off);
            slot_seq.store(WRITING, Ordering::Relaxed);
            fence(Ordering::Release);
            let buf = unsafe {
                std::slice::from_raw_parts_mut(self.map.as_mut_ptr().add(off + SLOT_HDR), self.max_payload())
            };
            fill(buf);
            slot_seq.store(seq, Ordering::Release);
            self.write_seq().store(seq + 1, Ordering::Release);
        }

        /// 讀者：把第 seq 筆交給 `f`（f 通常把資料複製進 dora 的輸出 sample）。
        /// 回傳 false 代表這格在讀的過程中被寫者蓋掉了（讀者太慢），f 的結果要丟掉。
        pub fn read(&self, seq: u64, f: impl FnOnce(&[u8])) -> bool {
            let off = self.slot_off(seq);
            let slot_seq = self.atomic(off);
            if slot_seq.load(Ordering::Acquire) != seq {
                return false;
            }
            let buf = unsafe { std::slice::from_raw_parts(self.map.as_ptr().add(off + SLOT_HDR), self.max_payload()) };
            f(buf);
            fence(Ordering::Acquire);
            slot_seq.load(Ordering::Relaxed) == seq
        }
    }
}

// ─────────────────── Xilinx AXI DMA（PG021）S2MM ───────────────────
/// 只列本篇用到的 S2MM（Stream to Memory-Mapped，也就是 PL → DDR）direct register mode 暫存器。
pub mod axidma {
    pub const S2MM_DMACR: usize = 0x30;
    pub const S2MM_DMASR: usize = 0x34;
    pub const S2MM_DA: usize = 0x48;
    pub const S2MM_DA_MSB: usize = 0x4C;
    pub const S2MM_LENGTH: usize = 0x58;

    pub const CR_RS: u32 = 1 << 0;
    pub const CR_RESET: u32 = 1 << 2;
    pub const CR_IOC_IRQ_EN: u32 = 1 << 12;
    pub const CR_ERR_IRQ_EN: u32 = 1 << 14;

    pub const SR_HALTED: u32 = 1 << 0;
    pub const SR_IDLE: u32 = 1 << 1;
    pub const SR_DMA_INT_ERR: u32 = 1 << 4;
    pub const SR_DMA_SLV_ERR: u32 = 1 << 5;
    pub const SR_DMA_DEC_ERR: u32 = 1 << 6;
    pub const SR_IOC_IRQ: u32 = 1 << 12;
    pub const SR_ERR_IRQ: u32 = 1 << 14;
    pub const SR_ERRORS: u32 = SR_DMA_INT_ERR | SR_DMA_SLV_ERR | SR_DMA_DEC_ERR | SR_ERR_IRQ;

    /// PL 串流進來的每一塊資料前面放的 header（這是本篇自訂的 FPGA 格式，不是 Xilinx 規定）。
    /// 0x00 magic u32 | 0x04 frame u32 | 0x08 n_samples u32 | 0x0C pad | 0x10 t_ns u64 | 0x18 i16 樣本…
    pub const FRAME_MAGIC: u32 = 0xADC0_FEED;
    pub const FRAME_HDR: usize = 24;

    /// MMIO 暫存器視窗：一律 volatile 存取，一次 32 位元，不能用 memcpy／一般 load。
    pub struct Regs {
        map: memmap2::MmapRaw,
    }

    impl Regs {
        pub fn new(map: memmap2::MmapRaw) -> Self {
            assert!(map.len() >= 0x100);
            Regs { map }
        }
        pub fn read(&self, off: usize) -> u32 {
            unsafe { std::ptr::read_volatile(self.map.as_ptr().add(off) as *const u32) }
        }
        pub fn write(&self, off: usize, v: u32) {
            unsafe { std::ptr::write_volatile(self.map.as_mut_ptr().add(off) as *mut u32, v) }
        }
    }
}
