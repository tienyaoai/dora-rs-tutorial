# lesson-13 範例

測試與除錯：單元測試、不開 daemon 的整合測試（`DORA_TEST_WITH_INPUTS`）、黑箱比對（`check.sh`）、錄製重播（`recorded/`）。

說明見 [第 13 篇](../../docs/lessons/lesson-13.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-13   # 自 repo 根目錄
cargo test --release         # 單元＋整合測試
./check.sh                   # 黑箱：固定輸入 vs fixtures/expected.jsonl
dora validate dataflow.yml
dora run dataflow.yml --stop-after 3s; echo "exit=$?"
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
