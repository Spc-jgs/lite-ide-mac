#!/usr/bin/env bash
#
# 量内存：开关标签会不会累积（issue #10 第 3 条）。
#
# # 判据不是「关掉后回落到基线」
#
# 语言包缓存（`langs-load.ts` 的那个 Map）只 set 不 evict —— 那是**设计**，
# 12 种语言加载过就留着，所以关完不回到基线是正常的，拿它当泄漏证据会误判。
#
# 真泄漏的判据是**关完的值逐轮往上走**：同样 12 个文件开关 N 轮，
# 对「关完」序列做最小二乘，看斜率和 R²。
#
# # 三个会让人得出错误结论的坑，都写进实现里了
#
#   1. **用 Physical footprint，不用 RSS。** RSS 把共享的系统框架在四个进程里
#      各算一遍，加起来虚高。
#   2. **单次 vmmap 读数抖动十几 MB，必须取中位数。** 第一版一个点读一次，
#      四轮的关完值是 160/125/147/184 —— 看着像在涨，其实是噪声。
#      现在每个点读三次取中间那个。
#   3. **不能只比「第一轮 vs 基线」。** 第一轮涨的大头是 12 个 Lezer parser
#      的一次性加载，和泄漏完全是两回事。要看的是第 N 轮相对第 N-1 轮。
#
# # WebKit 辅助进程怎么认领
#
# 它们的 ppid 都是 1（launchd 起的 XPC 服务），没法按父进程找。
# 用差分：起应用前记下所有 WebKit 进程，之后新增且 pid 大于主进程的就是我们的。
#
# # 这个方法量不到什么
#
# 残差标准差约 15 MB，也就是说**每轮小于十几 MB 的泄漏它看不见**。
# 对 issue #10 关心的量级（155→238 那 83 MB 的漂移）足够了；
# 要知道「堆里具体是谁」得上 Safari 的 Web Inspector，那需要
# 带 devtools feature 的构建，这个脚本给不出来。
#
# 用法：先 `pnpm app:bundle`，然后 `./scripts/mem.sh [轮数，默认 8]`
# 需要终端有「辅助功能」权限（同 smoke.sh）。
set -uo pipefail
export LANG=${LANG:-en_US.UTF-8}

ROUNDS=${1:-8}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${ROOT}/src-tauri/target/release/bundle/macos/lite-ide.app/Contents/MacOS/lite-ide"
[ -x "${APP}" ] || { echo "找不到 .app —— 先跑 pnpm app:bundle"; exit 2; }

FIX=$(mktemp -d /tmp/lite-ide-mem.XXXXXX)
WORK=$(mktemp -d /tmp/lite-ide-mem-work.XXXXXX)
LOG="${WORK}/app.log"
AXLIB="${WORK}/ax.applescript"
cleanup(){ pkill -f "MacOS/lite-ide" 2>/dev/null; rm -rf "${FIX}" "${WORK}"; }
trap cleanup EXIT

# 只用得上「按名字点一行」和「按名字找元素」，从 smoke.sh 抄这两条
cat > "${AXLIB}" <<'APPLESCRIPT'
on findRow(el, nm)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is "AXRow" then
        try
          if (value of attribute "AXTitle" of el) contains nm then return el
        end try
        try
          repeat with c in (UI elements of el)
            try
              if (value of c) contains nm then return el
            end try
            try
              if (value of attribute "AXTitle" of c) contains nm then return el
            end try
          end repeat
        end try
      end if
    end try
    try
      repeat with c in (UI elements of el)
        set r to my findRow(c, nm)
        if r is not missing value then return r
      end repeat
    end try
  end tell
  return missing value
end findRow

on run argv
  set nm to item 1 of argv
  tell application "System Events"
    tell process "lite-ide"
      set frontmost to true
      set w to window 1
    end tell
  end tell
  delay 0.2
  set el to findRow(w, nm)
  if el is missing value then return "NOTFOUND"
  tell application "System Events"
    set selected of el to true
    try
      click el
    end try
  end tell
  return "OK"
end run
APPLESCRIPT

