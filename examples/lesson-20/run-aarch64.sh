#!/usr/bin/env bash
# 用 qemu-user 模擬跑 aarch64 版：官方 aarch64 dora CLI＋aarch64 節點（需要先 binfmt 註冊 qemu-aarch64，見第 15 篇 setup-qemu.sh）
set -uo pipefail
cd "$(dirname "$0")"
export QEMU_LD_PREFIX=${QEMU_LD_PREFIX:?請設定 QEMU_LD_PREFIX：模擬板子 rootfs 的 aarch64 glibc sysroot（官方 aarch64 CLI 要 glibc ≥ 2.34）}
DORA=${DORA:?請設定 DORA：官方 aarch64 dora CLI 的路徑}
mkdir -p run
TZ=Asia/Taipei date '+開始 %F %T %Z' | tee run/dataflow-aarch64.log
$DORA run dataflow-aarch64.yml --stop-after ${1:-90}s >> run/dataflow-aarch64.log 2>&1
echo "dora run 結束碼 $?" | tee -a run/dataflow-aarch64.log
