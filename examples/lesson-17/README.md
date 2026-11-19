# lesson-17 範例

Rust 推論節點：candle（safetensors）與 ONNX Runtime（`ort`）兩種 CNN 推論，MNIST 假攝影機。先執行 `./fetch-data.sh` 下載資料集與 `mnist-12.onnx`（`data/` 不納入版控）。

說明見 [第 17 篇](../../docs/lessons/lesson-17.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-17   # 自 repo 根目錄
./fetch-data.sh                              # MNIST ＋ mnist-12.onnx，核對 sha256
dora validate dataflow.yml && dora validate dataflow-slow.yml
cargo build --release
./run-demo.sh dataflow.yml 15
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
