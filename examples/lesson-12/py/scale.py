"""Python 節點：收 Rust 送來的 Float32 陣列，用 numpy 放大、算平均，再送回 Rust。"""

import os

import numpy as np
import pyarrow as pa
from dora import Node


def main():
    node = Node()
    print(f"[scale.py] pid={os.getpid()} pyarrow={pa.__version__} numpy={np.__version__}")
    n = 0
    for event in node:
        if event["type"] == "INPUT" and event["id"] == "samples":
            n += 1
            arr = event["value"]  # pyarrow.FloatArray（Arrow Float32）
            meta = event["metadata"]  # dict：Rust 的 Parameter 已轉成 Python 型別
            if n == 1:
                print(f"[scale.py] value 型別={type(arr).__name__} arrow={arr.type} 長度={len(arr)}")
                print(
                    "[scale.py] metadata="
                    + str({k: (type(v).__name__, v) for k, v in sorted(meta.items()) if k != "timestamp"})
                )
                print(f"[scale.py] 另有 timestamp 鍵，型別={type(meta['timestamp']).__name__}")

            x = arr.to_numpy(zero_copy_only=True)  # float32、無 null → 不複製
            scaled = x * np.float32(meta["gain"])  # 保持 float32（NumPy 2 規則）
            mean = float(x.mean())  # Python float = f64
            seq = meta["seq"]

            # ① 陣列 + metadata（只挑要轉傳的鍵，不要整包 meta 丟回去）
            node.send_output(
                "scaled",
                pa.array(scaled),  # numpy float32 → Arrow Float32
                {"seq": seq, "mean": mean, "unit": meta["unit"],
                 "t_send_ns": meta["t_send_ns"], "by": "python"},
            )
            # ② 一列的結構化資料：int64 / float64 / 字串 / List<int64>
            summary = pa.StructArray.from_arrays(
                [
                    pa.array([seq], type=pa.int64()),
                    pa.array([mean], type=pa.float64()),
                    pa.array([f"{meta['unit']}#{seq}"]),
                    pa.array([np.rint(scaled).astype(np.int64)], type=pa.list_(pa.int64())),
                ],
                names=["seq", "mean", "label", "rounded"],
            )
            node.send_output("summary", summary, {"seq": seq})
        elif event["type"] == "STOP":
            break
    print(f"[scale.py] 總共處理 {n} 筆")


if __name__ == "__main__":
    main()
