#!/usr/bin/env bash
# 黑箱測試：拿「編譯好的那個執行檔」跑固定輸入，和預期輸出逐行比對。
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --bin lesson-13 -q
out=$(mktemp)
DORA_TEST_WITH_INPUTS=fixtures/inputs.json \
DORA_TEST_WRITE_OUTPUTS_TO="$out" \
DORA_TEST_NO_OUTPUT_TIME_OFFSET=1 \
  ./target/release/lesson-13 </dev/null 2>/dev/null
if diff -u fixtures/expected.jsonl "$out"; then
  echo "PASS：輸出與 fixtures/expected.jsonl 完全相同"
else
  echo "FAIL：輸出不同（上面是 diff）"; exit 1
fi
