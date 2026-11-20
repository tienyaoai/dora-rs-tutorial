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
| 8 | 2026-11-08 | 七份 YAML `validate` 0；`drop-latest` 收到 11/40、`drop-default` 20/40、`backpressure`／`matched`／`clock` 40/40；`clock-no-exit` 以 `timeout` 結束碼 124 為預期 |
| 9 | 2026-11-09 | `on-failure` 結束碼 0（重啟 2 次）；`never` 與 `exhausted` 結束碼 1 |
| 10 | 2026-11-10 | 三輪 benchmark 的 CSV 與節錄日誌見 `examples/lesson-10/rounds/`；`raw.yml` 結束碼 0 |
| 11 | 2026-11-11 | 4 份 YAML `validate` 通過；`operator`／`node`／`mixed` 用 `--stop-after 3s` 結束碼 0、收 25 筆；`multi-op.yml` 刻意示範失敗（結束碼 1） |
| 12 | 2026-11-12 | 2 份 YAML `validate` 通過；`node.yml`／`operator.yml` 結束碼 0、各 25 筆；不啟用 venv 的負例結束碼 1 |
| 13 | 2026-11-13 | `cargo test --release` 單元 4＋整合 3 通過；`check.sh` PASS（改壞預期值示範 FAIL 結束碼 1）；`dataflow-bad.yml` 結束碼 1 |
| 14 | 2026-11-14 | 單機以 coordinator＋兩個 `--machine-id` daemon 模擬兩台機器；**未**跨實體機器、未經防火牆／NAT |
| 15 | 2026-11-15 | aarch64／armv7 交叉編譯成功；以 qemu-user 模擬板子；**無**實體 ZYNQ 板 |
| 16 | 2026-11-16 | UART／共享記憶體／DMA 皆為模擬；**無**真實硬體；`dora run` 12 秒結束碼 0 |
| 17 | 2026-11-17 | candle 與 ort 兩種 MNIST 推論；離線量測 candle 98.09%；無 GPU、未上 ZYNQ |
| 18 | 2026-11-18 | release／fast／debug、佇列大小、CPU 綁定量測；共用虛擬機雜訊大；無 `perf`、無 flamegraph |
| 19 | 2026-11-19 | 合成畫面與 H.264 影片檔兩種來源；真攝影機（V4L2）未驗證；未上 ZYNQ |
| 20 | 2026-11-20 | 合成來源→mp4／png／ascii、影片檔→假 framebuffer、Xvfb 視窗（`--features window`）、aarch64 交叉編譯＋qemu-user 實跑；無真攝影機、無 `/dev/fb0`、無 ZYNQ 板；FPGA 加速僅為說明 |

完整終端機 log **未**納入 repo（含第 6 課課文曾提及的 `run-lifecycle.log`、第 7～9 課及第 11～20 課作者本機 `run-*.log`、`run/` 內的日誌與 `out/` session 紀錄）。

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

## 本 repo 維護時重驗（2026-11-09，第 8～9 課同步）

環境：Linux x86_64，Rust **1.99.1**（stable），dora-cli **1.1.0**。

| 檢查 | 結果 |
|------|------|
| `python3 scripts/check-markdown-nav.py` | **通過**（22 個 `.md`） |
| `cargo check` / `cargo build` @ `examples/lesson-08` | **通過** |
| `cargo check` / `cargo build` @ `examples/lesson-09` | **通過** |
| `dora validate` @ `examples/lesson-08/*.yml`（7 份） | **通過** |
| `dora validate` @ `examples/lesson-09/*.yml`（3 份） | **通過** |
| `dora run drop-latest.yml` | **通過**（結束碼 0；consumer `11/40`） |
| `dora run on-failure.yml` | **通過**（結束碼 0；sink `NodeRestarted 2 次`） |
| 其餘 lesson-08 YAML / lesson-09 `never`・`exhausted` | **未執行**（依課文與作者紀錄；請自行驗證） |

Windows 與 macOS **未**實測。

---

## 本 repo 維護時重驗（2026-11-10，第 10 課同步）

