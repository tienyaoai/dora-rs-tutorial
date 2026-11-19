#!/usr/bin/env bash
# 一鍵實測：背景啟動 fake-hw（模擬硬體）→ dora run 12 秒。用法：./run-demo.sh [dataflow.yml] → 收掉 fake-hw。
set -uo pipefail
YML=${1:-dataflow.yml}; TAG=$(basename "$YML" .yml)
cd "$(dirname "$0")"
DORA=${DORA:-dora}
mkdir -p run
rm -rf /tmp/lesson16
cargo build --release --bin fake-hw -q
./target/release/fake-hw --seconds 16 --unplug-at 4 --lose-every 40 > run/$TAG-fake-hw.log 2>&1 &
HW=$!
sleep 0.5
$DORA build "$YML" > run/$TAG-build.log 2>&1
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/$TAG.log
$DORA run "$YML" --stop-after 12s >> run/$TAG.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/$TAG.log
wait $HW
echo "fake-hw 結束碼 $?" >> run/$TAG-fake-hw.log
