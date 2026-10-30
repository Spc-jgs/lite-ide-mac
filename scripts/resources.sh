#!/usr/bin/env bash
# 按需执行；测试身份、有限 fixture、外部采样。不是定时任务，详见 docs/RESOURCE-GUARD.md。
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
cd "$(dirname "$0")/.."
if [ "${1:-}" = compare ]; then
  shift
  exec python3 scripts/lib/resource_guard.py compare "$@"
fi
if [ "${1:-}" = --self-test ]; then
  exec python3 tests/resources_test.py
fi
if [ "${1:-}" = --help ]; then
  echo 'scripts/resources.sh [--build] [--profile quick|full] [--workload mixed|json|json-text] [--pace burst|paced] [--minimap on|off] [--output 新目录] [--baseline report.json]'
  echo 'scripts/resources.sh compare before/report.json after/report.json'
  echo 'scripts/resources.sh --self-test'
  exit 0
fi
mkdir -p src-tauri/target/resource-tools.noindex
xcrun clang -O2 -Wall -Wextra -Werror scripts/lib/resource-probe.c -o src-tauri/target/resource-tools.noindex/resource-probe
if [ "${1:-}" = --build ]; then
  shift
  python3 scripts/lib/resource_guard.py build
fi
source scripts/lib/bridge.sh
export LITE_WORK
# prepare 在清数据、启动前拒绝已有测试进程和旧构建；不接管另一轮验收。
python3 scripts/lib/resource_guard.py prepare "$@"
cleanup_resources() {
  # 清理前重新核对 PID + 进程起点；失败则保留现场，不接管其他验收。
  if python3 scripts/lib/resource_guard.py cleanup; then
    lite_clean_data
    rm -rf "${LITE_WORK}"
  else
    echo "资源验收现场保留在 ${LITE_WORK}" >&2
  fi
}
trap cleanup_resources EXIT
lite_clean_data
# 默认标签上限 10；本场景固定 12 个标签，避免测到自动淘汰后的另一种工作量。
mkdir -p "${LITE_DATA}"
printf '%s\n' '{"editor.tabLimit":25}' > "${LITE_DATA}/settings.json"
lite_launch "${LITE_WORK}/fixture"
python3 scripts/lib/resource_guard.py measure
