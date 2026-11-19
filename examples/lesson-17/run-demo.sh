#!/usr/bin/env bash
# 一鍵實測：dora build → dora run N 秒。用法：./run-demo.sh [dataflow.yml] [秒數]
set -uo pipefail
YML=${1:-dataflow.yml}; SECS=${2:-15}; TAG=$(basename "$YML" .yml)
cd "$(dirname "$0")"
DORA=${DORA:-dora}
mkdir -p run
$DORA build "$YML" > run/$TAG-build.log 2>&1 || { echo "dora build 失敗，見 run/$TAG-build.log"; exit 1; }
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/$TAG.log
$DORA run "$YML" --stop-after ${SECS}s >> run/$TAG.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/$TAG.log
