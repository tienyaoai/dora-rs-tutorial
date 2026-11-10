# lesson-09 範例

producer → worker（可模擬當機）→ sink：`restart_policy`、`NodeRestarted` / `NodeFailed` 事件。

說明見 [第 9 篇](../../docs/lessons/lesson-09.md)。

```bash
cd examples/lesson-09   # 自 repo 根目錄
dora validate on-failure.yml
dora build on-failure.yml
dora run on-failure.yml; echo "exit=$?"
```

另可執行 `never.yml`、`exhausted.yml`（見課文）。原文作者 2026-11-09 Linux 實測摘要見 [VERSIONS.md](../../VERSIONS.md)。
