"""同一段邏輯寫成 Python operator（呼應第 11 篇）：由 dora runtime 載入，呼叫 on_event。"""

import os

import numpy as np
import pyarrow as pa
from dora import DoraStatus


class Operator:
    def __init__(self):
        self.n = 0
        print(f"[op_scale.py] pid={os.getpid()}")

    def on_event(self, dora_event, send_output) -> DoraStatus:
        if dora_event["type"] == "INPUT" and dora_event["id"] == "samples":
            self.n += 1
            meta = dora_event["metadata"]
            x = dora_event["value"].to_numpy(zero_copy_only=True)
            scaled = x * np.float32(meta["gain"])
            mean = float(x.mean())
            seq = meta["seq"]
            send_output(
                "scaled",
                pa.array(scaled),
                {"seq": seq, "mean": mean, "unit": meta["unit"],
                 "t_send_ns": meta["t_send_ns"], "by": "python"},
            )
            summary = pa.StructArray.from_arrays(
                [
                    pa.array([seq], type=pa.int64()),
                    pa.array([mean], type=pa.float64()),
                    pa.array([f"{meta['unit']}#{seq}"]),
                    pa.array([np.rint(scaled).astype(np.int64)], type=pa.list_(pa.int64())),
                ],
                names=["seq", "mean", "label", "rounded"],
            )
            send_output("summary", summary, {"seq": seq})
        return DoraStatus.CONTINUE

    def __del__(self):
        print(f"[op_scale.py] 總共處理 {self.n} 筆")
