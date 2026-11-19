# dora-rs 教學 第 15 篇：交叉編譯 aarch64——把 Rust 節點與 dataflow 部署到 ZYNQ

> 版本：dora v1.1.0（CLI、`dora-node-api` 1.1.0），rustc 1.99.1，cargo-zigbuild（ziglang 0.17.0），qemu-user 10.0.13。驗證日期：2026-11-15（台北時間）。**沒有實體 ZYNQ 板**：「板子上」的結果都是在 x86_64 虛擬機上用 qemu-user（qemu-aarch64／qemu-arm＋binfmt_misc）模擬，搭配從 Debian 套件解出的 glibc 2.28 與 2.41 sysroot 模擬新舊 rootfs。實測：aarch64、armv7 都編譯成功（glibc 符號最高 `GLIBC_2.28`）；x86_64 本機、qemu 下的官方 aarch64 dora CLI、x86 daemon 帶 aarch64／armv7 節點都跑通，0 遺失；「x86 coordinator＋x86 daemon＋aarch64 daemon（qemu）」跨架構分散式也跑通（送 49 收 49）；官方 aarch64 CLI 在 glibc 2.28 上**起不來**（要 `GLIBC_2.34`）；zynq profile 把 aarch64 節點從 25.0 MB 縮到 11.9 MB。**沒驗證**：實機 Cortex-A53／A9 的效能與時序、真的 scp 到板子、PetaLinux 實際 rootfs、自己重編 dora CLI（見文末）。

> **關於日誌：** 本篇引用的 `run/*.log` 等完整執行日誌**未**納入本 repo（`.gitignore` 排除 `*.log` 與 `run/`）；課文中的輸出是作者 2026-11 實測的節錄。範例可依「怎麼跑／執行」小節自行重現。

> **本 repo 說明：** `run-distributed.sh` 需要自備 aarch64 版 `dora` 與 sysroot，請以環境變數 `ZYNQ_DORA`、`ZYNQ_SYSROOT` 指定（原作者機器上的路徑未納入）。

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 14 篇](lesson-14.md)
- 下一課：[第 16 篇 →](lesson-16.md)
- 本課範例：[examples/lesson-15/](../../examples/lesson-15/)

## 目錄

