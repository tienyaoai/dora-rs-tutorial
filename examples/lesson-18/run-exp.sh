#!/usr/bin/env bash
# 用法：./run-exp.sh 名稱 PROFILE MODE SIZE MS N SLOW Q [前綴指令，如 taskset -c 0]
name=$1; shift
mkdir -p run
./gen.sh run/$name.yml $1 $2 $3 $4 $5 $6 $7 $name
shift 7
$@ dora run run/$name.yml > run/$name.log 2>&1
echo "== $name 結束碼 $?"; grep -oE "(worker MODE|sink \[).*" run/$name.log
