//! 三個節點共用的小工具：印出「我在哪個 daemon 底下」。
use std::time::{SystemTime, UNIX_EPOCH};

/// 節點啟動時印一行身分：node id、PID、父行程 PID（= 啟動它的 dora daemon）。
/// 同一台電腦上跑兩個 daemon 時，看父 PID 就知道節點被排到哪一個。
pub fn whoami(name: &str, node_id: &str) {
    let host = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "?".into());
    eprintln!(
        "[{name}] 啟動 node_id={node_id} host={host} pid={} 父pid(daemon)={}",
        std::process::id(),
        std::os::unix::process::parent_id()
    );
}

/// 目前時間（Unix 奈秒）。跨機器比較前要先做時間同步（chrony/PTP）。
pub fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}
