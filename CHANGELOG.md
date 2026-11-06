# Changelog

## 2026-11-06 — 初版公開整理

- 新增 6 篇完整繁中教材（`docs/lessons/lesson-01.md`～`lesson-06.md`），含導覽、目錄與 repo 相對連結。
- 新增第 1、3～6 課可編譯範例（`examples/lesson-01/`、`lesson-03/`～`lesson-06/`）。
- 第 2 課改以上游 dora-rs `v1.1.0` 官方 `rust-dataflow` 為準（`examples/lesson-02/README.md`），不納入無法獨立建置的 monorepo 快照。
- 新增 `README.md` 入口、`VERSIONS.md`、`NOTICE.md`、`.gitignore`。
- 維護時重驗：`cargo check` 通過 lesson-01、03～06；未執行 `dora validate` / `dora run`。
