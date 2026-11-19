# lesson-18 範例

效能調校：release／fast profile、記憶體複製、佇列大小、CPU 綁定。`dataflow.tpl.yml` 由 `gen.sh` 代入參數產生 `run/*.yml`。

說明見 [第 18 篇](../../docs/lessons/lesson-18.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-18   # 自 repo 根目錄
./build-all.sh                               # dev、release、fast 各編一次
./gen.sh run/p-release.yml release raw 262144 5 2000 0 10 p-release   # 產生一份 yml
dora validate run/p-release.yml
./run-exp.sh p-release release raw 262144 5 2000 0 10   # 產生並執行（需要 dora 在 PATH）
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
