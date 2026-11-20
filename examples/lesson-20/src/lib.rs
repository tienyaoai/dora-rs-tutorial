//! 第 20 篇共用程式：RGB 畫布、畫框、5×7 點陣字、ASCII 預覽、輸出成 RGB PNG／framebuffer 格式。
//! 只用標準函式庫＋png crate，不依賴任何圖形環境，所以 x86_64、aarch64（ZYNQ）都能直接編。
pub use lesson_19::{BBox, env_or, now_ns, pct, summary_us};

/// RGB888 畫布（每像素 3 byte，列優先）
pub struct Canvas { pub w: usize, pub h: usize, pub px: Vec<u8> }

pub type Rgb = [u8; 3];
pub const GREEN: Rgb = [40, 220, 60];
pub const RED: Rgb = [235, 50, 40];
pub const YELLOW: Rgb = [250, 210, 40];
pub const WHITE: Rgb = [245, 245, 245];

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self { Self { w, h, px: vec![0; w * h * 3] } }

    /// 灰階影像（gw×gh）用最近鄰放大 scale 倍畫進畫布（畫布大小必須是 gw*scale × gh*scale）
    pub fn blit_gray(&mut self, g: &[u8], gw: usize, gh: usize, scale: usize) {
        debug_assert_eq!((self.w, self.h), (gw * scale, gh * scale));
        for y in 0..self.h {
            let src = &g[(y / scale) * gw..(y / scale + 1) * gw];
            let dst = &mut self.px[y * self.w * 3..(y + 1) * self.w * 3];
            for (x, d) in dst.chunks_exact_mut(3).enumerate() {
                let v = src[x / scale];
                d.copy_from_slice(&[v, v, v]);
            }
        }
    }
    #[inline]
    pub fn put(&mut self, x: isize, y: isize, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }
    /// 實心矩形；alpha 0..=255（255 = 不透明），用來畫半透明的文字底色
    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: Rgb, alpha: u16) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                let i = (yy * self.w + xx) * 3;
                for k in 0..3 {
                    self.px[i + k] = ((self.px[i + k] as u16 * (255 - alpha) + c[k] as u16 * alpha) / 255) as u8;
                }
            }
        }
    }
    /// 空心外框，線寬 t
    pub fn rect(&mut self, b: BBox, t: usize, c: Rgb) {
        let (x0, y0) = (b.x as isize, b.y as isize);
        let (x1, y1) = ((b.x + b.w) as isize - 1, (b.y + b.h) as isize - 1);
        for d in 0..t as isize {
            for x in x0 - d..=x1 + d { self.put(x, y0 - d, c); self.put(x, y1 + d, c); }
            for y in y0 - d..=y1 + d { self.put(x0 - d, y, c); self.put(x1 + d, y, c); }
        }
    }
    /// 用 5×7 點陣字寫字，每個點放大成 s×s；回傳寫完後的 x
    pub fn text(&mut self, x: usize, y: usize, s: usize, c: Rgb, txt: &str) -> usize {
        let mut cx = x;
        for ch in txt.chars() {
            let g = glyph(ch.to_ascii_uppercase());
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) != 0 {
                        for dy in 0..s { for dx in 0..s {
                            self.put((cx + col * s + dx) as isize, (y + row * s + dy) as isize, c);
                        }}
                    }
                }
            }
            cx += 6 * s;
        }
        cx
    }
    /// 終端機 ASCII 預覽：縮成 cols×rows，用亮度挑字元
    pub fn ascii(&self, cols: usize, rows: usize) -> String {
        const RAMP: &[u8] = b" .:-=+*#%@";
        let mut s = String::with_capacity((cols + 1) * rows);
        for r in 0..rows {
            for c in 0..cols {
                // 一格涵蓋畫布上一塊區域：區域裡只要有外框顏色（純綠／純紅）就標 G／R（線很細，只取樣中心點會漏掉）
                let (x0, x1, y0, y1) = (c * self.w / cols, (c + 1) * self.w / cols, r * self.h / rows, (r + 1) * self.h / rows);
                let mut mark = None;
                for y in y0..y1 { for x in x0..x1 {
                    let i = (y * self.w + x) * 3;
                    let (pr, pg) = (self.px[i], self.px[i + 1]);
                    if pg > 180 && pr < 100 { mark = Some('G'); }
                    if pr > 180 && pg < 100 { mark = Some('R'); }
                }}
                if let Some(m) = mark { s.push(m); continue; }
                let i = (((y0 + y1) / 2) * self.w + (x0 + x1) / 2) * 3;
                let p = &self.px[i..i + 3];
                let l = (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8;
                s.push(RAMP[(l as usize * (RAMP.len() - 1)) / 255] as char);
            }
            s.push('\n');
        }
        s
    }
    /// 轉成 framebuffer 常見格式寫進 dst 的一列（bpp=32：B,G,R,X；bpp=16：RGB565 little-endian）
    pub fn row_to_fb(&self, y: usize, bpp: usize, dst: &mut [u8]) {
        let src = &self.px[y * self.w * 3..(y + 1) * self.w * 3];
        match bpp {
            32 => for (d, p) in dst.chunks_exact_mut(4).zip(src.chunks_exact(3)) { d.copy_from_slice(&[p[2], p[1], p[0], 0xff]); },
            16 => for (d, p) in dst.chunks_exact_mut(2).zip(src.chunks_exact(3)) {
                let v = ((p[0] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[2] as u16 >> 3);
                d.copy_from_slice(&v.to_le_bytes());
            },
            _ => {}
        }
    }
    pub fn save_png(&self, path: &str) -> eyre::Result<()> {
        let f = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(f, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&self.px)?;
        Ok(())
    }
}

/// 5×7 點陣字（每列 5 bit，最高位在左）。只收這個專題會用到的字；不認得的字畫成實心方塊。
pub fn glyph(c: char) -> [u8; 7] {
    let b = |r: [&str; 7]| r.map(|s| u8::from_str_radix(s, 2).unwrap());
    match c {
        '0' => b(["01110","10001","10011","10101","11001","10001","01110"]),
        '1' => b(["00100","01100","00100","00100","00100","00100","01110"]),
        '2' => b(["01110","10001","00001","00010","00100","01000","11111"]),
        '3' => b(["11111","00010","00100","00010","00001","10001","01110"]),
        '4' => b(["00010","00110","01010","10010","11111","00010","00010"]),
        '5' => b(["11111","10000","11110","00001","00001","10001","01110"]),
        '6' => b(["00110","01000","10000","11110","10001","10001","01110"]),
        '7' => b(["11111","00001","00010","00100","01000","01000","01000"]),
        '8' => b(["01110","10001","10001","01110","10001","10001","01110"]),
        '9' => b(["01110","10001","10001","01111","00001","00010","01100"]),
        'A' => b(["01110","10001","10001","11111","10001","10001","10001"]),
        'B' => b(["11110","10001","10001","11110","10001","10001","11110"]),
        'C' => b(["01110","10001","10000","10000","10000","10001","01110"]),
        'D' => b(["11100","10010","10001","10001","10001","10010","11100"]),
        'E' => b(["11111","10000","10000","11110","10000","10000","11111"]),
        'F' => b(["11111","10000","10000","11110","10000","10000","10000"]),
        'G' => b(["01110","10001","10000","10111","10001","10001","01111"]),
        'I' => b(["01110","00100","00100","00100","00100","00100","01110"]),
        'K' => b(["10001","10010","10100","11000","10100","10010","10001"]),
        'L' => b(["10000","10000","10000","10000","10000","10000","11111"]),
        'M' => b(["10001","11011","10101","10101","10001","10001","10001"]),
        'N' => b(["10001","10001","11001","10101","10011","10001","10001"]),
        'O' => b(["01110","10001","10001","10001","10001","10001","01110"]),
        'P' => b(["11110","10001","10001","11110","10000","10000","10000"]),
        'Q' => b(["01110","10001","10001","10001","10101","10010","01101"]),
        'R' => b(["11110","10001","10001","11110","10100","10010","10001"]),
        'S' => b(["01111","10000","10000","01110","00001","00001","11110"]),
        'T' => b(["11111","00100","00100","00100","00100","00100","00100"]),
        'U' => b(["10001","10001","10001","10001","10001","10001","01110"]),
        'X' => b(["10001","10001","01010","00100","01010","10001","10001"]),
        '.' => b(["00000","00000","00000","00000","00000","01100","01100"]),
        ':' => b(["00000","01100","01100","00000","01100","01100","00000"]),
        '%' => b(["11000","11001","00010","00100","01000","10011","00011"]),
        '#' => b(["01010","01010","11111","01010","11111","01010","01010"]),
        '/' => b(["00000","00001","00010","00100","01000","10000","00000"]),
        '=' => b(["00000","00000","11111","00000","11111","00000","00000"]),
        '-' => b(["00000","00000","00000","11111","00000","00000","00000"]),
        ' ' => [0; 7],
        _ => [0x1f; 7],
    }
}
