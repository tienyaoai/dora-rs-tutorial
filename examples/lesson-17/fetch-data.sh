#!/usr/bin/env bash
# 下載 MNIST（ossci-datasets 鏡像）與 ONNX Model Zoo 的 mnist-12.onnx，並核對 sha256（本篇實測時的值）。
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p data models
for f in train-images-idx3-ubyte train-labels-idx1-ubyte t10k-images-idx3-ubyte t10k-labels-idx1-ubyte; do
  [ -f data/$f ] || { curl -sSfL -o data/$f.gz https://ossci-datasets.s3.amazonaws.com/mnist/$f.gz && gunzip -f data/$f.gz; }
done
[ -f models/mnist-12.onnx ] || curl -sSfL -o models/mnist-12.onnx \
  https://github.com/onnx/models/raw/main/validated/vision/classification/mnist/model/mnist-12.onnx
sha256sum -c <<'SUM'
0fa7898d509279e482958e8ce81c8e77db3f2f8254e26661ceb7762c4d494ce7  data/t10k-images-idx3-ubyte
ff7bcfd416de33731a308c3f266cc351222c34898ecbeaf847f06e48f7ec33f2  data/t10k-labels-idx1-ubyte
ba891046e6505d7aadcbbe25680a0738ad16aec93bde7f9b65e87a2fc25776db  data/train-images-idx3-ubyte
65a50cbbf4e906d70832878ad85ccda5333a97f0f4c3dd2ef09a8a9eef7101c5  data/train-labels-idx1-ubyte
5c688690f8bacf667d4c2074af5ad0646ca328d7ab03eccf944a65b320171bdd  models/mnist-12.onnx
SUM
