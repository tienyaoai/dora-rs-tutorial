//! 不經過 dora 的微基準：只量「配置＋複製」vs「重用緩衝區」，排除行程間通訊的雜訊。
//! 三種寫法做的 filter 完全一樣，差別只在記憶體：
//!   reuse  ：filter 寫進事先配好、重複使用的緩衝區（等同 send_output_raw 寫進 dora 配好的緩衝區）
//!   alloc  ：每筆 vec![0; n] 再 filter（配置＋清零）
//!   alloc+copy：alloc 之後再 to_vec() 一次（等同 send_output 把 Vec 再編碼／複製進輸出緩衝區）
use lesson_18::{filter, pct};
use std::time::Instant;

fn main() {
    let size: usize = lesson_18::env_or("SIZE", 262_144);
    let iters: usize = lesson_18::env_or("ITERS", 2000);
    let rounds: usize = lesson_18::env_or("ROUNDS", 5);
    let src: Vec<u8> = (0..size).map(|i| (i * 7) as u8).collect();
    let mut reuse_buf = vec![0u8; size];
    let mut sink = 0u64;
    println!("micro SIZE={size} ITERS={iters}（每輪每種寫法各 {iters} 次，前 100 次暖機不計），單位 µs");
    for r in 1..=rounds {
        let mut res = [Vec::new(), Vec::new(), Vec::new()];
        for i in 0..iters {
            let t = Instant::now();
            filter(&src, &mut reuse_buf);
            sink += reuse_buf[size / 2] as u64;
            let a = t.elapsed().as_nanos() as i64;

            let t = Instant::now();
            let mut v = vec![0u8; size];
            filter(&src, &mut v);
            sink += v[size / 2] as u64;
            let b = t.elapsed().as_nanos() as i64;

            let t = Instant::now();
            let mut v = vec![0u8; size];
            filter(&src, &mut v);
            let c2 = v.to_vec();
            sink += c2[size / 2] as u64;
            let c = t.elapsed().as_nanos() as i64;
            if i >= 100 {
                res[0].push(a);
                res[1].push(b);
                res[2].push(c);
            }
        }
        for v in res.iter_mut() {
            v.sort();
        }
        println!(
            "round {r}: reuse p50={:.1} p99={:.1} | alloc p50={:.1} p99={:.1} | alloc+copy p50={:.1} p99={:.1}",
            pct(&res[0], 0.5) as f64 / 1e3, pct(&res[0], 0.99) as f64 / 1e3,
            pct(&res[1], 0.5) as f64 / 1e3, pct(&res[1], 0.99) as f64 / 1e3,
            pct(&res[2], 0.5) as f64 / 1e3, pct(&res[2], 0.99) as f64 / 1e3,
        );
    }
    println!("(sink={sink})");
}
