# lesson-08 範例

快 producer × 慢 consumer：計時器頻率、`queue_size`、`queue_policy`（`drop_oldest` / `backpressure`）與七份 YAML。

說明見 [第 8 篇](../../docs/lessons/lesson-08.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-08   # 自 repo 根目錄
for f in *.yml; do dora validate "$f"; done
dora build drop-latest.yml
dora run drop-latest.yml
```

詳見課文「怎麼跑」小節（含 `clock-no-exit.yml` 與 `timeout` 對照）。原文作者 2026-11-08 Linux 實測摘要見 [VERSIONS.md](../../VERSIONS.md)。
