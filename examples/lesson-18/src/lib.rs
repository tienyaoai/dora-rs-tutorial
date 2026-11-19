use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 現在的 Unix 時間（奈秒）。三個節點在同一台機器上，可以直接拿來算端到端延遲。
pub fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

/// 三點平滑濾波：out[i] = (in[i-1] + 2*in[i] + in[i+1]) / 4，邊界用自己。
/// 這就是本篇的「模擬工作」：每個位元組做一點算術，和真實的影像前處理同一個量級。
pub fn filter(src: &[u8], dst: &mut [u8]) {
    let n = src.len();
    assert_eq!(n, dst.len());
    if n < 3 {
        dst.copy_from_slice(src);
        return;
    }
    dst[0] = src[0];
    dst[n - 1] = src[n - 1];
    for (o, w) in dst[1..n - 1].iter_mut().zip(src.windows(3)) {
        *o = ((w[0] as u16 + 2 * w[1] as u16 + w[2] as u16) >> 2) as u8;
    }
}

/// 忙等 us 微秒（模擬「這個節點比較重」），用 spin 而不是 sleep，才會真的佔 CPU
pub fn busy_us(us: u64) {
    if us == 0 {
        return;
    }
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_micros(us) {
        std::hint::spin_loop();
    }
}

pub fn pct(v: &[i64], p: f64) -> i64 {
    if v.is_empty() {
        return 0;
    }
    v[(((v.len() - 1) as f64) * p).round() as usize]
}
