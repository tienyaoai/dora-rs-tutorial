# lesson-10 範例

`send_output_raw` / `send_output`、`TryFrom` 借用讀取 vs `.to_vec()`，以及 `DORA_ZERO_COPY_THRESHOLD`；三份 YAML（`raw.yml`、`vec.yml`、`noshm.yml`）。

說明見 [第 10 篇](../../docs/lessons/lesson-10.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-10   # 自 repo 根目錄
dora validate raw.yml && dora validate vec.yml && dora validate noshm.yml
dora build raw.yml
dora run raw.yml
```

作者 2026-11-10 三輪 benchmark 的 CSV 與節錄日誌見 `rounds/`；詳見課文與 [VERSIONS.md](../../VERSIONS.md)。
