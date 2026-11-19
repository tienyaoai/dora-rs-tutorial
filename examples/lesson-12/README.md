# lesson-12 範例

Rust 與 Python 節點混用：`node.yml`（Python node）與 `operator.yml`（Python operator）。需要先建 Python venv 並安裝 `dora-rs==1.1.0`、`pyarrow`、`numpy`。

說明見 [第 12 篇](../../docs/lessons/lesson-12.md)。

```bash
# 若 `dora` 不在 PATH，先加入你安裝 dora CLI v1.1.0 的目錄
cd examples/lesson-12   # 自 repo 根目錄
python3 -m venv .venv && ./.venv/bin/pip install -r py/requirements.txt
source .venv/bin/activate     # 讓 dora 找到裝了 dora-rs 的 python
cargo build --release
dora validate node.yml && dora validate operator.yml
dora run node.yml --stop-after 3s; echo "exit=$?"
```

作者的完整執行日誌（`run/*.log`）未納入本 repo；課文中的輸出為節錄。見 [VERSIONS.md](../../VERSIONS.md)。
