# 测试通道的 bash 那半（source 它）。协议见 src-tauri/src/testbridge.rs，客户端是同目录的 bridge.py。
#
# 和原来站在应用外面模仿人（System Events 敲键、AX 树、截图）比：**不发全局按键、不读 AX 树、不截图、启动后不抢焦点** ——
# 被测的是临时身份的 .app（scripts/build-test-app.sh 打的），数据目录和你的 lite-ide 是两套。
#
#   lite_launch [路径…]     后台起应用，等到 main 窗口的前端能回话；测试 .app 比源码旧就拒绝（返回 2）
#   lite <子命令> …         bridge.py 的子命令（windows / menu / close / eval）
#   lite_eval <窗口> <JS>   同 lite eval
#   lite_wait <窗口> <JS> [秒]   反复跑那段 JS，直到它 return true（默认等 10 秒）
#   lite_quit               点它自己的「退出」（走 winctl::quit，先存现场）
#   lite_teardown [keep]    杀掉、删数据目录（不删 .app，下次不用重打）；带 keep 留着临时目录（stderr 日志在里面）
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

# ── 前台切换记录（#54）──
# 采样「这一刻谁在最前」只能知道「到某一段开头时已经被抢了」；frontlog 订阅系统的激活通知，记下每一次切换的时间和应用，
# 再和 lite_mark 打的段落标记对时间（frontlog-report.py）。**只读**：不发按键、不读 AX、不截图
LITE_MARKS="${LITE_WORK}/marks.tsv"
lite_now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }
lite_mark() { printf '%s\t%s\n' "$(lite_now_ms)" "$1" >> "${LITE_MARKS}"; }
# 编一次（约 1 秒）、在后台起。起它的脚本退了，它一秒内自己退（看父进程是不是成了 launchd）
lite_frontlog_start() {
  xcrun swiftc -O -o "${LITE_WORK}/frontlog" "${LITE_ROOT}/scripts/lib/frontlog.swift" 2>/dev/null || { echo "  （frontlog 编不过，这一轮没有前台切换记录）" >&2; return 1; }
  "${LITE_WORK}/frontlog" > "${LITE_WORK}/front.tsv" &
  LITE_FRONTLOG_PID=$!
}
lite_frontlog_report() { python3 -I "${LITE_ROOT}/scripts/lib/frontlog-report.py" "${LITE_MARKS}" "${LITE_WORK}/front.tsv" "${LITE_ID}"; }

# 比测试 .app 新的源文件，打印第一个（没有就什么都不打）。
#
# 测试 .app 要手动重打（约 3 分钟），忘了重打，验的就是旧代码 —— 而且照样全绿。这个仓库在「跑的是哪个构建」上栽过
# （AGENTS.md 验证纪律：照着现象查了半天，最后发现早就修好了）。比的是 mtime：git 切分支也会把文件变新，宁可多打一次。
# 只列进构建的东西；`tests/` 底下的不算（测试改了，产物不变）
lite_stale() {
  (cd "${LITE_ROOT}" && find src public index.html vite.config.ts svelte.config.js package.json pnpm-lock.yaml \
    src-tauri/src src-tauri/crates src-tauri/capabilities src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs \
    src-tauri/tauri.conf.json -type f -newer "${LITE_APP}/Contents/MacOS/lite-ide" -not -path '*/tests/*' 2>/dev/null | head -1)
}

lite_launch() {
  [ -d "${LITE_APP}" ] || { echo "没有测试 .app，先跑 scripts/build-test-app.sh" >&2; return 2; }
  local stale
  stale=$(lite_stale)
  if [ -n "${stale}" ] && [ "${LITE_ALLOW_STALE:-}" != 1 ]; then
    echo "测试 .app 比源码旧（${stale} 在它打完之后改过）：先跑 scripts/build-test-app.sh。确实要测旧的那份就设 LITE_ALLOW_STALE=1" >&2
    return 2
  fi
  local prev
  prev=$(lite_front_id)
  # -g：后台起。--env 只给这一个进程，不改你的环境。
  # ZDOTDIR 指到一个空目录：测试应用里开的终端是你的登录 zsh，不隔开的话它读你的 .zshrc、往你的 ~/.zsh_history 里写
  # （2026-10-09 smoke ㉔ 的 `cd "sp dir"` 就这么写进去了十条）。zsh 的配置和历史都跟着 ZDOTDIR 走（/etc/zshrc 里
  # HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history），这一项让它们都落在临时目录里；顺带测试不再受你 rc 的快慢影响
  #
  # LITE_USER_SHELL=1 不隔开：scripts/accept/tasks-real.sh 要验的就是「从 Finder 起的应用，任务的登录 shell 读你的 .zshrc、找得到 mvn / pnpm」。
  # 那份脚本不开终端（任务是 `-ilc`，命令不经行编辑器、不进历史 —— TASKS.md 第 10 节实测），并且前后比对你的历史文件
  local zdot=(--env ZDOTDIR="${LITE_WORK}/zdot")
  [ "${LITE_USER_SHELL:-}" = 1 ] && zdot=()
  mkdir -p "${LITE_WORK}/zdot"
  lite_mark "[启动测试应用]"
  # PATH 设成 Finder 双击时那样（launchd 的默认值）：**`open` 会把调用它的终端的环境整个带给应用**（2026-10-10 整体审核实测：
  # 测试应用的 PATH 里有 homebrew、fnm、pyenv），不设的话验的是「从终端起的应用」—— 搜索会用上 rg（Finder 起的找不到，走内置实现），
  # tasks-real.sh「从 Finder 起也找得到 mvn」那条成了空的。`open --env` 只能覆盖、不能删，LANG 这类终端有、Finder 没有的还是会带过去
  open -g --env PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    --env LITE_IDE_TEST_SOCK="${LITE_TEST_SOCK}" --env LITE_IDE_DEBUG=1 ${zdot[@]+"${zdot[@]}"} \
    --stderr "${LITE_LOG}" -a "${LITE_APP}" "$@"
  lite_wait main "return true" 20 || { echo "20 秒内 main 窗口没回话（日志 ${LITE_LOG}）" >&2; return 1; }
  # **启动时那一下抢焦点关不掉**：tao（Tauri 底下的窗口库）在「启动完成」里无条件 activateIgnoringOtherApps，
  # 盖过 `open -g`；tao 有开关，Tauri 没开放（2026-10-09 读源码确认）。应用自己的 set_focus、新窗口拿焦点在测试构建里
  # 都关了（may_take_focus），剩这一下：起来之后立刻把焦点还给你原来在用的应用。
  # 测试构建还设了 Accessory（不进 Dock、不在 ⌘Tab 里）：你的应用隐藏 / 放弃激活时，系统不会再把前台转交给它（#54）
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
  [ -n "${LITE_FRONTLOG_PID:-}" ] && kill "${LITE_FRONTLOG_PID}" 2>/dev/null
  pkill -f "lite-ide-mwtest.app/Contents/MacOS" 2>/dev/null
  sleep 1
  lite_clean_data
  [ "${1:-}" = keep ] || rm -rf "${LITE_WORK}"
  # 从 LaunchServices 注销：不然 Finder 的「打开方式」里会多一个 lite-ide-mwtest。.app 留在盘上，下次不用重打；
  # 下次 `open -a` 它时系统会临时再登记
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "${LITE_APP}" >/dev/null 2>&1
}
