#!/usr/bin/env bash
# 交叉編譯 → 縮小 → 送到板子。BOARD 例：petalinux@192.168.1.20
# 沒有板子時設 BOARD=local，改用本機 cp 模擬（本篇實測就是這樣）。
set -euo pipefail
TARGET=${TARGET:-aarch64-unknown-linux-gnu}   # ZYNQ-7000 改 armv7-unknown-linux-gnueabihf
GLIBC=${GLIBC:-2.28}                          # 不超過板子 rootfs 的 glibc
PROFILE=${PROFILE:-zynq}
BOARD=${BOARD:-local}
DEST=/opt/dora-nodes/lesson-15

cargo zigbuild --profile "$PROFILE" --target "$TARGET.$GLIBC" --bin sysinfo --bin sink
OUT=target/$TARGET/$PROFILE
file "$OUT/sysinfo"
ls -l "$OUT/sysinfo" "$OUT/sink"

if [ "$BOARD" = local ]; then
  mkdir -p "$DEST" && install -m755 "$OUT/sysinfo" "$OUT/sink" "$DEST/"
else
  ssh "$BOARD" "mkdir -p $DEST"
  scp "$OUT/sysinfo" "$OUT/sink" "$BOARD:$DEST/"
  ssh "$BOARD" "file $DEST/sysinfo 2>/dev/null; ldd --version | head -1"
fi
echo "部署完成：$DEST"
