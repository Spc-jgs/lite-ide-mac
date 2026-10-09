# 测试通道的 bash 那半（source 它）。协议见 src-tauri/src/testbridge.rs，客户端是同目录的 bridge.py。
#
# 和 smoke.sh 的老路比：**不发全局按键、不读 AX 树、不截图、启动不抢焦点**（`open -g`）——
# 被测的是临时身份的 .app（scripts/build-test-app.sh 打的），数据目录和你的 lite-ide 是两套。
#
#   lite_launch [路径…]     后台起应用，等到 main 窗口的前端能回话
#   lite <子命令> …         bridge.py 的子命令（windows / menu / close / eval）
#   lite_eval <窗口> <JS>   同 lite eval
#   lite_wait <窗口> <JS> [秒]   反复跑那段 JS，直到它 return true（默认等 10 秒）
#   lite_quit               点它自己的「退出」（走 winctl::quit，先存现场）
#   lite_teardown           杀掉、删数据目录（不删 .app，下次不用重打）
#   lite_front              此刻最前面的应用叫什么（证明测试没抢你的焦点）

LITE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LITE_APP="${LITE_ROOT}/src-tauri/target/release/bundle/macos/lite-ide-mwtest.app"
LITE_ID="com.liteide.mwtest"
LITE_DATA="${HOME}/Library/Application Support/${LITE_ID}"
LITE_WORK="$(mktemp -d /tmp/lite-bridge.XXXXXX)"
export LITE_TEST_SOCK="${LITE_WORK}/bridge.sock"   # Unix socket 路径有 104 字节上限，放 /tmp 下
LITE_LOG="${LITE_WORK}/stderr.log"

lite() { python3 "${LITE_ROOT}/scripts/lib/bridge.py" "$@"; }
lite_eval() { lite eval "$1" "$2"; }
lite_front() { osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null; }
lite_pid() { pgrep -f "lite-ide-mwtest.app/Contents/MacOS" | head -1; }

lite_wait() {
  local w="$1" js="$2" secs="${3:-10}" i
  for ((i = 0; i < secs * 4; i++)); do
    [ "$(lite eval "${w}" "${js}" 2>/dev/null)" = "true" ] && return 0
    sleep 0.25
  done
  return 1
}

lite_clean_data() {
  local d
  for d in "WebKit/${LITE_ID}" "Application Support/${LITE_ID}" "Logs/${LITE_ID}" "Caches/${LITE_ID}" \
    "Saved Application State/${LITE_ID}.savedState" "Preferences/${LITE_ID}.plist"; do
    rm -rf "${HOME}/Library/${d}"
  done
}

lite_front_id() { osascript -e 'tell application "System Events" to get bundle identifier of first process whose frontmost is true' 2>/dev/null; }

lite_launch() {
  [ -d "${LITE_APP}" ] || { echo "没有测试 .app，先跑 scripts/build-test-app.sh" >&2; return 2; }
  local prev
  prev=$(lite_front_id)
  # -g：后台起。--env 只给这一个进程，不改你的环境
  open -g --env LITE_IDE_TEST_SOCK="${LITE_TEST_SOCK}" --env LITE_IDE_DEBUG=1 --stderr "${LITE_LOG}" -a "${LITE_APP}" "$@"
  lite_wait main "return true" 20 || { echo "20 秒内 main 窗口没回话（日志 ${LITE_LOG}）" >&2; return 1; }
  # **启动时那一下抢焦点关不掉**：tao（Tauri 底下的窗口库）在「启动完成」里无条件 activateIgnoringOtherApps，
  # 盖过 `open -g`；tao 有开关，Tauri 没开放（2026-10-09 读源码确认）。应用自己的 set_focus、新窗口拿焦点在测试构建里
  # 都关了（may_take_focus），剩这一下：起来之后立刻把焦点还给你原来在用的应用
  if [ -n "${prev}" ] && [ "$(lite_front_id)" != "${prev}" ]; then
    osascript -e "tell application id \"${prev}\" to activate" >/dev/null 2>&1
  fi
}

lite_quit() {
  lite menu quit >/dev/null 2>&1
  local i
  for ((i = 0; i < 40; i++)); do [ -z "$(lite_pid)" ] && return 0; sleep 0.25; done
  return 1
}

lite_teardown() {
  pkill -f "lite-ide-mwtest.app/Contents/MacOS" 2>/dev/null
  sleep 1
  lite_clean_data
  rm -rf "${LITE_WORK}"
  # 从 LaunchServices 注销：不然 Finder 的「打开方式」里会多一个 lite-ide-mwtest。.app 留在盘上，下次不用重打；
  # 下次 `open -a` 它时系统会临时再登记
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "${LITE_APP}" >/dev/null 2>&1
}
