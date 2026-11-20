# Changelog

## 2026-11-20 — 第 20 篇（最終篇），系列完結

- 新增 [第 20 篇](docs/lessons/lesson-20.md)（綜合專題下：顯示節點、完整管線整合與 ZYNQ 部署收尾、20 篇總複習）與 `examples/lesson-20/`。
- 更新 `README.md`（已發布 1～20 課，系列完結）、第 19 課導覽、第 10 課頁尾的「下一篇」連結與 `scripts/refresh-lesson-nav.py`（支援第 20 課，並標示最終篇）。
- 路徑改為 repo 相對路徑；移除本機絕對路徑；課文內作業備註（清理本機暫存、「沒有 push」等）已刪除。
- 不納入：`target/`、`*.log`、`run/`、`out/`、`display.mp4`、截圖、假 framebuffer 檔，以及作者本機解壓的 qemu-user／`file` 工具目錄。
- 維護時重驗紀錄見 `VERSIONS.md` 與本輪 PR 描述。

## 2026-11-19 — 第 11～19 篇與範例

- 新增 [第 11 篇](docs/lessons/lesson-11.md)（Operator vs Node）、[第 12 篇](docs/lessons/lesson-12.md)（混用 Python 節點）、[第 13 篇](docs/lessons/lesson-13.md)（測試與除錯）、[第 14 篇](docs/lessons/lesson-14.md)（分散式部署）、[第 15 篇](docs/lessons/lesson-15.md)（交叉編譯到 ZYNQ／aarch64）、[第 16 篇](docs/lessons/lesson-16.md)（自訂硬體節點）、[第 17 篇](docs/lessons/lesson-17.md)（AI 推論節點）、[第 18 篇](docs/lessons/lesson-18.md)（效能調校）、[第 19 篇](docs/lessons/lesson-19.md)（綜合專題上）與 `examples/lesson-11/`～`lesson-19/`。
- 更新 `README.md`（已發布 1～19 課）、第 10 課導覽與 `scripts/refresh-lesson-nav.py`（支援第 11～19 課；頁尾切除改為只切最後的導覽區塊，避免課文內的 `---` 被截斷）。
- 路徑改為 repo 相對路徑；移除本機絕對路徑；課文內「完成後保留／沒有 push」之類的作業備註一句已刪除。
- 不納入：`target/`、`*.log`、`run/`、`out/`、MNIST `data/`、`clip.mp4`／`clip.labels`、第 17 課作者機器專用的 `.cargo/config.toml` 與 `linkfix/`（課文有說明）。
- 維護時重驗紀錄見 `VERSIONS.md` 與本輪 PR 描述。

## 2026-11-10 — 第 10 篇與零拷貝 benchmark 範例

- 新增 [第 10 篇](docs/lessons/lesson-10.md)（共享記憶體門檻、`send_output_raw`、benchmark）與 `examples/lesson-10/`（三份 YAML、`rounds/` 重現證據）。
- 更新 `README.md`（已發布 1～10 課）、第 9 課導覽與 `scripts/refresh-lesson-nav.py`（支援第 10 課）。
- 路徑改為 repo 相對路徑；移除本機絕對路徑引用；`rounds/*.log` 以 `.gitignore` 例外納入。
- 內容來源：`lesson10-upload.tar.gz`（SHA256 `b1e8972baa2d111698ab53108f35ac60809b3470d4046271bbdf370c9c868388`）。
- 維護時重驗紀錄見 `VERSIONS.md` 與本輪 PR 描述。

## 2026-11-09 — 第 8～9 篇與背壓／重啟範例

- 新增 [第 8 篇](docs/lessons/lesson-08.md)（計時器、佇列策略、背壓）與 `examples/lesson-08/`（七份 YAML）。
- 新增 [第 9 篇](docs/lessons/lesson-09.md)（錯誤處理、`restart_policy`）與 `examples/lesson-09/`。
- 更新 `README.md`（已發布 1～9 課）、第 7 課導覽與 `scripts/refresh-lesson-nav.py`（支援第 8～9 課）。
- 路徑改為 repo 相對路徑；移除本機路徑與未公開 log 引用。
- 內容來源：`dora-lessons-08-09-source.zip`（SHA256 `59342fe7326c18e1d0734d9d365f002646895f21c251466316d78f798a01bfff`）。
- 維護時重驗紀錄見 `VERSIONS.md` 與本輪 PR 描述。

## 2026-11-07 — 第 7 篇與四節點範例

- 新增 [第 7 篇](docs/lessons/lesson-07.md)（多節點管線、扇入／扇出）與 `examples/lesson-07/`。
- 更新 `README.md`（已發布 1～7 課）、第 6 課導覽與 `scripts/refresh-lesson-nav.py`（支援第 7 課）。
- 路徑改為 `examples/lesson-07`；移除本機絕對路徑與未公開 log 引用。
- 維護時重驗紀錄見 `VERSIONS.md` 與本輪 PR 描述。

## 2026-11-06 — 閱讀版面修整（文件 only）

- 六課與 `README.md`：將不適合小螢幕的寬表格改為分組條列／短段落（技術內容、程式、練習均保留）。
- 各課導覽改為多行清單（返回總覽、上一課／下一課、範例）；目錄 anchor 與 GitHub 標題 slug 對齊。
- 新增 `scripts/check-markdown-nav.py`、`scripts/refresh-lesson-nav.py` 供連結／導覽檢查。
- **範例程式未變**；`cargo check` / `dora validate` 結果仍沿用初版提交（commit `78ef920`）之程式版本。

## 2026-11-06 — 初版公開整理

- 新增 6 篇完整繁中教材（`docs/lessons/lesson-01.md`～`lesson-06.md`），含導覽、目錄與 repo 相對連結。
- 新增第 1、3～6 課可編譯範例（`examples/lesson-01/`、`lesson-03/`～`lesson-06/`）。
- 第 2 課改以上游 dora-rs `v1.1.0` 官方 `rust-dataflow` 為準（`examples/lesson-02/README.md`），不納入無法獨立建置的 monorepo 快照。
- 新增 `README.md` 入口、`VERSIONS.md`、`NOTICE.md`、`.gitignore`。
- 維護時重驗：`cargo check` 通過 lesson-01、03～06；未執行 `dora validate` / `dora run`。
