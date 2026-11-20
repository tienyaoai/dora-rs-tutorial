# lesson-20 範例（最終篇）

綜合專題（下）：camera → detect → **display**（畫框＋字，輸出 `mp4`／`png`／`ascii`／`fb`／`window`），以及交叉編譯到 aarch64（ZYNQ）的部署。`camera`、`detect` 與第 19 篇逐字相同。

依賴（路徑依賴）：`../lesson-19`（→ `../lesson-17`）。請先依第 17 篇執行 `examples/lesson-17/fetch-data.sh`；`dataflow-file.yml` 還需要第 19 篇的 `clip.mp4`／`clip.labels`（`examples/lesson-19/make-clip.sh 300` 產生，需要 `ffmpeg`）。

說明見 [第 20 篇](../../docs/lessons/lesson-20.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-20   # 自 repo 根目錄
for f in dataflow*.yml; do dora validate $f; done
cargo build --release
./run-demo.sh dataflow.yml 60          # 合成來源 → mp4＋png＋ascii（需要 ffmpeg）
# 真視窗（X11）：cargo build --release --features window --bin display && ./run-demo.sh dataflow-window.yml 60
# aarch64（qemu-user）：先設 DORA、QEMU_LD_PREFIX，見課文與 run-aarch64.sh
```

作者的完整執行日誌（`run/*.log`）、`display.mp4`、截圖與假 framebuffer 檔未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
