#!/usr/bin/env bash
# 旧入口保留；旧脚本会操作正式应用，不能继续拿它做验收。
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "${1:-}" = 8 ]; then
  shift
  exec scripts/resources.sh --profile full "$@"
fi
exec scripts/resources.sh "$@"
