#!/usr/bin/env bash
# 用法：./gen.sh 輸出.yml PROFILE MODE SIZE MS N SLOW Q LABEL
sed -e "s/@PROFILE@/$2/g" -e "s/@MODE@/$3/g" -e "s/@SIZE@/$4/g" -e "s/@MS@/$5/g" \
    -e "s/@N@/$6/g" -e "s/@SLOW@/$7/g" -e "s/@Q@/$8/g" -e "s/@LABEL@/$9/g" dataflow.tpl.yml > "$1"