- [重點說明](#重點說明)
- [完整範例](#完整範例)
- [實際執行](#實際執行)
- [常見錯誤](#常見錯誤)
- [小練習](#小練習)
- [本篇沒有驗證成功的部分](#本篇沒有驗證成功的部分)
## 重點說明

第 14 篇的兩個 daemon 都是 x86_64。真正的 ZYNQ 是 ARM，板子上通常也沒有 cargo（編 dora 依賴樹要好幾 GB 記憶體），所以做法是：**在工作站上交叉編譯 → 縮小 → scp 到板子 → 板子上的 daemon 直接執行**。

### 1. 兩代 ZYNQ，兩個 Rust target

| 晶片 | 處理器 | 架構 | Rust target | `uname -m` | 指標寬度 |
|---|---|---|---|---|---|
| ZYNQ UltraScale+ MPSoC／RFSoC | 4×（或 2×）Cortex-A53 | ARMv8-A，64 位元 | `aarch64-unknown-linux-gnu` | `aarch64` | 64 |
| ZYNQ-7000 | 1～2× Cortex-A9 | ARMv7-A，32 位元，硬體浮點（VFPv3＋NEON） | `armv7-unknown-linux-gnueabihf` | `armv7l` | 32 |

要注意的差異：

- **ZYNQ-7000 不能跑 aarch64 執行檔**；ZYNQ MP 核心雖多半開了 `CONFIG_COMPAT`，但 rootfs 通常沒有 armhf glibc，也跑不了 armv7。兩代各編一份。
- `gnueabihf` 的 **hf** 是 hard-float ABI。ZYNQ-7000 的 PetaLinux 是 hf；`arm-unknown-linux-gnueabi`（軟浮點）會要 `ld-linux.so.3`，放上去找不到 interpreter。
- **官方 dora v1.1.0 的 Linux CLI 只有 x86_64 和 aarch64**（查 GitHub release 資產），沒有 armv7：ZYNQ MP 可直接用官方 dora 當 daemon，ZYNQ-7000 要自己交叉編譯 dora CLI（本篇沒驗證成功，見文末）。
- 32 位元下 `usize` 是 4 byte：`as usize` 截斷、`AtomicU64`（armv7 用 `ldrexd/strexd`，較慢）要多留意。

### 2. glibc 版本：交叉編譯最常踩的坑

Rust 執行檔動態連結 glibc，**要求的 glibc 符號版本不能比板子新**。在工作站（Debian trixie，glibc 2.41）直接編，產物會要 `GLIBC_2.34`；PetaLinux 的 glibc 跟著 Yocto 版本走（thud 2.28、zeus 2.30、gatesgarth 2.32、honister 2.34、kirkstone 2.35、langdale 2.36、scarthgap 2.39），舊專案常停在 2.28～2.32。**以板子上 `ldd --version` 為準。**

cargo-zigbuild 可以在 target 後面加 glibc 版本，連結時用 zig 附的舊版符號表：

```bash
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28
cargo zigbuild --release --target armv7-unknown-linux-gnueabihf.2.28
```

產物還是放在 `target/aarch64-unknown-linux-gnu/release/`（目錄名不含 `.2.28`）。

**官方 aarch64 dora CLI 要 glibc ≥ 2.34**（`readelf -V` 最高 `GLIBC_2.34`，實測在 2.28 上起不來）。板子 ≥ 2.34 → 官方 CLI 當 daemon＋自己編的節點；更舊 → 節點用 `.2.28` 編沒問題，daemon 要自己編。

### 3. 交叉編譯工具：為什麼用 cargo-zigbuild

只做 `rustup target add` 就 `cargo build --target …` **不會成功**：dora-node-api 的依賴樹裡有 `ring`（`cargo tree -i ring`：zenoh → quinn → ring），build script 要編 C／組語，找不到 `aarch64-linux-gnu-gcc` 就失敗（見「常見錯誤」）。可選的做法：

- **cargo-zigbuild**（本篇用）：zig 當 C 編譯器兼連結器，不用 Docker、不用 sysroot，glibc 版本一個後綴搞定。
- `cross`：Docker 容器裡有完整工具鏈（dora repo 也附 `Cross.toml`），但容器的 glibc 版本固定。
- `gcc-aarch64-linux-gnu` 或 **PetaLinux SDK**（`petalinux-build --sdk`）：在 `.cargo/config.toml` 設 `linker`、另設 `CC_aarch64_unknown_linux_gnu`；用 SDK 的好處是 glibc 與板子完全一致。

後兩種本篇沒有實測。

### 4. dataflow.yml 怎麼指到交叉編譯產物

有 `build:` 時，dora 會在**目標那台機器**上跑該指令，再執行 `path:`。所以 ZYNQ 上的節點：

- **不要寫 `build:`**（寫了，`dora build` 時 ZYNQ daemon 會嘗試在板子上跑 cargo）。
- `path:` 寫**板子上的絕對路徑**（第 14 篇：路徑在目標機解析，工作目錄是 `_work/<build-id>/`，相對路徑容易對不上）。
- 用第 14 篇的 `deploy.machine: zynq` 把節點釘到 ZYNQ 的 daemon（1.0 起鍵名是 `deploy`，舊的 `_unstable_deploy` 寫了會報錯）。也可以 daemon 帶 `--labels arch=aarch64`、YAML 用 `deploy.labels`，但第 14 篇實測 labels 會讓直連慢一拍。
- `deploy.distribute` 有 `local`／`scp`／`http`，`dora validate` 也接受 `distribute: scp`；但在 1.1.0 原始碼 `rg` 全專案，`DistributeStrategy` **只有定義、沒有程式碼使用**，寫了不會幫你傳檔。本篇自己 scp。

### 5. 縮小執行檔：release profile

在 `Cargo.toml` 加自訂的 `zynq` profile（見下方範例註解）：`opt-level = "s"`、`lto = "fat"`、`codegen-units = 1`、`panic = "abort"`、`strip = "symbols"`。用自訂 profile 而不改 `[profile.release]`，本機開發版就保有符號與 panic backtrace；`panic = "abort"` 時節點 panic 直接結束行程，daemon 當作節點失敗處理。

## 完整範例

位置：[examples/lesson-15/](../../examples/lesson-15/)

```
examples/lesson-15/
├── Cargo.toml
├── dataflow.yml            # x86_64 本機版（有 build:）
├── dataflow-aarch64.yml    # 直接跑 aarch64 產物（沒有 build:）
├── dataflow-armv7.yml      # 直接跑 armv7 產物
├── dataflow-zynq.yml       # 分散式：sysinfo → zynq，sink → workstation
├── deploy-zynq.sh          # 交叉編譯 → 縮小 → scp
├── run-distributed.sh      # 一台電腦模擬 x86 工作站＋aarch64 板子
├── setup-qemu.sh           # 註冊 qemu-user 的 binfmt_misc
├── run/                    # 實測 log
└── src/
    ├── lib.rs              # compiled_for()、uname()、glibc()
    └── bin/
        ├── sysinfo.rs      # 送出「我跑在哪」
        └── sink.rs         # 接收並印出
```

兩個節點都會印出**編譯時的目標**（`std::env::consts::ARCH`）、**執行時的 `uname(2)`** 和**實際載入的 glibc 版本**，一眼就能看出節點有沒有放對機器、用對 glibc。

### Cargo.toml

```toml
[package]
name = "lesson-15"
version = "0.1.0"
edition = "2024"

[dependencies]
dora-node-api = "1.1.0"
eyre = "0.6"
libc = "0.2"

# 給 ZYNQ 用的「小檔案」設定：cargo build --profile zynq
[profile.zynq]
inherits = "release"
opt-level = "s"        # 以大小為優先（"z" 更小但通常更慢）
lto = "fat"            # 跨 crate 最佳化，順便丟掉沒用到的程式碼
codegen-units = 1      # 單一編譯單元，最佳化較徹底（編譯較慢）
panic = "abort"        # 不帶 unwind 表；panic 直接結束行程
strip = "symbols"      # 連結後去掉符號表（等同 strip）
```

### src/lib.rs

```rust
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
```

### src/bin/sysinfo.rs

```rust
//! 邊緣端節點（要放到 ZYNQ）：每個 tick 送出一行「我是誰」的文字，metadata 帶 seq。
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId};

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let me = lesson_15::report("sysinfo");
    eprintln!("{me}");
    let out = DataId::from("info".to_owned());
    let mut seq: i64 = 0;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, .. } if id.as_str() == "tick" => {
                seq += 1;
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                node.send_output(out.clone(), p, format!("seq={seq} {me}").as_str())?;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[sysinfo] 結束，共送出 {seq} 筆");
    Ok(())
}
```

### src/bin/sink.rs

```rust
//! 接收端節點：印出收到的內容，同時印自己的架構——兩邊架構不同就代表真的跨機器／跨架構了。
use dora_node_api::{DoraNode, Event, get_integer_param};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;
    eprintln!("{}", lesson_15::report("sink"));
    let (mut count, mut last_seq) = (0u64, 0i64);
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "info" => {
                let s: &str = (&data).try_into()?;
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                if count == 0 || seq % 10 == 0 {
                    println!("[sink] 收到 {s}");
                }
                count += 1;
                last_seq = seq;
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[sink] 結束，共收到 {count} 筆，最後 seq={last_seq}");
    Ok(())
}
```

### dataflow-aarch64.yml

`dataflow.yml`（本機版）就是第 14 篇的寫法：`build: cargo build --release --bin …`、`path: target/release/…`。交叉編譯版拿掉 `build:`，路徑指到 target 目錄：

```yaml
nodes:
  - id: sysinfo
    path: target/aarch64-unknown-linux-gnu/release/sysinfo
    inputs:
      tick: dora/timer/millis/200
    outputs:
      - info
  - id: sink
    path: target/aarch64-unknown-linux-gnu/release/sink
    inputs:
      info: sysinfo/info
```

`dataflow-armv7.yml` 只把路徑換成 `target/armv7-unknown-linux-gnueabihf/release/…`。

### dataflow-zynq.yml（分散式）

```yaml
# sysinfo 派到 ZYNQ（aarch64 daemon），sink 留在 x86_64 工作站
nodes:
  - id: sysinfo
    deploy:
      machine: zynq                          # 對應板子上 dora daemon --machine-id zynq
    # 沒有 build:——板子上沒有 cargo，執行檔事先交叉編譯、用 scp 放好
    path: /opt/dora-nodes/lesson-15/sysinfo  # 在「板子」的檔案系統上解析
    inputs:
      tick: dora/timer/millis/200
    outputs:
      - info

  - id: sink
    deploy:
      machine: workstation
    build: cargo build --release --bin sink
    path: target/release/sink
    inputs:
      info: sysinfo/info
```

### deploy-zynq.sh

```bash
#!/usr/bin/env bash
# 交叉編譯 → 縮小 → 送到板子。BOARD 例：petalinux@192.168.1.20
# 沒有板子時設 BOARD=local，改用本機 cp 模擬（本篇實測就是這樣）。
set -euo pipefail
TARGET=${TARGET:-aarch64-unknown-linux-gnu}   # ZYNQ-7000 改 armv7-unknown-linux-gnueabihf
GLIBC=${GLIBC:-2.28}                          # 不超過板子 rootfs 的 glibc
PROFILE=${PROFILE:-zynq}
BOARD=${BOARD:-local}
DEST=/opt/dora-nodes/lesson-15

cargo zigbuild --profile "$PROFILE" --target "$TARGET.$GLIBC" --bin sysinfo --bin sink
OUT=target/$TARGET/$PROFILE
file "$OUT/sysinfo"
ls -l "$OUT/sysinfo" "$OUT/sink"

if [ "$BOARD" = local ]; then
  mkdir -p "$DEST" && install -m755 "$OUT/sysinfo" "$OUT/sink" "$DEST/"
else
  ssh "$BOARD" "mkdir -p $DEST"
  scp "$OUT/sysinfo" "$OUT/sink" "$BOARD:$DEST/"
  ssh "$BOARD" "file $DEST/sysinfo 2>/dev/null; ldd --version | head -1"
fi
echo "部署完成：$DEST"
```

板子上的 daemon（ZYNQ MP、glibc ≥ 2.34；`dora` 是官方 aarch64 tar.gz 解出來的）：

```bash
scp dora petalinux@192.168.1.20:/usr/local/bin/
ssh petalinux@192.168.1.20 dora daemon --coordinator-addr 192.168.1.10 --machine-id zynq --labels arch=aarch64
```

`run-distributed.sh`（見專案目錄）在同一台 x86_64 上開 coordinator、`workstation` daemon、用 qemu 執行的 aarch64 `zynq` daemon，`dora start dataflow-zynq.yml` 10 秒後 `ps`、`dora stop`，再把 `_work/*/out/<uuid>/log_*.jsonl` 整理到 `run/dist-*/node-logs.txt`。

## 實際執行

```bash
cd examples/lesson-15   # 自 repo 根目錄
rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf

# 1) x86_64 本機：證明程式本身沒問題
cargo build --release && dora run dataflow.yml --stop-after 5s

# 2) 交叉編譯
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28
cargo zigbuild --release --target armv7-unknown-linux-gnueabihf.2.28

# 3) 檢查產物
file target/aarch64-unknown-linux-gnu/release/sysinfo
readelf -h target/armv7-unknown-linux-gnueabihf/release/sysinfo | grep -E 'Class|Machine|Flags'
readelf -A target/armv7-unknown-linux-gnueabihf/release/sysinfo     # CPU/FPU 屬性
readelf -V target/aarch64-unknown-linux-gnu/release/sysinfo | grep -o 'GLIBC_[0-9.]*' | sort -uV

# 4) 沒有板子：qemu-user + binfmt_misc（setup-qemu.sh 會掛載並註冊 qemu-aarch64／qemu-arm，需要 root）
./setup-qemu.sh
export QEMU_LD_PREFIX=$HOME/sysroot/aarch64-glibc2.41  # 模擬板子 rootfs；依你的 sysroot 位置調整
$HOME/aarch64/dora-cli-aarch64-unknown-linux-gnu/dora run dataflow-aarch64.yml --stop-after 5s

# 5) 縮小＋部署，再跑分散式
./deploy-zynq.sh                 # BOARD=petalinux@192.168.1.20 ./deploy-zynq.sh 就是真的 scp
./run-distributed.sh
```

這台虛擬機的 apt 鏡像回 500，qemu 是直接下載 Debian 13 的 `qemu-user` .deb 用 `dpkg -i` 裝（Debian 13 起它已是靜態連結，`qemu-user-static` 只是過渡套件）。sysroot 是把 Debian trixie（glibc 2.41）與 buster（glibc 2.28）的 arm64／armhf `libc6`、libgcc 套件用 `dpkg-deb -x` 解出來。

### 實測輸出（2026-11-15，台北時間）

**產物檢查**（`file`／`readelf`，節錄）：

```
aarch64 sysinfo: ELF 64-bit LSB pie executable, ARM aarch64, … interpreter /lib/ld-linux-aarch64.so.1, for GNU/Linux 2.0.0, stripped
armv7   sysinfo: ELF 32-bit LSB pie executable, ARM, EABI5 … interpreter /lib/ld-linux-armhf.so.3, for GNU/Linux 2.0.0, stripped
armv7 -h: Class ELF32  Machine ARM  Flags 0x5000400, Version5 EABI, hard-float ABI
armv7 -A: Tag_CPU_arch: v7  Tag_FP_arch: VFPv3  Tag_Advanced_SIMD_arch: NEONv1
aarch64 NEEDED: libm.so.6 libc.so.6 libpthread.so.0 libdl.so.2 librt.so.1
aarch64 GLIBC 符號: GLIBC_2.17 GLIBC_2.18 GLIBC_2.25 GLIBC_2.28     （armv7 同樣最高 2.28）
x86_64 本機 release 最高 GLIBC_2.34；官方 aarch64 dora CLI 最高 GLIBC_2.34
```

建置時間（8 核虛擬機，從零開始）：x86_64 release 1 分 47 秒、aarch64 1 分 43 秒、armv7 1 分 40 秒。

**x86_64 本機**（`dora run dataflow.yml`，5 秒）：

```
[sink] 編譯目標=x86_64-linux（指標寬度 64 位元） 執行中 uname=Linux 6.12.113+ x86_64 glibc=2.41 pid=2028959
[sysinfo] 結束，共送出 23 筆
[sink] 結束，共收到 23 筆，最後 seq=23
```

**qemu：官方 aarch64 dora CLI＋aarch64 節點**（glibc 2.41 sysroot）：送 20 收 20。

```
$ qemu-aarch64 -L sysroot/aarch64-glibc2.41 dora --version
dora-cli 1.1.0
[sink] 收到 seq=20 [sysinfo] 編譯目標=aarch64-linux（指標寬度 64 位元） 執行中 uname=Linux 6.12.113+ aarch64 glibc=2.41 …
```

同一個官方 CLI 換 glibc 2.28 sysroot（模擬舊 PetaLinux）：

```
dora: /lib/aarch64-linux-gnu/libm.so.6: version `GLIBC_2.29' not found (required by dora)
dora: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.34' not found (required by dora)
…（另有 2.30、2.32、2.33）  exit=1
```

**我們的節點在 glibc 2.28 上**（x86 daemon 經 binfmt 啟動 aarch64／armv7 節點）：

```
[sink] 編譯目標=aarch64-linux（指標寬度 64 位元） 執行中 uname=Linux 6.12.113+ aarch64 glibc=2.28 …   → 送 23 收 23
[sink] 編譯目標=arm-linux（指標寬度 32 位元） 執行中 uname=Linux 6.12.113+ armv7l glibc=2.28 …      → 送 24 收 24
```

armv7 版是 32 位元、`armv7l`，和 ZYNQ-7000 一樣。（qemu-user 只模擬 CPU，核心版本 6.12.113+ 是主機的；真板子會是 PetaLinux 的 xilinx 核心。）

**跨架構分散式**（`run-distributed.sh`，sysinfo 用 zynq profile 產物）：

```
DAEMON ID                                         LAST HEARTBEAT  RESTARTS
workstation-01a1dc79-1751-742c-aeac-31545cb4947a  3409ms ago      0
zynq-01a1dc79-18cf-73e9-9c47-b3bde28dc732         3209ms ago      0
start 結束碼 0
    PID    PPID COMMAND
2065845 2065741 .../bin/dora daemon … --machine-id workstation
2065846 2065741 /usr/bin/qemu-aarch64 …/dora daemon … --machine-id zynq
2066542 2065845 …/lesson-15/target/release/sink
2066562 2065846 /usr/bin/qemu-aarch64 /opt/dora-nodes/lesson-15/sysinfo   ← aarch64 daemon 生出 aarch64 節點
sink: [sink] 收到 seq=40 [sysinfo] 編譯目標=aarch64-linux（指標寬度 64 位元） … aarch64 glibc=2.41 pid=2066562
sink: [sink] 結束，共收到 49 筆，最後 seq=49
sysinfo: [sysinfo] 結束，共送出 49 筆
```

x86_64 的 sink 收到 aarch64 節點的資料，中間經過兩個不同架構的 daemon 和 zenoh，Arrow 資料不用任何轉換（兩邊都是 little-endian）。

**檔案大小**（`sysinfo`，byte）：

| 產物 | 大小 | gzip -9 | 最高 GLIBC |
|---|---|---|---|
| x86_64 release（未 strip） | 38,619,056 | 11,153,592 | 2.34 |
| x86_64 release 再 `strip` | 27,345,208 | — | 2.34 |
| aarch64 release（zigbuild） | 25,021,416 | 9,169,276 | 2.28 |
| **aarch64 zynq profile** | **11,935,784** | **4,912,252** | 2.28 |
| armv7 release（zigbuild） | 21,743,108 | 8,701,978 | 2.28 |
| **armv7 zynq profile** | **11,297,180** | **5,091,685** | 2.28 |

`sink` 幾乎一樣（aarch64 zynq 11,843,360、armv7 zynq 11,212,524）。zynq profile 約砍一半，主要來自 `lto` 和 `opt-level="s"`；zigbuild 的 release 版已沒有 `.symtab`（`file` 顯示 stripped），所以 strip 對它幫助不大。代價是編譯時間：aarch64 3 分 24 秒（同時有別的編譯在跑）、armv7 2 分 33 秒，部署時才用。zynq profile 版在 glibc 2.28 上也實測跑通（送 24 收 24）。

## 常見錯誤

1. **只加 target 就 `cargo build --target`**（實測）：
   ```
   warning: ring@0.17.14: … failed to find tool "aarch64-linux-gnu-gcc": No such file or directory
   error: failed to run custom build command for `ring v0.17.14`
   ```
   是 C 依賴需要交叉 C 編譯器。改用 `cargo zigbuild`，或裝交叉 gcc 並設 linker 與 `CC_aarch64_unknown_linux_gnu`。
2. **`version 'GLIBC_2.34' not found`**：在新 glibc 的工作站直接編，或把官方 dora CLI 放到舊 PetaLinux。節點用 `.2.28`（換成板子 `ldd --version` 的版本）；daemon 要板子 ≥ 2.34 或自己編。
3. **忘了 scp（或路徑打錯）**：實測把板子上的 `sysinfo` 移走後 `dora start`：
   ```
   WARN   sysinfo on daemon `zynq`: daemon::pending node exited before initializing dora connection
   ERROR  coordinator spawn failed on daemon `zynq-…`: failed to prepare node sysinfo
     hint: if nodes require building, run `dora build <dataflow.yml>` first
   start 結束碼 1
   ```
   訊息沒說找不到檔案，提示還叫你 `dora build`。看到 `failed to prepare node` 先 `ssh 板子 ls -l <path>`。
4. **架構放錯**：aarch64 執行檔放到 ZYNQ-7000 會 `Exec format error`；armhf 放到沒有 armhf rootfs 的板子會出現看似矛盾的 `No such file or directory`（找不到 interpreter）。部署前 `file` 看一眼（本篇無實機，沒實測）。
5. **ZYNQ 節點寫了 `build: cargo build …`**：分散式 `dora build` 會在目標機上跑它，板子沒有 cargo 就會失敗（依 build 在目標機執行的行為推論，本篇沒實測）。ZYNQ 節點只寫 `path:`。
6. **qemu 跑得通 ≠ 板子跑得通**：qemu-user 時序完全不同；加了 `-C target-cpu=cortex-a53`（或 `cortex-a9`）這類最佳化，以實機為準。

## 小練習

**題目**：做 ZYNQ-7000 版的分散式部署——把 `sysinfo` 編成 `armv7-unknown-linux-gnueabihf.2.28`（zynq profile），daemon 帶 `--labels arch=armv7`，YAML 改用 `deploy.labels`，讓同一份 YAML 只換 daemon 標籤就能在兩代 ZYNQ 間切換；sink 要印出資料來自 32 還是 64 位元。

提示：
- `TARGET=armv7-unknown-linux-gnueabihf ./deploy-zynq.sh`；兩個架構別放同一個目錄，可改成 `/opt/dora-nodes/lesson-15/$TARGET/`。
- 官方沒有 armv7 dora CLI。沒有板子時，讓 **x86** daemon 帶 `--labels arch=armv7`、`QEMU_LD_PREFIX` 指 armhf sysroot，binfmt 會替它執行 armv7 節點（即 `run/x86-daemon-armv7-nodes-glibc228.log` 的做法）。
- 位元數別解析字串：metadata 加 `Parameter::Integer(usize::BITS as i64)`，sink 用 `get_integer_param` 讀。
- 加分：`opt-level = "z"` 或 `lto = "thin"`，比較大小與編譯時間。

## 本篇沒有驗證成功的部分

- **自己交叉編譯 dora CLI**（給 ZYNQ-7000 或 glibc < 2.34 的板子當 daemon）：`cargo zigbuild --release -p dora-cli --target aarch64-unknown-linux-gnu.2.28` 第一次因虛擬機沒有 `make`（`openssl-src` 要編 OpenSSL）失敗；補裝後第二次失敗在 `dora-core` 的 `file not found for module build`——本機那份 dora 原始碼複本缺 `libraries/core/src/build/`，不是交叉編譯本身的問題。依「同一步卡兩次就停」沒再試，完整 git clone 下能否編過、能否在 armv7 上跑都**未驗證**。
- `cross`、交叉 gcc、PetaLinux SDK 只有說明；真的 scp／ssh 到板子沒實測（`deploy-zynq.sh` 的 `BOARD=local` 用 `install` 模擬）。

收尾：`rm -rf target _work out`（x86_64／aarch64／armv7 的 release 與 zynq 產物共約 5.2 GB），`/opt/dora-nodes` 模擬目錄也已刪除。

下一篇：[第 16 篇](lesson-16.md)講自訂硬體節點：用 Rust 寫序列埠（UART）節點、和其他行程交換大量資料的共享記憶體，以及在 ZYNQ 上把 PL 端 FPGA 的 DMA 緩衝區接進 dora dataflow 的概念。

---

**導覽**

- [返回教材總覽](../../README.md)
- 上一課：[第 14 篇](lesson-14.md)
- 下一課：[第 16 篇 →](lesson-16.md)
- 本課範例：[examples/lesson-15/](../../examples/lesson-15/)
