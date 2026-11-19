//! 兩個節點共用：回報「我實際跑在什麼 CPU、什麼核心、什麼 glibc 上」。
use std::ffi::CStr;

/// 編譯時就決定的目標架構（std::env::consts）——跟執行時的 uname 比對用。
pub fn compiled_for() -> String {
    format!(
        "{}-{}（指標寬度 {} 位元）",
        std::env::consts::ARCH,
        std::env::consts::OS,
        usize::BITS
    )
}

/// 執行時的 uname(2)：machine 欄位就是 `uname -m`（ZYNQ MP 會是 aarch64、ZYNQ-7000 是 armv7l）。
pub fn uname() -> String {
    // SAFETY: utsname 是純 C 結構，全 0 是合法初始值；uname 只寫入它。
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return "uname 失敗".into();
    }
    let f = |s: &[libc::c_char]| unsafe { CStr::from_ptr(s.as_ptr()) }.to_string_lossy().into_owned();
    format!("{} {} {}", f(&u.sysname), f(&u.release), f(&u.machine))
}

/// 執行時載入的 glibc 版本（gnu_get_libc_version）。
pub fn glibc() -> String {
    unsafe extern "C" {
        fn gnu_get_libc_version() -> *const libc::c_char;
    }
    unsafe { CStr::from_ptr(gnu_get_libc_version()) }
        .to_string_lossy()
        .into_owned()
}

pub fn report(name: &str) -> String {
    format!(
        "[{name}] 編譯目標={} 執行中 uname={} glibc={} pid={}",
        compiled_for(),
        uname(),
        glibc(),
        std::process::id()
    )
}
