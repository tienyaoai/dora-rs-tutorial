# 版本與驗證紀錄

## 教材基準版本

| 元件 | 版本 |
|------|------|
| dora CLI | **1.1.0** |
| `dora-node-api`（範例 `Cargo.toml`） | **1.1.0** |
| Rust（原文主機） | **1.99.1** |
| 平台（原文） | **x86_64 Linux** |

Windows 與 macOS **未**列入原文驗證範圍。

---

## 各課原文驗證（原作者，台北時間）

以下為課文內記載之實測；**非**本 GitHub repo 建立時的自動重跑結果。

| 課 | 驗證日 | 摘要 |
|----|--------|------|
| 1 | 2026-11-01 前後 | `dora build` / `dora run` 成功；輸出見 [lesson-01.md](docs/lessons/lesson-01.md) |
| 2 | 2026-11-02 | 官方 `rust-dataflow` 首次 cargo ~1m30s；`dora run` 結束碼 0；sink 100 則 |
| 3 | 2026-11-03 | `validate` / `build` / `run` 結束碼 0；含 strict-types 示範 |
| 4 | 2026-11-04 | `dora run` 結束碼 0；含 `--stop-after` 行為說明 |
| 5 | 2026-11-05 | `dora run` 結束碼 0；Arrow + metadata |
| 6 | 2026-11-06 | `up` / `start` / `logs` / `stop` / `down` 等結束碼如課文；`status` 在 down 後為 1 |
| 7 | 2026-11-07 | `validate` / `build` / `run` 結束碼 0；最終 `total=210`；錯誤輸出名／錯誤節點 id 之 `validate` 結束碼 1（課文聲明，見下文重驗） |

完整終端機 log **未**納入 repo（含第 6 課課文曾提及的 `run-lifecycle.log`、第 7 課交接用 `run.log`）。

---

## 本 repo 維護時重驗（2026-11-06，Cloud Agent）

環境：Linux x86_64，Rust **1.99.1**（stable），**未**安裝 dora CLI 時僅做編譯檢查。

| 檢查 | 結果 |
|------|------|
| `cargo check` @ `examples/lesson-01` | 通過 |
| `cargo check` @ `examples/lesson-03` | 通過 |
| `cargo check` @ `examples/lesson-04` | 通過 |
| `cargo check` @ `examples/lesson-05` | 通過 |
| `cargo check` @ `examples/lesson-06` | 通過 |
| `dora validate` @ `examples/lesson-01,03-06` | **通過**（dora-cli 1.1.0，2026-11-06） |
| `dora run` / 生命週期實跑 | **未執行**（避免長時間常駐 coordinator；請依課文自行驗證） |
| 第 2 課上游 `rust-dataflow` | **未重編**；請依 [examples/lesson-02/README.md](examples/lesson-02/README.md) 自行驗證 |

重驗紀錄以 PR 描述與 `CHANGELOG.md` 為準；通過 `cargo check` **不代表** dataflow 執行路徑已端到端測試。

---

## 本 repo 維護時重驗（2026-11-07，第 7 課同步）

環境：Linux x86_64，Rust **1.99.1**（stable），dora-cli **1.1.0**。

| 檢查 | 結果 |
|------|------|
| `python3 scripts/check-markdown-nav.py` | **通過**（18 個 `.md`） |
| `cargo check` / `cargo build` @ `examples/lesson-07` | **通過**（Rust 1.99.1） |
| `dora validate` @ `examples/lesson-07/dataflow.yml` | **通過**（dora-cli 1.1.0） |
| `dora run`（自然結束，**未**使用 timeout） | **通過**（結束碼 0；stdout 最後 `管線累計結果：210`） |
| 負例 validate（`ticker/WRONG`、`tickerr/count`） | **通過**（各結束碼 1；訊息與課文摘要一致；僅 `/tmp` 臨時 YAML） |

Windows 與 macOS **未**實測。

---

## 內容來源包（建檔用）

公開內容最初整理自內部來源 zip（SHA256 `8e8a9cf6431e60a32d43221dbf1badb6ea5b96e3cfb6b411d52a7de67c4fb5a8`）。第 7 課來自 `dora-rs-lesson-07-source-20261107.zip`（SHA256 `ad4fc51d192868f3970cb408388831027277582ed7bfc97aecda337dcc45e02f`）。zip、移轉清單與完整 raw log**未**納入本 repo。
