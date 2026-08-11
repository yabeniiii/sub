#!/bin/sh
set -e

# curl -sS -X POST http://localhost:8080/ \
#     -H "Content-Type: application/x-www-form-urlencoded" \
#     --data "WINID=0&DATA=&CMD=ClearBuffer" \
#     -o /dev/null 

env RUSTFLAGS="-Awarnings" probe-rs run --chip STM32F401CC "$1"