open_row(){ osascript "${AXLIB}" "$1" 2>&1 | tail -1; }
menu(){ osascript -e "tell application \"System Events\" to tell process \"lite-ide\"
  set frontmost to true
  delay 0.2
  click menu item \"$2\" of menu 1 of menu bar item \"$1\" of menu bar 1
end tell" >/dev/null 2>&1; }

# ── fixture：12 种语言，各 400 行 ─────────────────────────────────
cd "${FIX}"; git init -q -b main .
gen(){ local f=$1 line=$2 i; : > "$f"; for i in $(seq 1 400); do printf '%s\n' "${line//NUM/$i}" >> "$f"; done; }
gen app.ts    'export function fnNUM(x: number): string { return `v${x + NUM}`; }'
gen app.js    'export const fNUM = (a, b) => ({ id: NUM, sum: a + b });'
gen app.py    'def fn_NUM(x: int) -> str:  return f"v{x + NUM}"'
gen app.rs    'pub fn fn_NUM(x: u32) -> String { format!("v{}", x + NUM) }'
gen app.go    'func FnNUM(x int) string { return fmt.Sprintf("v%d", x+NUM) }'
gen app.java  'public String fnNUM(int x) { return "v" + (x + NUM); }'
gen app.rb    'def fn_NUM(x) = "v#{x + NUM}"'
gen app.php   'function fnNUM($x) { return "v" . ($x + NUM); }'
gen app.cpp   'std::string fnNUM(int x) { return "v" + std::to_string(x + NUM); }'
gen app.cs    'public string FnNUM(int x) => $"v{x + NUM}";'
gen app.swift 'func fnNUM(_ x: Int) -> String { return "v\(x + NUM)" }'
gen app.kt    'fun fnNUM(x: Int): String = "v${x + NUM}"'
FILES="app.ts app.js app.py app.rs app.go app.java app.rb app.php app.cpp app.cs app.swift app.kt"
git add -A && git commit -qm init
echo "fixture：12 种语言各 400 行，共 $(du -sh "${FIX}" | cut -f1)；跑 ${ROUNDS} 轮"

# ── 起应用，差分认领 WebKit 辅助进程 ──────────────────────────────
pkill -f "MacOS/lite-ide" 2>/dev/null; sleep 1
BEFORE=$(pgrep -f "com.apple.WebKit" | sort -u | tr '\n' ' ')
LITE_IDE_DEBUG=1 LITE_IDE_POS=0,40 "${APP}" "${FIX}" > "${LOG}" 2>&1 &
disown
for i in $(seq 1 40); do grep -q "App 已挂载" "${LOG}" && break; sleep 0.5; done
sleep 4
MAIN=$(pgrep -f "MacOS/lite-ide" | head -1)
[ -n "${MAIN}" ] || { echo "应用没起来，看 ${LOG}"; exit 1; }
OURS=""
for p in $(pgrep -f "com.apple.WebKit" | sort -u); do
  case " ${BEFORE} " in *" ${p} "*) ;; *) [ "${p}" -gt "${MAIN}" ] && OURS="${OURS} ${p}" ;; esac
done
[ -n "${OURS}" ] || { echo "没认领到 WebKit 辅助进程（是不是有别的应用同时在起？）"; exit 2; }
echo "主进程 ${MAIN}，WebKit 辅助进程${OURS}"

mem1(){
  local t=0 v p
  for p in ${MAIN} ${OURS}; do
    v=$(vmmap --summary "${p}" 2>/dev/null | awk -F: '/Physical footprint:/{gsub(/[ \t]/,"",$2); print $2; exit}')
    [ -z "${v}" ] && continue
    t=$(awk -v a="${t}" -v b="${v}" 'BEGIN{
      u=substr(b,length(b)); n=substr(b,1,length(b)-1)+0;
      if(u=="G") n*=1024; else if(u=="K") n/=1024;
      printf "%.1f", a+n }')
  done
  echo "${t}"
}
mem(){ local a b c; a=$(mem1); sleep 2; b=$(mem1); sleep 2; c=$(mem1); printf '%s\n%s\n%s\n' "$a" "$b" "$c" | sort -n | sed -n 2p; }

# 会话恢复会把上次的标签开回来 —— 基线必须干净
menu "文件" "关闭所有标签"; sleep 3
T0=$(mem); printf '\n基线（无标签） %s MB\n' "${T0}"

PS=""; TS=""
for r in $(seq 1 "${ROUNDS}"); do
  for f in ${FILES}; do open_row "${f}" >/dev/null; sleep 1.8; done
  P=$(mem)
  menu "文件" "关闭所有标签"
  sleep 25          # 给 GC 时间；WebKit 没有可调的 gc()，只能等
  T=$(mem)
  printf '  第 %s 轮  峰值 %8s   关完 %8s\n' "${r}" "${P}" "${T}"
  PS="${PS} ${P}"; TS="${TS} ${T}"
done
sleep 60
printf '  再静置 60s        %8s MB\n' "$(mem)"

echo "${TS}" | tr ' ' '\n' | grep -v '^$' | awk -v r="${ROUNDS}" '
  { n++; x[n]=n; y[n]=$1; sx+=n; sy+=$1 }
  END{
    if(n<3){ print "\n  轮数太少，看不出趋势"; exit }
    mx=sx/n; my=sy/n
    for(i=1;i<=n;i++){ num+=(x[i]-mx)*(y[i]-my); den+=(x[i]-mx)^2; ss+=(y[i]-my)^2 }
    b=num/den
    for(i=1;i<=n;i++){ e=y[i]-(my+b*(x[i]-mx)); rss+=e*e }
    printf "\n  关完序列的趋势：%+.1f MB / 轮（每轮 12 个标签，%+.2f MB/标签）\n", b, b/12
    printf "  R² = %.2f，残差标准差 %.1f MB\n", (ss>0?1-rss/ss:0), sqrt(rss/(n-2))
    printf "\n  斜率显著为正且 R² 高 → 线性泄漏；\n"
    printf "  斜率接近 0 或 R² 低   → 没有可测的泄漏，关完不回基线的那部分是\n"
    printf "                          语言包缓存（设计如此）加分配器不归还。\n"
  }'
