# lesson-15 範例

交叉編譯到 aarch64／armv7（ZYNQ）：`cargo zigbuild`、縮小產物、部署腳本；`run-distributed.sh` 在一台 x86_64 上用 qemu 模擬 x86 工作站＋aarch64 板子。

說明見 [第 15 篇](../../docs/lessons/lesson-15.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-15   # 自 repo 根目錄
dora validate dataflow.yml
cargo build --release && dora run dataflow.yml --stop-after 5s
# 交叉編譯、qemu 與分散式步驟見課文；run-distributed.sh 需設定 ZYNQ_DORA、ZYNQ_SYSROOT
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
