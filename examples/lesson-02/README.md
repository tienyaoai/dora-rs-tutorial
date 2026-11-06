# 第 2 課範例：官方 `rust-dataflow`（上游 monorepo）

第 2 課教材說明如何安裝 dora CLI，並在 **dora-rs 官方儲存庫** 內執行 `examples/rust-dataflow/`。

本目錄**不是**可獨立編譯的 Cargo 專案。`dataflow.yml` 裡的 `build: cargo build -p rust-dataflow-example-*` 與 `path: ../../target/debug/...` 都假設你已在 **dora monorepo 根目錄** 建好 workspace。

## 建議做法（可重現）

在任意工作目錄（教材常用 `~/dora`，你也可以換成自己喜歡的路徑）：

```bash
mkdir -p ~/dora && cd ~/dora
export PATH=$HOME/dora/bin:$PATH   # 依你安裝 dora CLI 的位置調整

# 1. 安裝 dora CLI v1.1.0（x86_64 Linux 預編譯範例）
curl -sLO https://github.com/dora-rs/dora/releases/download/v1.1.0/dora-cli-x86_64-unknown-linux-gnu.tar.gz
tar xzf dora-cli-x86_64-unknown-linux-gnu.tar.gz
mkdir -p bin && cp dora-cli-x86_64-unknown-linux-gnu/dora bin/
dora --version   # 應為 dora-cli 1.1.0

# 2. 取得與教材一致的官方原始碼
git clone --depth 1 --branch v1.1.0 https://github.com/dora-rs/dora.git repo

# 3. 編譯並執行 rust-dataflow
cd repo
cargo build -p rust-dataflow-example-node \
            -p rust-dataflow-example-status-node \
            -p rust-dataflow-example-sink
cd examples/rust-dataflow
dora run dataflow.yml
```

也可用 `dora build dataflow.yml`（在 `examples/rust-dataflow/` 目錄下）代替手動 `cargo build`。

## 上游來源與授權

- 範例原始碼：<https://github.com/dora-rs/dora/tree/v1.1.0/examples/rust-dataflow>
- dora-rs 專案授權：**Apache License 2.0**（見上游 `LICENSE`）
- 本繁中教材對程式碼的解說為原創；**請勿**將教材文字視為 dora-rs 官方文件

## 為什麼不把快照放進本 repo？

來源包曾含 `rust-dataflow` 目錄快照，但缺少 monorepo 根部的 workspace `Cargo.toml` / `Cargo.lock`，**無法**在本 repo 內單獨重現原文的編譯流程。公開副本也容易與 crates.io 解析結果漂移；因此改為以上游 tag 為準。

## 相關教材

- [第 2 篇：安裝 dora CLI，跑官方 rust-dataflow 範例](../../docs/lessons/lesson-02.md)
- [返回教材總覽](../../README.md)
