#!/usr/bin/env bash
for p in "" "--release" "--profile fast"; do
  s=$(date +%s); cargo build $p 2>&1 | tail -2; echo "[cargo build $p] $(( $(date +%s)-s )) 秒"
done
