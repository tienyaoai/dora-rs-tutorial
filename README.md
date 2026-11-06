# dora-rs 繁體中文教學（Rust 入門到進階）

以 **Rust** 為主、從零認識 [dora-rs](https://github.com/dora-rs/dora) 的繁體中文系列教材。每課包含概念說明、完整程式、執行步驟與練習題；範例可在對應目錄依指令重現。

**版本基準：** dora CLI 與 `dora-node-api` **v1.1.0**  
**實測平台：** 原文作者於 **x86_64 Linux** 驗證（見 [VERSIONS.md](VERSIONS.md)）。**未**在 Windows / macOS 上實測，請自行評估差異。

---

## 這份教材在學什麼

dora-rs 用 **dataflow.yml** 描述節點之間的資料流，節點可用 Rust（以及 Python、C/C++ 等）撰寫。系列規劃 **20 篇**；目前 **第 1～6 篇** 已發布，其餘章節預留擴充位置（不捏造未寫完的內文）。

---

## 適合對象

- 會基本 Rust 與命令列，想學機器人／AI 管線的 **dataflow** 思維
- 不必先會 ROS 2（第 1 篇會對照說明）
- 願意在 Linux 上安裝 Rust toolchain 與 dora CLI

---

## 先備條件

| 項目 | 說明 |
|------|------|
| Rust | `rustup` 的 stable（教材撰寫時為 **1.99.1**；需支援 **edition 2024**） |
| dora CLI | **v1.1.0**，與節點 crate 同版（安裝見 [第 2 篇](docs/lessons/lesson-02.md)） |
| 作業系統 | 原文與範例以 **Linux x86_64** 為準；官方 CLI 亦有 aarch64 預編譯檔 |
| 磁碟 | 首次 `cargo build` / `dora build` 會產生大型 `target/`（debug 可達數 GB），練習完建議 `cargo clean` |
| glibc | 使用官方預編譯 CLI 時需 **glibc ≥ 2.34** |

---

## 環境準備（最短路徑）

1. 安裝 Rust：`https://rustup.rs`
2. 安裝 dora CLI v1.1.0 → [第 2 篇](docs/lessons/lesson-02.md)
3. Clone 本 repo，進入各課 `examples/lesson-NN/` 執行課文指令

```bash
git clone https://github.com/tienyaoai/dora-rs-tutorial.git
cd dora-rs-tutorial
# 例：第 1 課
export PATH=$HOME/dora/bin:$PATH
cd examples/lesson-01
dora build dataflow.yml && dora run dataflow.yml
```

第 2 課需在 **dora 官方 monorepo** 內執行 → [examples/lesson-02/README.md](examples/lesson-02/README.md)

---

## 推薦閱讀順序

依篇號 1 → 6 閱讀。第 1 篇建立概念；第 2 篇裝 CLI 並跑官方範例；第 3 篇讀 YAML；第 4～5 篇寫 Rust 節點；第 6 篇管理常駐 coordinator／daemon。

---

## 已發布課程（6 篇）

| 篇 | 標題 | 教材 | 範例 |
|----|------|------|------|
| 1 | dora-rs 是什麼？跟 ROS 2 差在哪 | [lesson-01.md](docs/lessons/lesson-01.md) | [examples/lesson-01/](examples/lesson-01/) |
| 2 | 安裝 dora CLI，跑官方 rust-dataflow 範例 | [lesson-02.md](docs/lessons/lesson-02.md) | [取得官方範例](examples/lesson-02/README.md) |
| 3 | 逐行讀懂 dataflow.yml | [lesson-03.md](docs/lessons/lesson-03.md) | [examples/lesson-03/](examples/lesson-03/) |
| 4 | 第一個像樣的 Rust 節點——`DoraNode` 與事件迴圈 | [lesson-04.md](docs/lessons/lesson-04.md) | [examples/lesson-04/](examples/lesson-04/) |
| 5 | Arrow 資料與 metadata——除了純量，還能送什麼？ | [lesson-05.md](docs/lessons/lesson-05.md) | [examples/lesson-05/](examples/lesson-05/) |
| 6 | `dora up / start / logs / down`——資料流的生命週期管理 | [lesson-06.md](docs/lessons/lesson-06.md) | [examples/lesson-06/](examples/lesson-06/) |

---

## 課程大綱（20 篇規劃）

| 篇 | 狀態 | 備註 |
|----|------|------|
| 1–6 | **已發布** | 見上表 |
| 7 | 尚未發布 | 原文僅寫「繼續往前」，無正式標題 |
| 8 | 尚未發布 | 暫定主題提示（非正式標題）：佇列／背壓 |
| 9 | 尚未發布 | 暫定：重啟策略 |
| 10 | 尚未發布 | 暫定：共享記憶體零拷貝 |
| 11–12 | 尚未發布 | — |
| 13 | 尚未發布 | 暫定：測試 |
| 14 | 尚未發布 | 暫定：分散式／daemon |
| 15 | 尚未發布 | 暫定：ZYNQ／aarch64 |
| 16–20 | 尚未發布 | — |

第 8～15 篇的「暫定主題」僅來自已發布課文中的交叉引用，**不代表最終標題**。

---

## 如何執行範例

- **第 1、3～6 課：** 在 repo 根目錄 `cd examples/lesson-NN`，依各課「怎麼跑」小節執行 `dora validate` / `dora build` / `dora run` 或生命週期指令。
- **第 2 課：** 必須 `git clone` [dora-rs v1.1.0](https://github.com/dora-rs/dora/tree/v1.1.0) 後在 `examples/rust-dataflow/` 操作；原因見 [examples/lesson-02/README.md](examples/lesson-02/README.md)。
- **安全提醒：** `dora down --force` 會強制停止 coordinator 上的資料流；**只在確認沒有他人共用服務時** 於自己的練習環境使用（第 6 課詳述）。

---

## 版本與驗證範圍

詳細日期、結束碼摘要與「原文驗證 vs 本 repo 重驗」分界見 **[VERSIONS.md](VERSIONS.md)**。

---

## 授權與第三方內容

- **繁中教材正文**（`docs/lessons/`、`README.md` 等）：由本 repo 維護者提供之教學內容；**本 repo 未設定整庫開源授權條款**，使用前請依你的用途自行判斷。
- **dora-rs 官方程式**（第 2 課引用之 `rust-dataflow` 等）：**Apache License 2.0**，見 [NOTICE.md](NOTICE.md) 與上游 [LICENSE](https://github.com/dora-rs/dora/blob/v1.1.0/LICENSE)。
- **第 1、3～6 課範例**：教學用 Rust 專案，依賴 crates.io 上的 `dora-node-api` 等套件（各套件自有授權）。

---

## 目錄結構

```text
README.md                 # 本頁（入口）
VERSIONS.md               # 版本與驗證紀錄
NOTICE.md                 # 上游授權說明
CHANGELOG.md              # 本 repo 變更紀錄
docs/lessons/lesson-01.md … lesson-06.md
examples/lesson-01/ … lesson-06/   # 第 2 課僅 README + 上游指引
scripts/                  # 維護用（處理課文導覽）
```

---

## 勘誤與貢獻

歡迎在 GitHub 以 Issue 或 Pull Request 指出錯字、版本漂移或可讀性改進。合併前請勿提交 `target/`、`.env`、完整執行 log 或含個人路徑的私有設定。
