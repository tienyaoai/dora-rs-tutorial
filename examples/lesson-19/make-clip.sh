#!/usr/bin/env bash
# 產生測試影片：make-clip 寫 rawvideo＋答案 → ffmpeg 壓成 H.264 的 clip.mp4（有損壓縮，像真的影片檔）
set -euo pipefail
cd "$(dirname "$0")"
N=${1:-300}
target/release/make-clip clip.y8 clip.labels "$N" 320 240
ffmpeg -hide_banner -loglevel error -y -f rawvideo -pix_fmt gray -s 320x240 -r 30 -i clip.y8 \
       -c:v libx264 -crf 23 -pix_fmt yuv420p clip.mp4
rm clip.y8
ls -l clip.mp4 clip.labels
