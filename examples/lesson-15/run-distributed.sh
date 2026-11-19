#!/usr/bin/env bash
# 一台 x86_64 模擬兩台機器：x86 coordinator + x86 daemon(workstation) + aarch64 daemon(zynq, 跑在 qemu-aarch64 底下)
# ZYNQ_DORA：板子上用的 dora 執行檔；ZYNQ_SYSROOT：模擬板子 rootfs 的 glibc
set -u
IP=${IP:-172.30.0.2}
X86_DORA=${X86_DORA:-dora}
ZYNQ_DORA=${ZYNQ_DORA:?請設定 ZYNQ_DORA：板子上用的 aarch64 dora 執行檔路徑}
ZYNQ_SYSROOT=${ZYNQ_SYSROOT:?請設定 ZYNQ_SYSROOT：模擬板子 rootfs 的 aarch64 glibc sysroot 路徑}
TAG=${TAG:-glibc241}
mkdir -p run/dist-$TAG && L=run/dist-$TAG

$X86_DORA coordinator --interface $IP --store memory > $L/coordinator.log 2>&1 &
sleep 1
$X86_DORA daemon --coordinator-addr $IP --machine-id workstation > $L/daemon-workstation.log 2>&1 &
# binfmt_misc 已註冊 qemu-aarch64，QEMU_LD_PREFIX 讓 daemon 和它生出來的節點都用這個 sysroot
QEMU_LD_PREFIX=$ZYNQ_SYSROOT $ZYNQ_DORA daemon --coordinator-addr $IP --machine-id zynq \
    --labels arch=aarch64 --local-listen-port 53292 > $L/daemon-zynq.log 2>&1 &
sleep 4
export DORA_COORDINATOR_ADDR=$IP
$X86_DORA cluster status | tee $L/cluster-status.txt
$X86_DORA start dataflow-zynq.yml --name zynq-demo --detach > $L/start.log 2>&1; echo "start 結束碼 $?" | tee -a $L/start.log
sleep 10
ps -o pid,ppid,args -C sysinfo,sink,dora,qemu-aarch64 | cut -c1-160 | tee $L/ps.txt
$X86_DORA stop zynq-demo > $L/stop.log 2>&1
sleep 3
# dataflow 結束後用名稱查 dora logs 會找不到；直接讀 daemon 寫的 jsonl（真實板子上 sysinfo 的檔在板子那邊）
UUID=$(grep -o 'started: [0-9a-f-]*' $L/start.log | cut -d' ' -f2)
for f in _work/*/out/$UUID/log_*.jsonl; do
  python3 -c 'import json,sys; [print(d["ts"], d["node"]+":", d["msg"]) for d in map(json.loads, open(sys.argv[1]))]' "$f"
done > $L/node-logs.txt
kill %1 %2 %3 2>/dev/null; wait 2>/dev/null
echo done
