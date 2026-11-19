#!/usr/bin/env bash
# 假設 coordinator 與兩個 daemon（edge、gpu-box）已在跑；依序跑三個情境。
set -u
export DORA_COORDINATOR_ADDR=${DORA_COORDINATOR_ADDR:-127.0.0.1}
mkdir -p run
for f in dataflow dataflow-pinned; do
  echo "=== $f.yml ==="
  dora start $f.yml --name $f --detach
  sleep 10
  ps -o pid,ppid,comm -C sensor,aggregator,logger
  dora stop $f
  sleep 2
done
echo "=== dataflow-missing.yml（應該失敗）==="
dora start dataflow-missing.yml --name missing --detach
echo "結束碼 $?"
