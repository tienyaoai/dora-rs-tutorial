# Changelog

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
