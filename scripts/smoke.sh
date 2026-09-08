#!/usr/bin/env bash
#
# 用**真的 .app** 跑一遍验收清单（issue #11）。
#
# # 为什么要有它
#
# 有一整类 bug，单元测试和浏览器里的 `pnpm dev` 都抓不到 —— 它们只在
# 「真 .app + 真 IPC + 真文件系统」这条完整链路上才现形。2026-09-07 就靠手工
# 点这一遍逮到一个：一个 26MB 全中文的 UTF-8 日志在界面上整份是乱码，
# 而行数和级别统计全对，每一层的单测也全绿 —— 错的是编码探测的那个标签，
# 只有真打开一个大中文日志才看得见。
#
# 那次是手点的。这个脚本把那一遍固化下来。
#
# # 它靠什么点得动界面
#
# WKWebView 把整棵 DOM 暴露成了 macOS 的辅助功能树，所以 AppleScript 能按名字
# 找到按钮并 AXPress、能给编辑器设焦点、能读回文本。（issue #11 原来写着
# 「AppleScript 够不着 webview」，那条是错的。）
#
# 七个踩过的坑，都写进实现里了，别改回去：
#
#   1. `click at {x, y}` 不行（报 -25208），必须按元素 AXPress。
#   2. `set value of text area` 不生效 —— 它不触发 input 事件，Svelte 收不到。
#   3. **中文输入法会把敲进去的 ASCII 吃掉**：`keystroke "verify: commit"`
#      出来的是 `verify啊commit through贴合realapp`。所以一律走剪贴板 + ⌘V，
#      粘贴不过输入法（⌘V 这种带修饰键的组合本身不受影响）。
#   4. 别记 AX 路径 —— 界面一变它就断（暂存之后「提交」会变成「提交 (1)」）。
#      每次按角色 + 名字重新递归查。
#   5. **`$VAR` 后面紧跟中文，变量名会被吃掉一截**：`echo "分支 $BR，远程 ..."`
#      里的 `$BR，` 被 bash 当成了变量 `BR<那几个字节>`，`set -u` 下直接
#      `unbound variable` 把脚本打断。这个脚本正文全是中文，撞上的概率很高 ——
#      **变量一律写 `${VAR}`**。写这段自检的提示文案时又踩了第二次。
#   6. **刚出现的浮层/菜单，第一次查找必落空。** WKWebView 的 AX 子树是
#      惰性构建的：⇧⌘F 的浮层弹出来之后，紧接着的第一次 `findIt` 返回
#      NOTFOUND，而下一次同样的查找就成功。所以**动手之前先 `wait_has`
#      确认它出来了** —— 轮询正好把落空的那一次消化掉，顺便验了前提。
#      直接上手的话报出来的是「输入框找不到（浮层没出来？）」，
#      而浮层其实好端端开着，排查方向完全错。
#   7. **只有 `keystroke` 需要应用在前台，别的一概不需要。** click、读属性、
#      set focused、连点原生菜单栏，在应用处于后台时全部照常生效
#      （实测：前台停在 Finder，点「Git 改动」面板照常出来、点文件树的行
#      文件照常打开、点「文件 → 关闭所有标签」标签确实关掉）。
#      原来每一次 AX 调用都无条件 `set frontmost`，跑一遍几百次，
#      人根本没法同时用电脑。现在只有 paste 和 keys 抢。
#      （试过用 Swift 的 `CGEventPostToPid` 把键直接投给进程、完全不抢焦点，
#      对这个 WKWebView **无效** —— ⌘P 发过去没有任何反应。）
#
# # 断言尽量落在盘上
#
# 能用 `git log` / `stat` 验的就不去读界面：界面读回来的是我们自己画的，
# 而盘上的东西是真的。只有「日志正文有没有乱码」这一条必须读界面 ——
# 那正是它当初漏掉的地方。
#
# 用法：
#   ./scripts/smoke.sh              # 用已经打好的 .app
#   ./scripts/smoke.sh --keep       # 跑完不删临时仓库，方便自己再点两下
#
# 前提：先 `pnpm app:bundle`；终端需要「辅助功能」权限（第一次会弹窗）。

set -uo pipefail

KEEP=0
for a in "$@"; do
  case "$a" in
    --keep) KEEP=1 ;;
    *) echo "未知参数: ${a}（支持 --keep）"; exit 2 ;;
  esac
done

