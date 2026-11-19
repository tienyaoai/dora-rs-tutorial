# lesson-16 範例

自訂硬體節點：UART（以 pty 模擬）、共享記憶體 seqlock、模擬 AXI DMA；`fake-hw` 是模擬裝置，`run-demo.sh` 一鍵跑。所有硬體皆為模擬，未在真實硬體上驗證。

說明見 [第 16 篇](../../docs/lessons/lesson-16.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-16   # 自 repo 根目錄
dora validate dataflow.yml && dora validate dataflow-backpressure.yml
./run-demo.sh                                 # 需要 dora 在 PATH，或設 DORA=…
./run-demo.sh dataflow-backpressure.yml
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