環境：Linux x86_64，Rust **1.99.1**（stable），dora-cli **1.1.0**。

| 檢查 | 結果 |
|------|------|
| `python3 scripts/check-markdown-nav.py` | **通過**（24 個 `.md`） |
| `cargo check` / `cargo build --release` @ `examples/lesson-10` | **通過** |
| `dora validate` @ `examples/lesson-10/*.yml`（3 份） | **通過** |
| `dora run raw.yml` | **通過**（結束碼 0；sink `bad=0`） |
| `vec.yml` / `noshm.yml` 完整 benchmark | **未執行**（依課文與作者 `rounds/` 紀錄；請自行驗證） |

Windows 與 macOS **未**實測。

---

## 本 repo 維護時重驗（2026-11-19，第 11～19 課同步）

環境：Linux x86_64（8 核虛擬機，無 GPU、無攝影機），Rust **1.99.1**（stable），dora-cli **1.1.0**。

| 檢查 | 結果 |
|------|------|
| `python3 scripts/check-markdown-nav.py` | **通過**（42 個 `.md`） |
| `cargo check --all-targets` @ `examples/lesson-11`～`lesson-19`（第 11 課為 `--workspace`） | **通過**（9 個專案；各課 `Cargo.lock` 與作者原檔相同） |
| `cargo test`（dev profile）@ `examples/lesson-13` | **通過**（單元＋整合測試，0 失敗） |
| `dora validate` @ 第 11～17、19 課全部 `*.yml`（24 份）與第 18 課以 `gen.sh` 產生的一份 | **通過** |
| `bash -n` 全部 `.sh`、`py_compile` 第 12 課 Python | **通過** |
| `dora run`／實際執行／交叉編譯／qemu／真硬體／真攝影機 | **未執行**（依課文與作者紀錄；請自行驗證） |

Windows 與 macOS **未**實測。第 11～19 課的完整執行日誌、`data/`、`clip.mp4` 等產生物未納入本 repo。

---

## 本 repo 維護時重驗（2026-11-20，第 20 課同步，系列完結）

環境：Linux x86_64（8 核虛擬機，無 GPU、無攝影機），Rust **1.99.1**（stable），dora-cli **1.1.0**。

| 檢查 | 結果 |
|------|------|
| `python3 scripts/check-markdown-nav.py` | **通過**（43 個 `.md`） |
| `cargo check --all-targets` @ `examples/lesson-20`（預設 feature 與 `--features window`） | **通過**（Rust 1.99.1；`Cargo.lock` 與作者原檔相同） |
| `dora validate` @ `examples/lesson-20/*.yml`（5 份） | **通過** |
| `bash -n` @ `run-demo.sh`、`run-aarch64.sh` | **通過** |
| `dora run`／交叉編譯／qemu／Xvfb 視窗／真 framebuffer／真攝影機 | **未執行**（依課文與作者紀錄；請自行驗證） |

Windows 與 macOS **未**實測。第 20 課的完整執行日誌、`display.mp4`、截圖、假 framebuffer 檔未納入本 repo。

---

## 內容來源包（建檔用）

公開內容最初整理自內部來源 zip（SHA256 `8e8a9cf6431e60a32d43221dbf1badb6ea5b96e3cfb6b411d52a7de67c4fb5a8`）。第 7 課來自 `dora-rs-lesson-07-source-20261107.zip`（SHA256 `ad4fc51d192868f3970cb408388831027277582ed7bfc97aecda337dcc45e02f`）。第 8～9 課來自 `dora-lessons-08-09-source.zip`（SHA256 `59342fe7326c18e1d0734d9d365f002646895f21c251466316d78f798a01bfff`）。第 10 課來自 `lesson10-upload.tar.gz`（SHA256 `b1e8972baa2d111698ab53108f35ac60809b3470d4046271bbdf370c9c868388`）。zip、移轉清單與完整 raw log**未**納入本 repo。 第 11～20 課來自作者的本機教材目錄（`dora-tutorial/`、`dora-poc/lessons/`），不含 `target/`、日誌與大型產生物。