# **必须给 pbcopy 一个 UTF-8 的 locale。** LANG 没设的时候（从 GUI 或某些
# 自动化环境起的 shell 就是这样），`printf '中文' | pbcopy` 会把剪贴板置成
# **空的** —— 于是脚本里那句「⌘A 全选、⌘V 粘贴」变成了「全选、粘个空」，
# 编辑器被清空，⌘S 老老实实把 0 字节写进了文件。
# 这个坑写 smoke.sh 时踩到了：`real/config.txt` 变成 0 字节，而断言只会说
# 「内容不对」，看不出是剪贴板的锅。
export LANG=${LANG:-en_US.UTF-8}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/src-tauri/target/release/bundle/macos/lite-ide.app/Contents/MacOS/lite-ide"
FIX="$(mktemp -d /tmp/lite-ide-smoke.XXXXXX)"
# **这三个都不能放进 $FIX** —— 那是个 git 仓库，而应用会一直往日志里写。
# 放进去的话 `git status` 永远不干净，「提交之后工作区该是空的」这条断言
# 就永远不会成立，而失败信息会指向一个根本不存在的死锁
WORK="$(mktemp -d /tmp/lite-ide-smoke-work.XXXXXX)"
LOG="$WORK/app.log"
CLIP="$WORK/clipboard.bak"
AXLIB="$WORK/ax.applescript"
PASS=0; FAIL=0

