# lesson-07 範例

四節點管線：`ticker → multiply → accumulate → printer`，含 `ticker/count` 扇出與 `printer` 扇入。

說明見 [第 7 篇](../../docs/lessons/lesson-07.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-07   # 自 repo 根目錄
dora validate dataflow.yml
dora build dataflow.yml
dora run dataflow.yml
```

預期管線結束時 `printer [total]` 最後一則為 **210**（原文作者 2026-11-07 Linux 實測）。
