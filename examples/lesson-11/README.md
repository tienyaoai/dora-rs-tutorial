# lesson-11 範例

Operator vs Node：同一段邏輯（ticker → double → acc → sink）用 Node 與 Operator 兩種寫法；`multi-op.yml` 刻意示範失敗（一個 runtime 節點放兩個 operator）。

說明見 [第 11 篇](../../docs/lessons/lesson-11.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-11   # 自 repo 根目錄
dora validate operator.yml && dora validate node.yml && dora validate mixed.yml && dora validate multi-op.yml
cargo build --release        # 先建整個 workspace
dora run operator.yml --stop-after 3s; echo "exit=$?"
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