# **每个段落开头把焦点还回去。**
#
# `keys` 抢了焦点是不会自己还的，而第一次敲键盘发生在 ② —— 不还的话
# 从那之后 lite-ide 就一直占着前台，实测占用率 94%，等于没改。
#
# 还焦点只能放在**段落边界**上：段落内部常常是「粘贴 → ⌘S」这种连续动作，
# 中间还掉的话，后面那下 ⌘S 就打进用户正在用的应用里去了。
# `say` 恰好只在每段开头调用一次，是现成的边界。
say()  {
  [ -n "${PREV_APP:-}" ] && osascript -e "tell application \"${PREV_APP}\" to activate" >/dev/null 2>&1
  printf '\n\033[1m== %s\033[0m\n' "$1"
}
ok()   { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  \033[31m✗\033[0m %s\n' "$1"; }
check(){ if [ "$1" = "$2" ]; then ok "$3"; else bad "$3（期望 [$2]，实得 [$1]）"; fi; }
# 只观察、不计成败。给「这条断言本身还没验稳」的项用 ——
# 把没把握的东西算进成败，等于教人忽略红色
note() { printf '  \033[33m•\033[0m %s\n' "$1"; }

cleanup() {
  pkill -f "MacOS/lite-ide" 2>/dev/null
  # 剪贴板是用户的东西，借来用完要还
  [ -f "$CLIP" ] && pbcopy < "$CLIP"
  # 前台应用同理。敲键盘那十来次会把焦点抢过来，跑完要放回原处 ——
  # 不放的话，人回到电脑前发现自己打的字进了一个已经被 kill 的窗口
  [ -n "${PREV_APP:-}" ] && osascript -e "tell application \"${PREV_APP}\" to activate" >/dev/null 2>&1
  if [ "$KEEP" = 1 ]; then
    echo; echo "临时仓库留着了：${FIX}（日志在 ${LOG}）"
  else
    rm -rf "$FIX" "$WORK"   # $REMOTE / $OTHER 都在 $WORK 底下
  fi
}
trap cleanup EXIT

# ─────────────────── AppleScript 那一层 ───────────────────

cat > "$AXLIB" <<'APPLESCRIPT'
-- 递归找元素。**不能用 `entire contents`** —— 它返回的引用读不出 name
-- （实测 0 个有名字的元素），而一层层 `UI elements of` 出来的引用读得出。
on findIt(el, wantRole, wantName)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is wantRole then
        if wantName is "" then return el
        try
          if (value of attribute "AXTitle" of el) is wantName then return el
        end try
        try
          if (value of attribute "AXDescription" of el) is wantName then return el
        end try
      end if
    end try
    try
      repeat with c in (UI elements of el)
        set r to my findIt(c, wantRole, wantName)
        if r is not missing value then return r
      end repeat
    end try
  end tell
  return missing value
end findIt

-- 名字/取值里**包含**某段文字的元素。名字会变（「提交」→「提交 (1)」）、
-- 日志行会被拆成好几段 static text，所以精确匹配不够用
on findSub(el, wantRole, sub)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is wantRole then
        try
          if (value of attribute "AXTitle" of el) contains sub then return el
        end try
        try
          if (value of attribute "AXDescription" of el) contains sub then return el
        end try
        try
          if (value of attribute "AXValue" of el) contains sub then return el
        end try
        -- 没有 label 的 <input> 只有 placeholder，而它落在这个属性上，
        -- 上面三个都取不到 —— ⌘P 那个搜索框就是这种
        try
          if (value of attribute "AXPlaceholderValue" of el) contains sub then return el
        end try
      end if
    end try
    try
      repeat with c in (UI elements of el)
        set r to my findSub(c, wantRole, sub)
        if r is not missing value then return r
      end repeat
    end try
  end tell
  return missing value
end findSub

-- 文件树那一行：先找到名字对得上的 static text，再往上走到它所在的 AXRow
on findRow(el, nm)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is "AXRow" then
        if my findSub(el, "AXStaticText", nm) is not missing value then return el
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
  set act to item 1 of argv
  set wantRole to item 2 of argv
  set wantName to item 3 of argv
  tell application "System Events"
    tell process "lite-ide"
      -- **只有要敲键盘的动作才抢焦点。**
      --
      -- `keystroke` 发给的是**当前前台应用**，不抢的话 paste 那两下
      -- ⌘A/⌘V 会打进用户正在用的那个应用里去 —— 那不只是没做成，
      -- 是在别人的文档上乱按。
      --
      -- 而 click / 读属性 / set focused **在后台完全生效**（实测：
      -- 前台停在 Finder，点「Git 改动」Git 面板照常出来、点文件树的行
      -- 文件照常打开）。原来这里无条件抢，于是跑一遍 smoke.sh
      -- 几百次 AX 调用全在抢焦点，人根本没法同时用电脑。
      -- `rowfocus` 也要抢：它设的键盘焦点是给紧接着那下 ⇧F10 用的，
      -- 而 `keys` 里那句 `set frontmost` 在激活应用时会让 webview
      -- **把焦点复位**（同 paste 分支的注释）——
      -- 于是行上的焦点没了，⇧F10 打空，菜单不出来。
      -- 先在这儿激活，`keys` 那次就成了 no-op，焦点保得住。
      if act is "paste" or act is "rowfocus" then set frontmost to true
      set w to window 1
    end tell
  end tell
  delay 0.2
  if act is "row" or act is "rowfocus" then
    set el to findRow(w, wantName)
  else if act is "click~" or act is "has" then
    set el to findSub(w, wantRole, wantName)
  else
    set el to findIt(w, wantRole, wantName)
  end if
  if el is missing value then return "NOTFOUND"
  tell application "System Events"
    if act is "click" or act is "click~" then
      click el
    else if act is "focus" then
      set focused of el to true
    else if act is "paste" then
      -- **聚焦和敲键必须在同一次 osascript 里**。分成两次的话，第二次那句
      -- `set frontmost to true` 会让 webview 把焦点复位，于是 ⌘A/⌘V 打空 ——
      -- 现象是提交框里还是占位符、编辑器被 ⌘A 清空后存成了 0 字节。
      set focused of el to true
      delay 0.4
      keystroke "a" using {command down}
      delay 0.2
      keystroke "v" using {command down}
      delay 0.5
    else if act is "row" then
      set selected of el to true
      try
        click el
      end try
    else if act is "rowfocus" then
      -- **只聚焦，不点击。** ⇧F10 的 handler 绑在每一行上（onRowKey），
      -- 要那一行拿到**键盘焦点**才收得到；而 `row` 的 click 会把文件打开，
      -- 焦点跟着跑去编辑器，⇧F10 就再也到不了树上了。
      try
        set selected of el to true
      end try
      set focused of el to true
    end if
  end tell
  return "OK"
end run
APPLESCRIPT

ax() { osascript "$AXLIB" "$1" "$2" "${3:-}" 2>&1 | tail -1; }

# 界面文本快照。读用 `entire contents` 那条路 —— 它的**文本输出**里带名字，
# 正好和上面递归查找的用途互补
ax_text() {
  osascript -e 'tell application "System Events" to tell process "lite-ide" to get entire contents of window 1' 2>/dev/null | tr ',' '\n'
}

# **这个必须抢焦点** —— `keystroke` 只发给前台应用，没有别的办法：
# 试过用 Swift 的 `CGEventPostToPid` 直接投递给进程（不经前台），
# 对这个 WKWebView **无效**（⌘P 发过去一点反应都没有）。
# 所以脚本运行期间会有十来次短暂占用键盘，其余步骤都不抢（见 AXLIB 里的注释）。
keys() { osascript -e "tell application \"System Events\" to tell process \"lite-ide\"
  set frontmost to true
  delay 0.2
  $1
end tell" >/dev/null 2>&1; }

# 点原生菜单栏。**能走菜单栏就别敲快捷键** —— 菜单栏是真的 AppKit 菜单，
# 点得到就说明那条命令确实被触发了；而快捷键是发给 webview 的，
# 焦点在别处（比如浮层里的输入框）时会被吃掉，表现成「命令没反应」，
# 排查方向却指向命令本身。
#
# **不抢焦点**：AppKit 的菜单项在应用处于后台时照样点得动，而且真的执行
# （实测：前台停在 Finder，点「文件 → 关闭所有标签」，标签确实关掉了）。
menu() { osascript -e "tell application \"System Events\" to tell process \"lite-ide\"
  click menu item \"$2\" of menu 1 of menu bar item \"$1\" of menu bar 1
end tell" >/dev/null 2>&1; }

# 选中全部再粘贴（绕开输入法，见文件头第 3 条）。
# `$1` 是目标元素的角色，`$2` 是要粘的文本
paste_into() {
  printf '%s' "$2" | pbcopy
  local r; r=$(ax paste "$1" "")
  sleep 0.5
  [ "$r" = "OK" ]
}

# 轮询等一个 shell 条件成立，超时返回 1
wait_for() {
  local secs=$1; shift
  local i=0
  while [ $i -lt $((secs * 4)) ]; do
    eval "$@" >/dev/null 2>&1 && return 0
    sleep 0.25; i=$((i+1))
  done
  return 1
}

# 等界面上出现某段文字。**每一步动手之前都用它确认前提** ——
# 不确认的话，一次「消息没粘进去」会被报成「提交挂住了」，
# 而那两件事的排查方向完全不同（写 smoke.sh 时就被这么误导过一轮）
wait_has() {
  local role=$1 sub=$2 secs=${3:-8} i=0
  while [ $i -lt $((secs * 2)) ]; do
    [ "$(ax has "$role" "$sub")" = "OK" ] && return 0
    sleep 0.5; i=$((i+1))
  done
  return 1
}

# 文件树里点开一个文件。**按名字找那一行**，不记行号 ——
# 行号随展开状态变，而展开状态随上一步做了什么变
open_from_tree() {
  if [ "$(ax row "" "$1")" != "OK" ]; then
    bad "文件树里找不到 $1"; return 1
  fi
  sleep 2
}

# ─────────────────── 造一个验收用的仓库 ───────────────────

[ -x "$APP" ] || { echo "找不到 .app —— 先跑 pnpm app:bundle"; exit 2; }
pbpaste > "$CLIP" 2>/dev/null
PREV_APP=$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null)
echo "跑之前的前台应用是「${PREV_APP:-未知}」，跑完会还回去"
echo "（只有敲快捷键的那十来步会占用键盘，其余步骤不抢焦点，可以继续用电脑）"

say "造 fixture：$FIX"
cd "$FIX"
git init -q -b main .
git config user.email smoke@local; git config user.name smoke
printf '#!/bin/sh\necho hello\n' > run.sh; chmod 755 run.sh
mkdir -p real; printf '原始内容\n第二行\n' > real/config.txt
ln -s real/config.txt link.txt
printf 'v1\n' > note.txt
# ⇧⌘F 要搜的那根针。**两行是有讲究的**：搜第一行的词，命中行会显示在
# 结果列表里 —— 断言它等于「浮层上有这段字」，浮层没关就永远绿。
# 所以断言认第二行：那句只有真把文件打开才看得见。
mkdir -p deep/nested
printf 'ZQXJ_SMOKE_NEEDLE\n这一行要把文件打开才看得见\n' > deep/nested/needle.txt
# ⑨ 要扔进废纸篓的那个。**名字必须是独一无二的**，两个理由：
# 废纸篓里撞名的话 Finder 会按自己的规则改名（`note 2.txt`），
# 按固定路径就 stat 不到；而且跑第二遍时上一遍的残留会让断言假绿。
TRASH_NAME="lite-ide-smoke-trash-$$-$(date +%s).txt"
printf '这个文件是给「移到废纸篓」那一步用的\n' > "${TRASH_NAME}"
# 大日志：**必须带中文，而且要大过 detect_encoding 的 256KB 样本** ——
# 那个乱码 bug 正是「样本按字节截，边界切在多字节字符中间」造出来的
awk 'BEGIN{for(i=0;i<400000;i++) printf "2026-09-07 12:00:00 INFO  服务处理完成，第 %d 条记录\n", i}' > big.log
git add -A && git commit -qm "初始提交"
git branch feature/x
# 话多的钩子：3000 行稳稳超过管道那几十 KB 缓冲，用来复现那个死锁
printf '#!/bin/sh\nfor i in $(seq 1 3000); do echo "smoke: 噪声 $i"; done\nexit 0\n' > .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
printf 'v2 改过了\n' > note.txt
echo "  大日志 $(du -h big.log | cut -f1)，钩子 3000 行"

say "起 .app"
LITE_IDE_DEBUG=1 LITE_IDE_ONTOP=1 LITE_IDE_POS=0,40 "$APP" "$FIX" > "$LOG" 2>&1 &
# 从作业表里摘掉：不摘的话 cleanup 里的 pkill 会让 bash 在最后印一行
# `Terminated: 15`，那行看着像脚本自己出错了，实际是收尾正常杀进程
disown
if wait_for 20 'grep -q "App 已挂载" '"$LOG"; then
  ok "挂载成功"
else
  bad "20 秒内没挂起来，后面全跳过"; exit 1
fi
sleep 2

# **拿得到窗口才往下跑。**
#
# 没有「辅助功能」权限时 System Events 不报权限错，只是把窗口数报成 0，
# 于是后面每一条断言都红 —— 12 条全红看起来像应用整个坏掉了，
# 而真正的原因和被测的东西一点关系都没有。2026-09-07 被这个骗过一轮。
#
# 特别注意：`name of every process` **不需要**授权也能用，所以
# 「osascript 能列出进程」不能拿来当权限已给的证据 —— 我就是这么判错的。
WINS=$(osascript -e 'tell application "System Events" to tell process "lite-ide" to get count of windows' 2>/dev/null)
if [ "${WINS:-0}" = "0" ]; then
  printf '\n\033[31m拿不到 lite-ide 的窗口（count of windows = 0），后面全部跳过。\033[0m\n'
  echo "两种可能，按概率排："
  echo "  1. 跑这个脚本的终端没有「辅助功能」权限。"
  echo "     系统设置 → 隐私与安全性 → 辅助功能 → 勾上你的终端，然后**重开终端**。"
  echo "     （注意是跑脚本的那个终端，不是 lite-ide 自己。)"
  echo "  2. 应用真的没建出窗口 —— 看 ${LOG}。"
  exit 2
fi
ok "AX 拿得到窗口（$WINS 个）"

# **把遗留的横幅关掉再开始。**
#
# 会话快照是跨次留存的（localStorage 在 WKWebView 的容器里），而上一轮的
# fixture 目录已经被删了 —— 这一轮启动时会话恢复去开那些标签就会失败，
# 弹出一条「知道了」横幅。它和这轮要验的东西毫无关系，但会让
# 「界面上有没有失败提示」这类断言全部误报（写这个脚本时被它骗过一轮：
# 提交明明成功了，却被报成「钩子把它挂住了」）。
for _ in 1 2 3; do
  [ "$(ax click AXButton "知道了")" = "OK" ] || break
  sleep 0.4
done

# ─────────────────── 1. 提交（带话多的钩子）───────────────────

say "① 提交：3000 行的 pre-commit 钩子不能把它挂住"
MSG="smoke: commit through the real app"
if [ "$(ax click AXButton "Git 改动")" != "OK" ]; then
  bad "点不到「Git 改动」"
elif ! wait_has AXButton "全部暂存" 10; then
  bad "Git 面板没渲染出来"
else
  paste_into AXTextArea "$MSG"
  if ! wait_has AXStaticText "$MSG" 5; then
    # 再试一次：应用刚起来时会先恢复上次的标签，那期间焦点可能被抢
    paste_into AXTextArea "$MSG"
  fi
  if ! wait_has AXStaticText "$MSG" 5; then
    bad "提交信息没粘进输入框（不是钩子的问题，是这一步没做成）"
  else
    [ "$(ax click AXButton "全部暂存")" = "OK" ] || bad "点不到「全部暂存」"
    wait_for 10 'git -C "'"$FIX"'" diff --cached --quiet; [ $? -ne 0 ]' || bad "没暂存上"
    # 盘上暂存了不等于界面已经刷过来。**暂存之后按钮名字会变成「提交 (N)」**，
    # 不等就会点到上一帧的旧按钮
    if ! wait_has AXButton "提交 (" 10; then
      bad "界面没刷出「提交 (N)」"
    else
      [ "$(ax "click~" AXButton "提交 (")" = "OK" ] || bad "点不到提交按钮"
      # **断言认「多了一条提交、标题对得上」，不认「工作区变干净」** ——
      # 后者会被任何无关的工作区噪声搅黄（应用自己的日志、临时文件、
      # 上一步留下的改动），而那时的失败信息会指向一个不存在的死锁
      if wait_for 40 '[ "$(git -C "'"$FIX"'" log -1 --format=%s)" = "'"$MSG"'" ]'; then
        ok "提交落地（$(git -C "$FIX" log -1 --format='%h %s')）"
      else
        bad "40 秒内没提交成功 —— 多半是钩子把它挂住了（stderr 没被并发排空）"
        echo "     暂存区：$(git -C "$FIX" diff --cached --name-only | tr '\n' ' ')"
        echo "     git log：$(git -C "$FIX" log --oneline | head -2 | tr '\n' ' ')"
        echo "     界面上的提示：$(ax has AXStaticText "失败")"
      fi
    fi
  fi
fi

# ─────────────────── 2. 保存不动文件的身份 ───────────────────

say "② ⌘P 唤得出来（它是懒加载的，首屏之后才预拉）"
# **先切回文件树**：Git 面板上那个提交信息框也是 AXTextArea，
# 留着它，「找第一个 text area」会抓到它而不是编辑器 —— 于是内容粘进了
# 提交框、⌘S 什么也没存，而断言看起来只是「内容没改」
[ "$(ax click AXButton "文件树")" = "OK" ] || bad "点不到「文件树」"
sleep 1.5
keys 'keystroke "p" using {command down}'
sleep 1.5
#
# ⚠ **这一条目前只观察，不计成败。**
#
# ⌘P 本身是好的 —— 手工在真 .app 里用它开过文件、也列得出候选。红的是
# 「怎么认出浮层出来了」这件事：
#
#   - 别认「随处搜索」：那是空态卡片上的字，浮层不出来它也在，
#     那条断言会**永远绿**（这种断言比没有更糟）。
#   - 认输入框的占位符要读 `AXPlaceholderValue` —— 没有 label 的 `<input>`
#     的 placeholder 只落在这个属性上，`AXTitle` / `AXDescription` / `AXValue`
#     都是空的。上面 findSub 刚补上这条，**但还没验过**。
#   - 兜底认浮层脚注里的「换范围」，那几个字只有这个浮层有。
#
# 下次谁跑到这儿：确认一下这两条能不能认出来，能就把 `note` 改回 `ok/bad`。
if [ "$(ax has AXTextField "输入文件名")" = "OK" ] || [ "$(ax has AXStaticText "换范围")" = "OK" ]; then
  ok "⌘P 唤出来了"
else
  note "⌘P 浮层没认出来 —— 待确认是它没出来，还是这条断言认错了属性（见上面注释）"
fi
keys 'key code 53'   # Esc 收掉
sleep 0.8

say "③ 保存一个 0755 的脚本：权限不能丢"
# 走文件树而不是 ⌘P —— 这一步要验的是保存，不是打开方式；
# 混在一起的话，⌘P 抽风会被报成「保存坏了」
open_from_tree "run.sh"
if ! wait_has AXStaticText "echo" 6; then
  bad "run.sh 没打开"
elif paste_into AXTextArea '#!/bin/sh
echo hello from smoke'; then
  keys 'keystroke "s" using {command down}'
  wait_for 10 'grep -q "hello from smoke" "'"$FIX"'/run.sh"'
  check "$(stat -f %Lp run.sh)" "755" "权限还是 755"
  check "$(./run.sh 2>&1)" "hello from smoke" "内容改了，而且还能执行"
else
  bad "粘不进编辑器"
fi

say "④ 保存一条软链：不能把链接换成普通文件"
# 走文件树而不是 ⌘P —— 软链文件现在在 ⌘P 里搜不到，见 issue #19
open_from_tree "link.txt"
if ! wait_has AXStaticText "原始内容" 6; then
  bad "link.txt 没打开"
elif paste_into AXTextArea '通过 app 改过的
第二行还在'; then
  keys 'keystroke "s" using {command down}'
  sleep 2
  [ -L link.txt ] && ok "link.txt 仍然是软链" || bad "软链被换成普通文件了"
  check "$(head -1 real/config.txt)" "通过 app 改过的" "改动写进了真身"
  [ -z "$(ls -a | grep 'lite-ide-tmp')" ] && ok "没留下临时文件" || bad "留了临时文件"
else
  bad "打不开 link.txt"
fi

# ─────────────────── 4. 切分支 ───────────────────

say "⑤ 切分支"
[ "$(ax click AXButton "main")" = "OK" ] || bad "点不开分支浮层"
sleep 1.5
# 分支按钮的名字后面跟着那条分支的最新提交标题，会变 —— 按前缀点
if [ "$(ax "click~" AXButton "feature/x")" = "OK" ]; then
  wait_for 20 '[ "$(git -C "'"$FIX"'" rev-parse --abbrev-ref HEAD)" = "feature/x" ]' \
    && ok "切到了 feature/x" || bad "没切过去"
else
  bad "分支浮层里找不到 feature/x"
fi

# ─────────────────── 5. 大日志：正文不能是乱码 ───────────────────

say "⑥ 打开 26MB 的中文日志：正文不能是乱码"
[ "$(ax click AXButton "文件树")" = "OK" ] || true
sleep 1
open_from_tree "big.log"
RSS_BEFORE=$(ps -o rss= -p "$(pgrep -f 'MacOS/lite-ide' | head -1)" | tr -d ' ')
# **这里不能用 ax_text**：日志一开，`entire contents` 就是几千行，
# osascript 的输出会被截断，于是断言变成「碰运气」。按角色递归找确定得多
if [ "$(ax has AXStaticText "服务处理完成")" = "OK" ]; then
  ok "中文正常（界面上读回了「服务处理完成」）"
else
  bad "正文乱码 —— detect_encoding 的样本又被切在半个字符上了（见 rules/rust.md）"
fi
[ "$(ax has AXStaticText "400,000")" = "OK" ] && ok "行数统计对（400,000）" || bad "行数统计不对"

say "⑦ 关掉大日志：mmap 要跟着放掉"
[ "$(ax "click~" AXButton "关闭 big.log")" = "OK" ] || bad "点不到关闭按钮"
sleep 3
RSS_AFTER=$(ps -o rss= -p "$(pgrep -f 'MacOS/lite-ide' | head -1)" | tr -d ' ')
echo "  RSS $RSS_BEFORE → $RSS_AFTER KB"
[ "$RSS_AFTER" -lt "$RSS_BEFORE" ] && ok "内存降下来了" || bad "关掉之后内存没降"

# ─────────────────── 6. 全局搜索 / 废纸篓 / 远程 ───────────────────
#
# 下面这三段补的是 issue #11 清单里剩下的那几条命令。它们和上面几条一起在
# 2026-09-06 那轮被挪到了 tokio 的阻塞池，但一直只有「提交 / 切分支 / 保存」
# 在真 .app 里被点过 —— `grep_project`、`trash_entry`、`git_fetch`、`git_push`
# 一次都没有。补齐之前说「#11 验完了」是说满了。

say "⑧ ⇧⌘F 全局搜索（grep_project）"
# 先把焦点收回文件树：上一步刚关掉日志标签，焦点可能还在日志面板上，
# 而 ⇧⌘F 是发给 webview 的
[ "$(ax click AXButton "文件树")" = "OK" ] || true
sleep 1
keys 'keystroke "f" using {command down, shift down}'
sleep 1.5
# **先确认浮层真的出来了，再动手。** 这一步同时干了两件事：确认前提，
# 以及把「新子树第一次查找必落空」那一次消化掉（见文件头第 6 条）。
# 认「换范围」是因为那几个字只有这个浮层的脚注有。
if ! wait_has AXStaticText "换范围" 8; then
  bad "⇧⌘F 的浮层没出来"
elif ! paste_into AXTextField "ZQXJ_SMOKE_NEEDLE"; then
  bad "浮层出来了，但输入框粘不进去"
else
  # 搜索要扫整个 fixture，里面躺着那个 26MB 的大日志 —— 给足时间。
  # **结果行是 AXButton，不是 AXStaticText** —— 整条（命中行 + 路径 + 行号）
  # 合成一个按钮名：`ZQXJ_SMOKE_NEEDLE deep/nested/needle.txt:1`。
  # 一开始按 AXStaticText 找，25 秒等不到，报出来像是「搜索没返回」，
  # 其实结果早就在屏幕上了。
  if wait_has AXButton "needle.txt" 25; then
    ok "搜到了 deep/nested/needle.txt"
    keys 'key code 36'   # ↵ 打开第一条命中
    # 断言认第二行 —— 第一行是命中行，浮层上本来就印着它
    if wait_has AXStaticText "这一行要把文件打开才看得见" 10; then
      ok "点得开，打开的确实是那个文件"
    else
      bad "搜到了但没打开（命令回来了，前端跳转那一步断了？）"
    fi
  else
    bad "25 秒内没搜到那根针"
    keys 'key code 53'
  fi
fi
sleep 0.8

say "⑨ 移到废纸篓（trash_entry）：不能是真删除"
# 键盘开上下文菜单。`click at {x, y}` 在 webview 里被系统拒（-25208，见文件头），
# 所以右键点不出来 —— ⇧F10 是文件树自己认的第二个入口（FileTree.svelte:430）
[ "$(ax click AXButton "文件树")" = "OK" ] || true
sleep 1
if [ "$(ax rowfocus "" "${TRASH_NAME}")" != "OK" ]; then
  bad "文件树里找不到 ${TRASH_NAME}"
else
  sleep 0.6
  keys 'key code 109 using {shift down}'   # ⇧F10
  sleep 1.2
  # 先等菜单出来再点 —— 刚出现的子树第一次查找必落空（文件头第 6 条）。
  # **菜单项是 AXMenuItem，不是 AXButton** —— ContextMenu 的 ARIA role
  # 被 WKWebView 映射成了 `menu item ... of menu "note.txt 的操作"`。
  # 按 AXButton 找是找不到的，而报出来的是「⇧F10 没开出菜单」，
  # 于是会往「快捷键没生效」的方向查 —— 菜单其实好端端开着。
  if ! wait_has AXMenuItem "移到废纸篓" 6; then
    bad "⇧F10 没开出上下文菜单（或者菜单里没有这一项）"
  # **用 `click~` 不用 `click`。** `click` 走 findIt，认的是 AXTitle/AXDescription
  # **精确相等**；菜单项的名字落在别的属性上，于是 `has`（contains）找得到、
  # `click` 找不到 —— 报出来是「菜单出来了但点不到」，看着像菜单项被禁用了。
  elif [ "$(ax "click~" AXMenuItem "移到废纸篓")" != "OK" ]; then
    bad "菜单出来了，但点不到「移到废纸篓」"
  else
    # 确认框是另一个新出现的子树，同样先等 —— 它上面的按钮是真 <button>，
    # 所以这里回到 AXButton
    if ! wait_has AXButton "移到废纸篓" 6; then
      bad "确认框没出来"
    fi
    [ "$(ax "click~" AXButton "移到废纸篓")" = "OK" ] || bad "确认框上点不到「移到废纸篓」"
    if wait_for 15 '[ ! -e "'"$FIX"'/'"${TRASH_NAME}"'" ]'; then
      ok "${TRASH_NAME} 从工作区没了"
      # **进废纸篓才算对，不是真删除。**
      #
      # **按具体路径查，不要列目录。** 终端没有「完全磁盘访问」权限时
      # `ls ~/.Trash` / `find ~/.Trash` 会 Operation not permitted 返回空，
      # 而 `stat ~/.Trash/具体文件名` 读得到 —— 这条 rules/rust.md 里早写着，
      # 第一版这里写的是 `find`，于是把一次正常的废纸篓操作报成了「真删除」。
      #
      # 找到了也不动它：那是用户的废纸篓，脚本只读不写（结尾提示一句）。
      if [ -e "${HOME}/.Trash/${TRASH_NAME}" ]; then
        ok "在废纸篓里找得到（Finder 里可以「放回原处」）"
        TRASHED=1
      else
        bad "工作区没了，但 ~/.Trash/${TRASH_NAME} 不在 —— 这就成真删除了"
      fi
    else
      bad "15 秒内文件还在"
    fi
  fi
fi

say "⑩ 推送 / 拉取（git_push / git_fetch）"
# **remote 是这一步现加的，不写进 fixture。** 一开始就有上游的话，
# 分支按钮的名字会跟着变（`main` → 带上 ↑N 之类），而 ⑤ 是按精确名字点它的 ——
# 会把上面那条搞红，且失败信息完全指不到这里。
REMOTE="$WORK/origin.git"
OTHER="$WORK/other"
git init -q --bare "$REMOTE"
git remote add origin "$REMOTE"
BR=$(git rev-parse --abbrev-ref HEAD)
echo "  当前分支 ${BR}，远程 ${REMOTE}"

menu "Git" "推送…"
sleep 1.5
# 没有上游时按钮是「推送并跟踪」，有上游时是「推送」—— 这里必然是前者
if [ "$(ax click AXButton "推送并跟踪")" != "OK" ]; then
  bad "推送确认条没出来（或按钮不叫这个名字）"
else
  if wait_for 30 'git -C "'"$REMOTE"'" rev-parse --verify -q "'"$BR"'"'; then
    check "$(git -C "$REMOTE" rev-parse "$BR")" "$(git rev-parse HEAD)" "推上去的 sha 和本地一致"
  else
    bad "30 秒内没推上去"
  fi
fi

# 造一个「别人推了新东西」的远程，再从界面上拉。
#
# **`-b ${BR}` 不能省。** bare 仓库的 HEAD 建出来就指向默认的 `main`，
# 而我们只往它推了 `feature/x` —— 不带 `-b` 的话 clone 会 warning
# 「remote HEAD refers to nonexistent ref」、检出一个空工作区，
# 后面的 commit 和 push 全部失败，而报出来的是「造不出远程的新提交」，
# 看着像脚本坏了却指不到这一行。
git clone -q -b "${BR}" "$REMOTE" "$OTHER" 2>/dev/null
git -C "$OTHER" config user.email smoke@local
git -C "$OTHER" config user.name smoke
printf '从另一个克隆推上来的\n' > "$OTHER/from-remote.txt"
git -C "$OTHER" add -A
git -C "$OTHER" commit -qm "远程的新提交"
if git -C "$OTHER" push -q origin "HEAD:$BR" 2>/dev/null; then
  UP=$(git -C "$OTHER" rev-parse HEAD)
  menu "Git" "拉取"
  # 拉取 = fetch + 本地合并两步（不是 git pull）。这里必然是快进：
  # 新提交只加了一个文件，碰不到 ③④ 改过的那两个
  if wait_for 40 '[ "$(git -C "'"$FIX"'" rev-parse HEAD)" = "'"$UP"'" ]'; then
    ok "拉取把本地推进到了远程那一条（$(echo "$UP" | cut -c1-7)）"
    [ -f "$FIX/from-remote.txt" ] && ok "新文件落到了工作区" || bad "HEAD 动了但文件没落盘"
  else
    bad "40 秒内没拉下来（HEAD 还停在 $(git -C "$FIX" rev-parse --short HEAD)）"
  fi
else
  bad "造不出远程的新提交 —— 这一步是脚本自己的问题，不是应用的"
fi

# ─────────────────── 收尾 ───────────────────

say "⑪ 界面自己有没有报错"
ERRS=$(grep -icE "\[diag/web\].*(error|fatal)|CSP 挡下" "$LOG")
check "$ERRS" "0" "诊断通道里没有前端报错 / CSP 违规"

# 把开出来的标签关掉，别把 fixture 的路径留在会话快照里 ——
# 下次启动会话恢复会去开一个已经删掉的目录
for f in run.sh link.txt; do ax "click~" AXButton "关闭 $f" >/dev/null; sleep 0.5; done

[ "${TRASHED:-0}" = 1 ] && echo "  （⑨ 往废纸篓里放了 ${TRASH_NAME}，脚本不动它 —— 自己清或者放回原处）"

printf '\n\033[1m通过 %d 条，失败 %d 条\033[0m\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
