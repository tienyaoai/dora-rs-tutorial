# lesson-19 範例

綜合專題：攝影機 → 辨識 → 結果輸出（合成畫面／H.264 影片檔／V4L2 設定）。依賴 `../lesson-17/`（程式庫、`data/`、`models/`），請先依第 17 篇準備。`clip.mp4` 為產生物，未納入版控。

說明見 [第 19 篇](../../docs/lessons/lesson-19.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-19   # 自 repo 根目錄
cargo build --release
./make-clip.sh 300                           # 產生 clip.mp4＋clip.labels（需要 ffmpeg）
dora validate dataflow.yml && dora validate dataflow-file.yml
./run-demo.sh dataflow.yml 60
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
