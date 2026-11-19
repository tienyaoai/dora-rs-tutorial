# lesson-14 範例

分散式部署：一條 dataflow 拆到兩個 daemon（`edge`／`compute`）；`dataflow.yml` 用 `machine:`，`dataflow-pinned.yml` 用 labels，`dataflow-missing.yml` 刻意示範失敗。需要先自行啟動 coordinator 與兩個 daemon（見課文）。

說明見 [第 14 篇](../../docs/lessons/lesson-14.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-14   # 自 repo 根目錄
dora validate dataflow.yml && dora validate dataflow-pinned.yml && dora validate dataflow-missing.yml
cargo build --release
# coordinator 與 daemon 啟動方式見課文，之後：
./run-demo.sh
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
