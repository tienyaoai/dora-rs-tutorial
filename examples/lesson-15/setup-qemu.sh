#!/usr/bin/env bash
# 沒有 ZYNQ 板子時：讓 x86_64 主機能直接 exec aarch64／armv7 的 ELF（qemu-user + binfmt_misc，需要 root）
# 用完可 echo -1 | sudo tee /proc/sys/fs/binfmt_misc/qemu-{aarch64,arm} 取消註冊
set -eu
[ -e /proc/sys/fs/binfmt_misc/register ] || sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc
[ -e /proc/sys/fs/binfmt_misc/qemu-aarch64 ] || sudo sh -c 'echo ":qemu-aarch64:M::\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\xb7\x00:\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff:/usr/bin/qemu-aarch64:F" > /proc/sys/fs/binfmt_misc/register'
[ -e /proc/sys/fs/binfmt_misc/qemu-arm ] || sudo sh -c 'echo ":qemu-arm:M::\x7fELF\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x28\x00:\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff:/usr/bin/qemu-arm:F" > /proc/sys/fs/binfmt_misc/register'
ls /proc/sys/fs/binfmt_misc/
