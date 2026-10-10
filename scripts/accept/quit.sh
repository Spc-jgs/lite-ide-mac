#!/usr/bin/env bash
# #52 验收：Dock 右键「退出」那条路（`quit` Apple Event，注销 / 关机同一条）退出之前，刚打的字要存下来。
# 走测试通道（scripts/lib/bridge.sh）：不发全局按键、不截图；退出用的 Apple Event 只发给测试身份的 bundle id。
# 先 scripts/build-test-app.sh 打一份测试 .app。
#
# 10-08 修之前实测：同样的步骤 3 次全丢（issue #52）；⌘Q 那条 3 次全留。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
# 有没有这串字。不写成 $(case …) —— macOS 自带的 bash 3.2 会把模式后面那个 ) 当成命令替换的结尾
contains() { case "$1" in *"$2"*) echo 1 ;; *) echo 0 ;; esac; }
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
trap 'lite_teardown; rm -rf "${A:-}"' EXIT

A=$(mktemp -d /tmp/acQ.XXXX); A=$(cd "${A}" && pwd -P)

# 打一个字串，200ms 内用 $2 那条路退出，等进程真没了；再起来看这串字在不在（盘上或者恢复出来的草稿里）
round() {
  local mark="$1" how="$2" i
  lite_clean_data
  local before; before=$(grep -c "系统要退出" "${LITE_LOG}" 2>/dev/null)
  echo "alpha" > "${A}/a.txt"
  lite_launch "${A}" >/dev/null || { ok 0 "${mark}：起不来"; return; }
  lite_wait main "return __lite.project.root === '${A}'" 15
  lite_eval main "await __lite.tabflow.openPath('${A}/a.txt', { preview: false }); return true" >/dev/null
  lite_wait main "return (__lite.text() ?? '').startsWith('alpha')" 8
  lite_eval main "__lite.caret(1, 1); __lite.type('${mark} '); return true" >/dev/null
  sleep 0.2
  case "${how}" in
    apple) osascript -e "tell application id \"${LITE_ID}\" to quit" >/dev/null 2>&1 & ;;
    menu)  lite menu quit >/dev/null 2>&1 ;;
  esac
  for ((i = 0; i < 60; i++)); do [ -z "$(lite_pid)" ] && break; sleep 0.25; done
  wait 2>/dev/null
  [ -z "$(lite_pid)" ] || { ok 0 "${mark}：15 秒内没退出"; pkill -f "lite-ide-mwtest.app/Contents/MacOS"; return; }
  # stderr 日志几次启动是接着写的，看这一轮多了几条
  local hook; hook=$(( $(grep -c "系统要退出" "${LITE_LOG}" 2>/dev/null) - ${before:-0} ))
  # 再起来：项目文件「离开就存」可能已经写盘了；没写盘的话草稿在会话快照里，开回来是脏的
  lite_launch "${A}" >/dev/null || { ok 0 "${mark}：重开起不来"; return; }
  lite_wait main "return __lite.project.root === '${A}'" 15
  lite_eval main "await __lite.tabflow.openPath('${A}/a.txt', { preview: false }); return true" >/dev/null
  lite_wait main "return (__lite.text() ?? '').includes('${mark}')" 6
  local got; got=$(lite_eval main "return __lite.text()" 2>/dev/null | head -1)
  ok "$(contains "${got}$(cat "${A}/a.txt")" "${mark}")" \
    "${how} 退出之后 ${mark} 还在（编辑器：${got:-空}；盘上：$(head -1 "${A}/a.txt")；钩子接到 ${hook} 次）"
  lite_quit >/dev/null
}

echo "== Dock 右键「退出」那条路（quit Apple Event），3 轮"
round Q1 apple; round Q2 apple; round Q3 apple
echo "== ⌘Q（菜单里的退出）照旧"
round M1 menu

echo "== 钩子装上了，没撞上 tao 已有的实现"
ok "$(grep -q "退出钩子装在" "${LITE_LOG}" && echo 1)" "stderr 里有「退出钩子装在 …」：$(grep -o '退出钩子装在 [^ ]*' "${LITE_LOG}" | head -1)"
LOGF="${HOME}/Library/Logs/${LITE_ID}/app.log"
ok "$(grep -q "applicationShouldTerminate" "${LOGF}" 2>/dev/null && echo 0 || echo 1)" "app.log 里没有「已经有 applicationShouldTerminate」的警告"

printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
