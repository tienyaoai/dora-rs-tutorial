//! 產生測試影片的原始畫面：用和 camera 的 synth 一模一樣的合成器，寫出 N 張灰階 rawvideo（clip.y8）
//! 與每張的答案（clip.labels，每行一個數字）。再用 ffmpeg 壓成 clip.mp4（見 make-clip.sh）。
//! 用法：make-clip 輸出.y8 輸出.labels 張數 [寬 高]
use lesson_19::{Mnist, Synth};
use std::io::Write;

fn main() -> eyre::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    eyre::ensure!(a.len() >= 4, "用法：make-clip 輸出.y8 輸出.labels 張數 [寬 高]");
    let n: u64 = a[3].parse()?;
    let (w, h): (usize, usize) = (a.get(4).map_or(Ok(320), |s| s.parse())?, a.get(5).map_or(Ok(240), |s| s.parse())?);
    let ds = Mnist::load("../lesson-17/data", "t10k")?;
    let mut s = Synth::new(ds, w, h, 4, 15);
    let mut y8 = std::io::BufWriter::new(std::fs::File::create(&a[1])?);
    let mut lab = std::io::BufWriter::new(std::fs::File::create(&a[2])?);
    let mut buf = vec![0u8; w * h];
    for seq in 0..n {
        let (label, _) = s.render(seq, &mut buf);
        y8.write_all(&buf)?;
        writeln!(lab, "{label}")?;
    }
    println!("寫出 {n} 張 {w}x{h} 到 {}，答案到 {}", a[1], a[2]);
    Ok(())
}
