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
# # 它怎么点得动界面：测试通道（2026-10-09 起）
#
# 被测的是 `scripts/build-test-app.sh` 打的**临时身份 .app**（com.liteide.mwtest），
# 指令走它里面的测试通道（`src-tauri/src/testbridge.rs`，客户端 `scripts/lib/bridge.sh`）：
# 在页面里按名字点按钮、读界面上的字，菜单项由 Rust 侧直接触发。
#
# 原来是站在应用外面模仿人 —— System Events 敲键、AX 树按名字找元素、剪贴板粘贴，
# 用的是你**真实的** lite-ide.app 和真实数据（跑之前备份、跑完还原）。三样都出过事：
#
#   - 敲键发给「当前最前面的应用」。2026-10-08 一次验收的 ⌘= 全落进了用户正在用的应用；
#     后来加了「不在前台就整轮停下」，安全了，但你一碰电脑它就停。
#   - AX 点击偶尔落空、刚弹出的浮层第一次必找不到（#22 那一串间歇红），
#     为此长出了 click_then / paste_into 两层重试，重试次数还要单独报。
#   - 备份还原真实数据：cleanup 跑两遍会把你的 WebKit 数据删掉（2026-10-09 修过），
#     「移到废纸篓」那一步往你的废纸篓里留文件。
#
# 现在：**不发全局按键、不读 AX、不碰剪贴板、不碰你的数据**，跑的时候可以照常用电脑。
# 启动那一下会闪一次焦点（tao 无条件激活，见 bridge.sh 的 lite_launch），之后一次都不抢 ——
# 最后一条断言就是验这个的。
#
# # 它验不到的：系统怎么把输入交给应用
#
# 页面里的点击和按键是 DOM 事件，不经过 macOS。所以下面这一层**这里验不到**，归验证方案第三轮（真实输入）：
#   - ⌘S / ⌘W 这类菜单键位，AppKit 有没有先吃掉、交给了谁（#53）—— 这里走 `menu` 指令，直达菜单事件的处理函数
#   - 中文输入法、剪贴板（原来专门绕过输入法走 ⌘V，见 git 历史里这个文件的旧版）
#   - 真实右键时 WebKit 把焦点给了谁（#50）
#
# 业务层面（存盘、git、搜索、日志、窗口路由）一条没少，编号和原来一一对应。
#
# # 断言尽量落在盘上
#
# 能用 `git log` / `stat` 验的就不去读界面：界面读回来的是我们自己画的，
# 而盘上的东西是真的。只有「日志正文有没有乱码」这一条必须读界面 ——
# 那正是它当初漏掉的地方。
#
# 用法：
#   scripts/build-test-app.sh      # 改了代码先打测试 .app（约 3 分钟）
#   scripts/smoke.sh               # 跑
#   scripts/smoke.sh --keep        # 跑完不删临时仓库和日志
#
# **用 bash 跑，别 source 进 zsh**：bridge.sh 靠 BASH_SOURCE 找自己在哪。
# 不需要「辅助功能」权限（不读 AX 了）。

set -uo pipefail

KEEP=0
for a in "$@"; do
  case "$a" in
    --keep) KEEP=1 ;;
    *) echo "未知参数: ${a}（支持 --keep）"; exit 2 ;;
  esac
done

# **变量一律写 `${VAR}`**：`$VAR` 后面紧跟中文时，bash 会把中文的字节当成变量名的一部分，
# `set -u` 下直接 unbound variable 把脚本打断。这个脚本正文全是中文，撞上过两次
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/bridge.sh"
[ -d "${LITE_APP}" ] || { echo "找不到测试 .app —— 先跑 scripts/build-test-app.sh"; exit 2; }

# **规范路径**：/tmp 是 /private/tmp 的软链，应用打开项目时会规范化（open.rs 的 prepare）。
# 不规范化的话，下面拿 FIX 去比项目根、拼文件树行的 data-path，全都对不上
FIX=$(cd "$(mktemp -d /tmp/lite-ide-smoke.XXXXXX)" && pwd -P)
WORK=$(cd "${LITE_WORK}" && pwd -P)
LOG="${LITE_LOG}"
APPLOG="${HOME}/Library/Logs/${LITE_ID}/app.log"
PASS=0; FAIL=0

# 焦点：测试应用起来之后**一次都不该跑到最前面**。判「最前面是不是它」，不判「最前面一直是开始时那个」——
# 你在测试跑的时候切换应用是正常的（accept/settings.sh 第一版就是这么误报的）。每段开头采样一次
STOLEN=0
say()  {
  [ "$(lite_front_id)" = "${LITE_ID}" ] && STOLEN=$((STOLEN + 1))
  printf '\n\033[1m== %s\033[0m\n' "$1"
}
ok()   { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  \033[31m✗\033[0m %s\n' "$1"; }
check(){ if [ "$1" = "$2" ]; then ok "$3"; else bad "$3（期望 [$2]，实得 [$1]）"; fi; }

# **cleanup 只跑一次**：INT / TERM / PIPE 一律 exit，由 EXIT 触发唯一的一次。
# bash 收到 TERM / Ctrl-C 时跑完 trap **不退出、接着往下跑**，退出时 EXIT 再跑一遍（2026-10-09 实测）。
# 现在 cleanup 只删临时目录和测试身份的数据，跑两遍也无害，但这个形状留着：下一个往 cleanup 里加东西的人不用再踩一次
CLEANED=0
cleanup() {
  [ "${CLEANED}" = 1 ] && return
  CLEANED=1
  if [ "${KEEP}" = 1 ]; then
    lite_teardown keep
    echo; echo "临时仓库留着了：${FIX}（stderr 在 ${LOG}）"
  else
    lite_teardown
    rm -rf "${FIX}"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 141' PIPE

# ─────────────────── 和页面说话的几样 ───────────────────
#
# 都作用在 ${W} 那个窗口上（窗口的 label：main、w-1…）。⑰ ⑲ 开了别的窗口时临时改它

W=main
# 把一段文字变成 JS 字符串字面量。文件内容带换行、路径带空格和中文，手拼引号迟早拼错
q()        { python3 -c 'import json,sys; print(json.dumps(sys.argv[1], ensure_ascii=False))' "$1"; }
# 跑一段 JS，return true 才算成立
is()       { [ "$(lite eval "${W}" "$1" 2>/dev/null)" = "true" ]; }
has()      { is "return __lite.has($(q "$1"))"; }
wait_has() { lite_wait "${W}" "return __lite.has($(q "$1"))" "${2:-8}"; }
# click <名字> [角色 button|menuitem|treeitem|any] [匹配 exact|prefix|contains]
click()    { is "return __lite.click($(q "$1"), $(q "${2:-button}"), $(q "${3:-exact}"))"; }
exists()   { is "return __lite.exists($(q "$1"), $(q "${2:-any}"), $(q "${3:-contains}"))"; }
wait_exists() { lite_wait "${W}" "return __lite.exists($(q "$1"), $(q "${2:-any}"), $(q "${3:-contains}"))" "${4:-8}"; }
key()      { lite eval "${W}" "__lite.key($(q "$1")); return true" >/dev/null; }
fill()     { is "__lite.fill($(q "$1")${2:+, $(q "$2")}); return true"; }
set_text() { is "__lite.setText($(q "$1")); return true"; }
# 浮层的结果列表里有没有这段字。**别用 exists 在整页找**：标签栏上「关闭 needle.txt」那种按钮也含这个名字，
# 文件开过一次，「搜到了 needle.txt」就永远成立（#42 加 ⑧ 第三段时撞上的，第二段的软链断言也因此一直有落空的可能）
in_results() { is "return !!document.querySelector('.popup .results')?.innerText.includes($(q "$1"))"; }
wait_results() { lite_wait "${W}" "return !!document.querySelector('.popup .results')?.innerText.includes($(q "$1"))" "${2:-8}"; }
# 菜单项：和点原生菜单走同一个处理函数（winctl::menu_event），只发给 ${W}
menu()     { lite menu "$1" "${W}" >/dev/null; }
active()   { lite eval "${W}" "return __lite.tabs.active?.path ?? ''" 2>/dev/null; }

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

# 侧边栏切到文件树。**看一眼再点**：导轨上点「当前那个」是收起（ui.md 第十条），已经在文件树上再点一下树就没了
show_tree() {
  is "return !!document.querySelector('[role=\"tree\"]')?.getClientRects().length" && return 0
  click "文件树"
  lite_wait "${W}" "return !!document.querySelector('[role=\"tree\"]')?.getClientRects().length" 4
}

# 文件树里点开一个文件（相对 ${FIX}）。**按 data-path 找那一行**，不按名字、不记行号 ——
# 名字会重，行号随展开状态变。等到它成了活动标签才返回
open_from_tree() {
  local p="${FIX}/$1"
  show_tree
  if ! is "return __lite.tap($(q "[role=\"treeitem\"][data-path=\"${p}\"]"))"; then
    bad "文件树里找不到 $1"; return 1
  fi
  lite_wait "${W}" "return __lite.tabs.active?.path === $(q "${p}")" 6 || { bad "点了 $1 但它没成为活动标签"; return 1; }
}

# 按 ID 找窗口：项目根是这个目录的那个窗口的 label
win_of() {
  lite windows | python3 -c 'import json,sys; print(next((w["label"] for w in json.load(sys.stdin) if w["root"] == sys.argv[1]), ""))' "$1"
}
nwin() { lite windows | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'; }

# ─────────────────── 造一个验收用的仓库 ───────────────────

lite_clean_data   # 上一次被 kill -9 的话，测试身份的数据可能还在；从干净的开始

say "造 fixture：${FIX}"
cd "${FIX}"
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
# 轮转那段（⑱）用的小日志：小到行数一眼数得清
awk 'BEGIN{for(i=0;i<100;i++) printf "2026-09-18 10:00:00 INFO  轮转前第 %d 条\n", i}' > rot.log
# 跳转要的那份**真 Maven 目录**（⑮ 用）。
#
# 桩里那个 Java 文件的 `package` 和它所在的目录对不上，所以 import / 同包
# 这两层在浏览器里根本走不到 —— 它们的全部依据就是「包路径 = 目录路径」，
# 而那是只有真实项目才有的形状。两个模块是故意的：跨模块跳转正是
# 这个功能的立身之本（api 里引用 core 的类）。
mkdir -p moduleA/src/main/java/com/demo/api moduleB/src/main/java/com/demo/core
cat > moduleA/src/main/java/com/demo/api/AdminController.java <<'JAVA'
package com.demo.api;

import com.demo.core.OrderClient;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class AdminController {
    private final OrderClient orderClient;
    private final SamePkgHelper helper;

    public AdminController(OrderClient orderClient, SamePkgHelper helper) {
        this.orderClient = orderClient;
        this.helper = helper;
    }
}
JAVA
cat > moduleA/src/main/java/com/demo/api/SamePkgHelper.java <<'JAVA'
package com.demo.api;

public class SamePkgHelper {
    public String tag() { return "ZQXJ_SAMEPKG"; }
}
JAVA
cat > moduleB/src/main/java/com/demo/core/OrderClient.java <<'JAVA'
package com.demo.core;

public class OrderClient {
    public String ping() { return "ZQXJ_CROSSMODULE"; }
}
JAVA
git add -A && git commit -qm "初始提交"
git branch feature/x
# 话多的钩子：3000 行稳稳超过管道那几十 KB 缓冲，用来复现那个死锁。
#
# **末尾那句 sleep 3 是给「进行中提示」用的**（issue #15 的 ①b）：修好之后提交不到 1 秒就完了，
# 那句「正在提交…」一闪而过，直接断言就是一条间歇红。原来走 AX 时要 sleep 8 ——
# 光是一次 AX 树遍历就以秒计；现在问一次页面是几十毫秒，3 秒绰绰有余，每轮省 5 秒
printf '#!/bin/sh\nfor i in $(seq 1 3000); do echo "smoke: 噪声 $i"; done\nsleep 3\nexit 0\n' > .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
printf 'v2 改过了\n' > note.txt
echo "  大日志 $(du -h big.log | cut -f1)，钩子 3000 行"

say "起 .app（后台，临时身份）"
lite_launch "${FIX}" || exit $?   # 2 = 测试 .app 比源码旧，见 bridge.sh 的 lite_stale
if lite_wait main "return __lite.project.root === $(q "${FIX}")" 15; then
  ok "挂载成功，项目根是 fixture"
else
  bad "15 秒内项目没开起来（项目根：$(lite eval main 'return __lite.project.root' 2>&1)）"; exit 1
fi

# ─────────────────── 1. 提交（带话多的钩子）───────────────────

say "① 提交：3000 行的 pre-commit 钩子不能把它挂住"
MSG="smoke: commit through the real app"
if ! click "Git 改动"; then
  bad "点不到「Git 改动」"
elif ! wait_exists "全部暂存" button exact 10; then
  bad "Git 面板没渲染出来"
elif ! fill "${MSG}" ".commit textarea" || ! wait_has "${MSG}" 3; then
  bad "提交信息没填进输入框（不是钩子的问题，是这一步没做成）"
else
  click "全部暂存"
  wait_for 4 'git -C "'"${FIX}"'" diff --cached --quiet; [ $? -ne 0 ]' || bad "没暂存上"
  # 盘上暂存了不等于界面已经刷过来。**暂存之后按钮名字会变成「提交 (N)」**，
  # 不等就会点到上一帧的旧按钮
  if ! wait_exists "提交 (" button prefix 10; then
    bad "界面没刷出「提交 (N)」"
  else
    click "提交 (" button prefix || bad "点不到提交按钮"
    # ①b issue #15：慢操作不能一声不吭。钩子里那句 sleep 3 撑开了窗口
    if wait_has "正在提交" 3; then
      ok "提交进行中有提示（issue #15）"
    else
      bad "点了提交但界面一声不吭 —— 和「点了没反应」分不出来"
    fi
    # **断言认「多了一条提交、标题对得上」，不认「工作区变干净」** ——
    # 后者会被任何无关的工作区噪声搅黄，而那时的失败信息会指向一个不存在的死锁
    if wait_for 40 '[ "$(git -C "'"${FIX}"'" log -1 --format=%s)" = "'"${MSG}"'" ]'; then
      ok "提交落地（$(git -C "${FIX}" log -1 --format='%h %s')）"
    else
      bad "40 秒内没提交成功 —— 多半是钩子把它挂住了（stderr 没被并发排空）"
      echo "     暂存区：$(git -C "${FIX}" diff --cached --name-only | tr '\n' ' ')"
      echo "     git log：$(git -C "${FIX}" log --oneline | head -2 | tr '\n' ' ')"
    fi
  fi
fi

# ─────────────────── 2. 保存不动文件的身份 ───────────────────

say "② ⌘P 唤得出来（它是懒加载的，首屏之后才预拉）"
# ⌘P 归网页自己接（keymap.ts 里 owner = key），所以页面里发一次 keydown 走的就是它真实的那条路。
# 认浮层用输入框的占位符：别认「随处搜索」—— 那是空态卡片上的字，浮层不出来它也在，那条断言会**永远绿**。
# （走 AX 时这条一直只能观察不计成败：占位符落在 AXPlaceholderValue 上，读不稳。现在读的是 DOM，转正）
show_tree
key "Mod-p"
if wait_has "输入文件名" 5; then
  ok "⌘P 唤出来了"
else
  bad "⌘P 没唤出浮层"
fi
key "Escape"
lite_wait "${W}" "return !__lite.has('输入文件名')" 3 || bad "Esc 没收掉 ⌘P 浮层"

say "③ 保存一个 0755 的脚本：权限不能丢"
# 走文件树而不是 ⌘P —— 这一步要验的是保存，不是打开方式；
# 混在一起的话，⌘P 抽风会被报成「保存坏了」
if open_from_tree "run.sh" && wait_has "echo" 6; then
  set_text '#!/bin/sh
echo hello from smoke'
  menu save
  wait_for 10 'grep -q "hello from smoke" "'"${FIX}"'/run.sh"'
  check "$(stat -f %Lp run.sh)" "755" "权限还是 755"
  check "$(./run.sh 2>&1)" "hello from smoke" "内容改了，而且还能执行"
else
  bad "run.sh 没打开"
fi

say "④ 保存一条软链：不能把链接换成普通文件"
if open_from_tree "link.txt" && wait_has "原始内容" 6; then
  set_text '通过 app 改过的
第二行还在'
  menu save
  wait_for 10 'grep -q "通过 app 改过的" "'"${FIX}"'/real/config.txt"'
  [ -L link.txt ] && ok "link.txt 仍然是软链" || bad "软链被换成普通文件了"
  check "$(head -1 real/config.txt)" "通过 app 改过的" "改动写进了真身"
  [ -z "$(ls -a | grep 'lite-ide-tmp')" ] && ok "没留下临时文件" || bad "留了临时文件"
else
  bad "link.txt 没打开"
fi

# ─────────────────── 4. 切分支 ───────────────────
#
# **切分支是两步的，不是一步**（2026-09-10 改成 IDEA 式）：点一行只是弹出那一行的动作菜单，
# 真正切过去的是菜单里的「检出」。理由见 `BranchPicker.svelte` 的 `rowMenu` ——
# 「点一下就切」在一个误点代价很大的操作上太轻了。
#
# 这一步 2026-09-11 才发现是坏的：改分支选择器的那一轮说了「先不跑 smoke」，
# 于是脚本一直在验一个已经不存在的行为，而它红的时候只说「没切过去」——
# **听起来像切分支坏了，其实是脚本过期了**。改 UI 的那一轮就该连它一起改。
say "⑤ 切分支"
if ! click "main"; then
  bad "点不开分支浮层（标题栏上没有叫 main 的挂件）"
# 分支行的名字是 aria-label 的全名（文件夹里只显示后半截 `x`）—— 按「含」点
elif ! wait_exists "feature/x" button contains 6 || ! click "feature/x" button contains; then
  bad "分支浮层里找不到 feature/x"
# **菜单项的角色是 menuitem，不是 button**：`ContextMenu.svelte` 里是 `<button role="menuitem">`。
# 只认「检出」两个字：菜单项的措辞跟 IDEA 走，分支名不在上面
elif wait_exists "检出" menuitem contains 8 && click "检出" menuitem contains; then
  wait_for 20 '[ "$(git -C "'"${FIX}"'" rev-parse --abbrev-ref HEAD)" = "feature/x" ]' \
    && ok "切到了 feature/x" || bad "点了「检出」但分支没变"
else
  bad "行菜单里没有「检出」—— 是不是又改回一步了？"
fi

# ─────────────────── 5. 大日志：正文不能是乱码 ───────────────────

say "⑥ 打开 26MB 的中文日志：正文不能是乱码"
RSS_BEFORE=$(ps -o rss= -p "$(lite_pid)" | tr -d ' ')
if open_from_tree "big.log"; then
  if wait_has "服务处理完成" 10; then
    ok "中文正常（界面上读回了「服务处理完成」）"
  else
    bad "正文乱码 —— detect_encoding 的样本又被切在半个字符上了（见 rules/rust.md）"
  fi
  wait_has "400,000" 10 && ok "行数统计对（400,000）" || bad "行数统计不对"
fi
RSS_OPEN=$(ps -o rss= -p "$(lite_pid)" | tr -d ' ')

say "⑦ 关掉大日志：mmap 要跟着放掉"
click "关闭 big.log" || bad "点不到关闭按钮"
sleep 3
RSS_AFTER=$(ps -o rss= -p "$(lite_pid)" | tr -d ' ')
echo "  RSS 打开前 ${RSS_BEFORE} → 开着 ${RSS_OPEN} → 关掉 ${RSS_AFTER} KB"
[ "${RSS_AFTER}" -lt "${RSS_OPEN}" ] && ok "内存降下来了" || bad "关掉之后内存没降"

say "⑱ 日志轮转：tail 不断、按名重开（2026-09-18）"
#
# logback 每晚一次的事：改名 + 新建同名。原来这时候关 tail、报「请重新打开」。
# 现在 Rust 侧同一个句柄按名重开，前端按原条件重跑，状态栏说一句「轮转过 N 次」。
# 三步：开 rot.log 打开 tail → 追加三行（行数 100 → 103，证明 tail 活着）
# → mv + 新建 5 行的同名文件（行数变成 5、状态栏有「轮转过 1 次」，证明跟上了新文件）。
if open_from_tree "rot.log"; then
  # 4KB 的 .log 按大小判定走的是编辑模式（日志模式是给大文件的），状态栏那格点一下切过去
  if click "编辑模式" button contains && wait_has "100 行" 8; then
    ok "rot.log 切到日志模式（100 行）"
    if click "跟随尾部" button contains; then
      printf '追加 1\n追加 2\n追加 3\n' >> "${FIX}/rot.log"
      wait_has "103 行" 6 && ok "tail 活着：追加三行后 103 行" || bad "tail 没跟上追加（还不是 103 行）"
      mv "${FIX}/rot.log" "${FIX}/rot.log.1"
      sleep 0.3
      printf '新 1\n新 2\n新 3\n新 4\n新 5\n' > "${FIX}/rot.log"
      if wait_has "轮转过 1 次" 8; then
        ok "轮转被认出来了，tail 还开着"
        wait_has "5 行" 4 && ok "句柄背后已经是新文件（5 行）" || bad "轮转后行数不是新文件的"
      else
        bad "轮转没被认出来（状态栏没有「轮转过 1 次」）"
      fi
    else
      bad "点不到「跟随尾部」"
    fi
  else
    bad "rot.log 没切到日志模式"
  fi
fi
click "关闭 rot.log" || true

# ─────────────────── 6. 全局搜索 / 废纸篓 / 远程 ───────────────────
#
# 下面这三段补的是 issue #11 清单里剩下的那几条命令：`grep_project`、`trash_entry`、`git_fetch`、`git_push`。

say "⑧ ⇧⌘F 全局搜索（grep_project）"
# ⇧⌘F 归菜单（owner = menu），走菜单指令。浮层弹出来自己会把焦点放进输入框，fill 填的就是它
menu quick-content
if ! wait_has "换范围" 8; then
  bad "⇧⌘F 的浮层没出来"
elif ! fill "ZQXJ_SMOKE_NEEDLE"; then
  bad "浮层出来了，但焦点不在输入框上"
# 搜索要扫整个 fixture，里面躺着那个 26MB 的大日志 —— 给足时间。
# **结果行是一整个按钮**（命中行 + 路径 + 行号合成一个名字），按「含」找文件名
elif wait_results "needle.txt" 25; then
  ok "搜到了 deep/nested/needle.txt"
  key "Enter"   # 打开第一条命中
  # 断言认第二行 —— 第一行是命中行，浮层上本来就印着它
  if lite_wait "${W}" "return __lite.text()?.includes('这一行要把文件打开才看得见') === true" 10; then
    ok "点得开，打开的确实是那个文件"
  else
    bad "搜到了但没打开（命令回来了，前端跳转那一步断了？）"
  fi
else
  bad "25 秒内没搜到那根针"
  key "Escape"
fi

# ── 同一条命令，再验 issue #19：软链文件的内容也要搜得到 ──
#
# ④ 把「通过 app 改过的」写进了 `real/config.txt`，而 `link.txt` 指向它。
# 修好之前，rg 和内置实现**都**整个跳过 symlink，于是结果里只有
# `real/config.txt` 那一条 —— 一个在文件树里点得开、存得进去的文件，在搜索里够不着。
#
# **等上一个浮层真的退场再开第二次**：关闭是异步的，紧接着再开会被还没走的浮层吃掉
lite_wait "${W}" "return !__lite.has('换范围')" 6
menu quick-content
if ! wait_has "换范围" 8; then
  bad "第二次 ⇧⌘F 的浮层没出来"
elif ! fill "通过 app 改过的"; then
  bad "第二次 ⇧⌘F 焦点不在输入框上"
elif wait_results "link.txt" 25; then
  ok "软链文件的内容也搜得到（issue #19）"
else
  bad "搜不到 link.txt —— issue #19 回归了（symlink 又被整个跳过？）"
fi
key "Escape"
lite_wait "${W}" "return !__lite.has('换范围')" 4

# ── 同一条命令，三个开关真的传到了 Rust（#42）──
#
# 原来 ⇧⌘F 用 rg 的 smart-case：小写的词不分大小写、带大写的就区分，界面上看不出来。现在是显式的开关，
# 默认不分大小写。这一段验「开关从浮层经 IPC 到了 Rust 的匹配器」：同一个小写的词，开关关着搜得到大写的针，
# 打开「区分大小写」就一条都没有。只在前端改了状态、没传下去的话，两次结果一样
menu quick-content
if ! wait_has "换范围" 8 || ! fill "zqxj_smoke_needle"; then
  bad "第三次 ⇧⌘F 的浮层没出来"
else
  wait_results "needle.txt" 25 && ok "开关全关：小写的词搜得到大写的针（不分大小写是默认）" || bad "开关全关时搜不到大写的针"
  click "区分大小写" button prefix
  lite_wait "${W}" "return document.querySelector('.toggles button')?.getAttribute('aria-pressed') === 'true'" 3
  if lite_wait "${W}" "return !document.querySelector('.popup .results')?.innerText.includes('needle.txt')" 10; then
    ok "打开「区分大小写」：小写的词不再命中大写的针（开关传到了 Rust）"
  else
    bad "打开「区分大小写」之后结果没变 —— 开关没传下去？"
  fi
  # 关回去：开关记在 store 里，浏览器桩和后面几段都按默认来
  click "区分大小写" button prefix
fi
key "Escape"
lite_wait "${W}" "return !__lite.has('换范围')" 4

say "⑨ 移到废纸篓（trash_entry）：不能是真删除"
show_tree
ROW="[role=\"treeitem\"][data-path=\"${FIX}/${TRASH_NAME}\"]"
if ! is "return __lite.rightClick($(q "${ROW}"))"; then
  bad "文件树里找不到 ${TRASH_NAME}"
elif ! wait_exists "移到废纸篓" menuitem contains 6; then
  bad "右键没开出上下文菜单（或者菜单里没有这一项）"
elif ! click "移到废纸篓" menuitem contains; then
  bad "菜单出来了，但点不到「移到废纸篓」"
else
  # 确认卡片上的是真 <button>，回到 button 角色
  if ! wait_exists "移到废纸篓" button contains 6; then
    bad "确认框没出来"
  fi
  click "移到废纸篓" button contains || bad "确认框上点不到「移到废纸篓」"
  if wait_for 15 '[ ! -e "'"${FIX}"'/'"${TRASH_NAME}"'" ]'; then
    ok "${TRASH_NAME} 从工作区没了"
    # **进废纸篓才算对，不是真删除。按具体路径查，不要列目录**：没有「完全磁盘访问」权限时
    # `ls ~/.Trash` 会 Operation not permitted 返回空，而 `stat ~/.Trash/具体文件名` 读得到。
    # 找到了也不动它：那是你的废纸篓（结尾提示一句）
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

say "⑩ 推送 / 拉取（git_push / git_fetch）"
# **remote 是这一步现加的，不写进 fixture。** 一开始就有上游的话，
# 分支挂件的名字会跟着变（`main` → 带上 ↑N 之类），而 ⑤ 是按精确名字点它的。
REMOTE="${WORK}/origin.git"
OTHER="${WORK}/other"
git init -q --bare "${REMOTE}"
git remote add origin "${REMOTE}"
BR=$(git rev-parse --abbrev-ref HEAD)
echo "  当前分支 ${BR}，远程 ${REMOTE}"

menu git-push
# 没有上游时按钮是「推送并跟踪」，有上游时是「推送」—— 这里必然是前者
if ! wait_exists "推送并跟踪" button exact 6 || ! click "推送并跟踪"; then
  bad "推送确认条没出来（或按钮不叫这个名字）"
elif wait_for 30 'git -C "'"${REMOTE}"'" rev-parse --verify -q "'"${BR}"'"'; then
  check "$(git -C "${REMOTE}" rev-parse "${BR}")" "$(git rev-parse HEAD)" "推上去的 sha 和本地一致"
else
  bad "30 秒内没推上去"
fi

# 造一个「别人推了新东西」的远程，再从界面上拉。
#
# **`-b ${BR}` 不能省。** bare 仓库的 HEAD 建出来就指向默认的 `main`，而我们只往它推了 `feature/x` ——
# 不带 `-b` 的话 clone 检出一个空工作区，后面的 commit 和 push 全部失败。
git clone -q -b "${BR}" "${REMOTE}" "${OTHER}" 2>/dev/null
git -C "${OTHER}" config user.email smoke@local
git -C "${OTHER}" config user.name smoke
printf '从另一个克隆推上来的\n' > "${OTHER}/from-remote.txt"
git -C "${OTHER}" add -A
git -C "${OTHER}" commit -qm "远程的新提交"
if git -C "${OTHER}" push -q origin "HEAD:${BR}" 2>/dev/null; then
  UP=$(git -C "${OTHER}" rev-parse HEAD)
  menu git-pull
  # 拉取 = fetch + 本地合并两步（不是 git pull）。这里必然是快进：
  # 新提交只加了一个文件，碰不到 ③④ 改过的那两个
  if wait_for 40 '[ "$(git -C "'"${FIX}"'" rev-parse HEAD)" = "'"${UP}"'" ]'; then
    ok "拉取把本地推进到了远程那一条（$(echo "${UP}" | cut -c1-7)）"
    [ -f "${FIX}/from-remote.txt" ] && ok "新文件落到了工作区" || bad "HEAD 动了但文件没落盘"
  else
    bad "40 秒内没拉下来（HEAD 还停在 $(git -C "${FIX}" rev-parse --short HEAD)）"
  fi
else
  bad "造不出远程的新提交 —— 这一步是脚本自己的问题，不是应用的"
fi

say "⑬ 切标签不能丢掉未保存的改动（issue #9 的护栏）"
#
# 这条护的是 v0.4.1 修过的那个 bug，也是这个仓库栽得最狠的一次：
# 编辑器被 `{#key active.id}` 包着，切标签就是**销毁重建**，而它的实时文本
# 从来没被存回去 —— 切走再切回来，改动和标签上那个「有未保存改动」的圆点
# **一起**消失，界面干干净净，人根本不会察觉自己丢了东西。
MARK="切标签不该丢的内容"
if open_from_tree "note.txt"; then
  set_text "${MARK}"
  # 切走：打开另一个文件（**不保存** note.txt），再切回来
  if open_from_tree "run.sh" && open_from_tree "note.txt"; then
    if lite_wait "${W}" "return __lite.text() === $(q "${MARK}")" 8; then
      ok "切走再切回来，未保存的改动还在"
      # 盘上那份必须还是旧的 —— 这条顺带证明「还在」的是草稿而不是「其实已经被存进去了」
      if grep -q "${MARK}" "${FIX}/note.txt" 2>/dev/null; then
        bad "内容被写进盘了 —— 这一步不该保存"
      else
        ok "盘上那份没动（还在的是草稿，不是被偷偷存了）"
      fi
    else
      bad "切回来之后改动没了 —— v0.4.1 那个 bug 回来了"
    fi
  fi
  # **把这个脏标签存掉再走。** 留着未保存的改动，⑫ 那句「关闭所有标签」会弹确认框，
  # 标签关不掉、editors 也就不归零 —— 报出来是「编辑器实例没释放」，而那跟释放一点关系都没有
  menu save
  wait_for 6 'grep -q "'"${MARK}"'" "'"${FIX}"'/note.txt"'
fi

say "⑭ 草稿：⌘N 建出来、不按 ⌘S 也落盘、空的关掉就丢"
#
# **必须在真 .app 上测**：「新建草稿」是菜单项，桩里没有原生菜单栏；
# 草稿目录是 Tauri 的 `app_data_dir()` 算出来的，桩里那条是编的。
# 草稿目录在测试身份的数据目录里，跑完整个删掉，不用一份份收拾
SCRATCHES="${LITE_DATA}/scratches"
mkdir -p "${SCRATCHES}"
ls "${SCRATCHES}" 2>/dev/null | sort > "${WORK}/scratch.before"

menu new-scratch
wait_for 4 '[ -n "$(ls "'"${SCRATCHES}"'" | sort | comm -13 "'"${WORK}"'/scratch.before" -)" ]'
NEW1=$(ls "${SCRATCHES}" 2>/dev/null | sort | comm -13 "${WORK}/scratch.before" - | head -1)
if [ -z "${NEW1}" ]; then
  bad "⌘N 没有在草稿目录里建出东西"
else
  # 名字形如 `2026-09-09 1408.md` —— 时间戳是翻回来时唯一记得的线索
  if printf '%s' "${NEW1}" | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{4}(-[0-9]+)?\.md$'; then
    ok "建出了 ${NEW1}"
  else
    bad "草稿名字不对：${NEW1}"
  fi
  # 写点东西，**不存** —— 草稿自动落盘（issue #40）。断言落在**盘上**，不读界面。
  # 停止输入 500ms 就该写，等 4 秒是给 IPC 和机器忙留余量
  DRAFT="排查用的 traceId b67c353d"
  lite_wait "${W}" "return __lite.tabs.active?.path === $(q "${SCRATCHES}/${NEW1}")" 4
  set_text "${DRAFT}"
  if wait_for 4 'grep -q "'"${DRAFT}"'" "'"${SCRATCHES}/${NEW1}"'"'; then
    ok "没按 ⌘S，草稿自己写进了盘上那份文件"
  else
    bad "草稿没有自动落盘（或存到别处去了）"
  fi
  # 关草稿标签不该弹「保存 / 丢弃」
  menu close-tab
  sleep 1
  if exists "丢弃改动" button contains; then
    bad "关草稿标签还在问「保存 / 丢弃」"
    click "丢弃改动" button contains
  else
    ok "关草稿标签没有问"
  fi
fi

# 二、点了加号又一个字没写：关掉就该把那个 0 字节的文件丢掉
ls "${SCRATCHES}" 2>/dev/null | sort > "${WORK}/scratch.before2"
menu new-scratch
wait_for 4 '[ -n "$(ls "'"${SCRATCHES}"'" | sort | comm -13 "'"${WORK}"'/scratch.before2" -)" ]'
menu close-tab
sleep 1.5
ls "${SCRATCHES}" 2>/dev/null | sort > "${WORK}/scratch.after2"
if diff -q "${WORK}/scratch.before2" "${WORK}/scratch.after2" >/dev/null; then
  ok "空草稿关掉之后盘上没留下东西"
else
  bad "空草稿留在盘上了：$(comm -13 "${WORK}/scratch.before2" "${WORK}/scratch.after2" | tr '\n' ' ')"
fi

say "⑮ ⌘B 跳到声明：跨模块的 import，和不写 import 的同包"
#
# **这一段只有真 .app 跑得了。** 两层的依据都是「包路径 = 目录路径」，
# 而浏览器里那个桩的 Java 文件 package 和目录对不上（见 fixture 那段）。
# ⌘B 归 CM6（owner = cm6），光标放好、编辑器拿到焦点，页面里发的 keydown 走的就是它真实的 keymap。
#
# 行列对着 fixture 里那份 AdminController.java 数（都从 1 起）：
#   第 8 行 `    private final OrderClient orderClient;`   第 25 列落在 OrderClient 上
#   第 9 行 `    private final SamePkgHelper helper;`      第 25 列落在 SamePkgHelper 上
#   第 6 行 `@RestController`                              第 9 列
ADMIN="${FIX}/moduleA/src/main/java/com/demo/api/AdminController.java"
jump_at() {
  is "__lite.caret($1, $2); __lite.key('Mod-b'); return true"
}
# **用 ⌘P 开，不用文件树**：这几个文件在六层目录底下，文件树要一层层展开
open_by_quick() {
  key "Mod-p"
  wait_has "输入文件名" 5 || { bad "⌘P 浮层没出来"; return 1; }
  fill "$1" || { bad "⌘P 的焦点不在输入框上"; return 1; }
  wait_results "$1" 8 || { bad "⌘P 里搜不到 $1"; return 1; }
  key "Enter"
  lite_wait "${W}" "return __lite.tabs.active?.path?.endsWith($(q "/$1")) === true" 6
}

if ! open_by_quick "AdminController.java"; then
  bad "打不开 AdminController.java"
else
  # ① 跨模块：import com.demo.core.OrderClient → moduleB 那份
  jump_at 8 25
  if lite_wait "${W}" "return __lite.tabs.active?.path?.endsWith('/OrderClient.java') === true" 8; then
    ok "⌘B 跨模块跳到了 moduleB 的 OrderClient.java"
  else
    bad "跨模块跳转没到（import 那一层）"
  fi

  # ② 同包：SamePkgHelper 不写 import，靠 package 声明推同目录
  if ! open_by_quick "AdminController.java"; then
    bad "切不回 AdminController.java"
  else
    jump_at 9 25
    if lite_wait "${W}" "return __lite.tabs.active?.path?.endsWith('/SamePkgHelper.java') === true" 8; then
      ok "⌘B 跳到了同包的 SamePkgHelper.java（它没有 import）"
    else
      bad "同包跳转没到"
    fi
  fi

  # ③ 第三方不该跳：RestController 在 jar 里，项目索引里没有它的源码。
  #    这一条守的是「有下划线 = 我确定」——它比前两条更要紧，因为跳错了人是不会怀疑的。
  #    **它单独绿不算数**：⌘B 压根没触发它也会绿。三条是一组，看结果要一起看。
  if ! open_by_quick "AdminController.java"; then
    bad "切不回 AdminController.java"
  else
    jump_at 6 9
    sleep 2
    check "$(active)" "${ADMIN}" "第三方（jar 里的）按 ⌘B 不动，停在原地"
  fi
fi

say "㉑ ⌘P 一个框走到底：文件@符号、文件:行:列、单独的 @ 和 :（issue #43）"
#
# Sublime 的 Goto Anything。符号来自 Lezer 语法树（和 ⇧⌘O 的大纲同一套），没打开的文件现读现解析。
# 断言认「活动标签 + 光标落在哪一行哪一列」，不认浮层上印了什么 —— 列表对了而回车跳歪了，用户看到的是后者
goto_by_quick() {
  key "Mod-p"
  wait_has "输入文件名" 5 || { bad "⌘P 浮层没出来"; return 1; }
  fill "$1" || { bad "⌘P 的焦点不在输入框上"; return 1; }
  wait_results "$2" 8 || { bad "「$1」的结果里没有「$2」"; key "Escape"; return 1; }
  key "Enter"
}
where_is() { lite eval "${W}" "return __lite.tabs.active?.path.split('/').pop() + ' ' + __lite.where()" 2>/dev/null; }
# OrderClient.java 第 4 行：`    public String ping() { … }`
if goto_by_quick "OrderClient@ping" "OrderClient.java:4"; then
  wait_for 6 '[ "$(where_is)" = "OrderClient.java 4:1" ]'
  check "$(where_is)" "OrderClient.java 4:1" "文件@符号：没打开的文件现解析，回车跳到 ping 那一行"
fi
# SamePkgHelper.java 第 4 行第 12 列：`    public String tag()` 里 String 的 S
if goto_by_quick "SamePkgHelper:4:12" "SamePkgHelper.java:4:12"; then
  wait_for 6 '[ "$(where_is)" = "SamePkgHelper.java 4:12" ]'
  check "$(where_is)" "SamePkgHelper.java 4:12" "文件:行:列"
fi
# 当前文件（还是 SamePkgHelper）：单独的 @ 和 :
if goto_by_quick "@SamePkg" "SamePkgHelper.java:3"; then
  wait_for 6 '[ "$(where_is)" = "SamePkgHelper.java 3:1" ]'
  check "$(where_is)" "SamePkgHelper.java 3:1" "单独的 @：当前文件的符号"
fi
if goto_by_quick ":1" "第 1 行"; then
  wait_for 6 '[ "$(where_is)" = "SamePkgHelper.java 1:1" ]'
  check "$(where_is)" "SamePkgHelper.java 1:1" "单独的 :：当前文件跳行"
fi

# ─────────────────── 收尾 ───────────────────

say "⑯ 系统送来的文件（open -a）：进已开着的窗口，不起第二个进程"
#
# Finder 双击 / 拖 Dock / 「打开方式」/ `open -a` 走的都是同一个 Apple Event
# （`RunEvent::Opened`，issue #40），命令行参数一条都接不到。这里用 `open -a` 代表那四条路 ——
# 它是唯一能从脚本里发的。`-g`：后台送，不把应用拉到前面。路径故意带中文和空格：
# 事件里是 `file://` 百分号编码，解错了就是「文件不存在」。
mkdir -p "${FIX}/odoc 目录"
printf 'odoc probe\n' > "${FIX}/odoc 目录/系统送来 的.txt"
open -g -a "${LITE_APP}" "${FIX}/odoc 目录/系统送来 的.txt"
if wait_exists "关闭 系统送来 的.txt" button exact 8; then
  ok "open -a 送来的文件开成了标签"
else
  bad "open -a 送来的文件没开（RunEvent::Opened 没接上？）"
fi
check "$(pgrep -f "lite-ide-mwtest.app/Contents/MacOS" | wc -l | tr -d ' ')" "1" "还是一个进程（Launch Services 发给了已在运行的实例）"

say "⑰ 别人给的仓库：.git/config 里有会执行命令的键，Git 不启用、一个命令都不跑（issue #24）"
#
# 威胁模型见 gitsvc/src/trust.rs 头上：clone 不带 config，会带的是「整个目录拿到手」。
# 这里就造一个这样的目录：config 里一条 filter.evil.clean，值是 touch 一个标记文件。
# 断言三条：标记文件不存在（没执行）、挂件写着「Git 未启用」、点信任之后 Git 出来。
# 用 `open -a` 送目录进去 —— 和 ⑯ 同一条路，也正是「别人 AirDrop 一个文件夹双击」那条。
TRAP="${WORK}/trap-repo"; PWNED="${WORK}/pwned"
mkdir -p "${TRAP}"
git -C "${TRAP}" init -q -b main
git -C "${TRAP}" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init
git -C "${TRAP}" config filter.evil.clean "touch '${PWNED}'; cat"
printf '*.txt filter=evil\n' > "${TRAP}/.gitattributes"
printf 'bait\n' > "${TRAP}/bait.txt"
rm -f "${PWNED}"
open -g -a "${LITE_APP}" "${TRAP}"
# 主窗口已经开着一个项目，再送一个目录进来是**开新窗口**，不是在原窗口里换项目
wait_for 10 '[ -n "$(win_of "'"${TRAP}"'")" ]'
TRAPWIN=$(win_of "${TRAP}")
check "$(nwin)" "2" "送来另一个目录：开了新窗口（不是在原窗口里换项目）"
if [ -z "${TRAPWIN}" ]; then
  bad "找不到项目根是 trap-repo 的窗口，这一段后面的断言没法做"
else
  W="${TRAPWIN}"
  lite_wait "${W}" "return true" 10
  if wait_exists "Git 未启用" button contains 10; then
    ok "受限：挂件写着「Git 未启用」"
    [ ! -e "${PWNED}" ] && ok "config 里那条命令没被执行" || bad "config 里的命令被执行了 —— 白名单没拦住"
    if click "Git 未启用" button contains && wait_exists "信任这个仓库" button exact 5; then
      ok "点开是确认卡片"
      if click "信任这个仓库" && wait_exists "Git 改动" button exact 10; then
        ok "信任之后 Git 出来了"
      else
        bad "点了信任，Git 没出来"
      fi
    else
      bad "点挂件没开出确认卡片"
    fi
  else
    bad "有可疑 config 的仓库没进受限（挂件上没有「Git 未启用」）"
  fi
  W=main
  # 关掉这个窗口（和点红叉一样）：后面几段都默认只有主窗口
  lite close "${TRAPWIN}" >/dev/null
  wait_for 6 '[ "$(nwin)" = 1 ]'
fi
# 信任记在测试身份的 trust.json 里，跑完整个删掉

say "⑲ 两个窗口：送来别的项目开新窗口，文件进它自己的窗口，关掉它不碰主窗口的终端（多窗口，2026-10-08）"
#
# 多窗口第 1–4 步的回归。⑰ 已经验了「送另一个目录 → 开新窗口」这一条，这里补三条只有两个窗口
# 同时开着才会出错的：路由（文件进项目包含它的那个窗口）、去重（同一个项目不开第二个窗口）、
# 资源归属（关掉一个窗口只收它自己的终端 —— 原来是关任何一个窗口就杀掉所有终端）。
FIX2="${WORK}/second-proj"; mkdir -p "${FIX2}"; printf 'second\n' > "${FIX2}/f2.txt"
open -g -a "${LITE_APP}" "${FIX2}"
wait_for 10 '[ -n "$(win_of "'"${FIX2}"'")" ]'
W2=$(win_of "${FIX2}")
check "$(nwin)" "2" "送来另一个项目：开了第二个窗口"
if [ -z "${W2}" ]; then
  bad "找不到项目根是 second-proj 的窗口，这一段后面的断言没法做"
else
  lite_wait "${W2}" "return true" 10
  open -g -a "${LITE_APP}" "${FIX2}/f2.txt"
  W="${W2}"
  if wait_exists "关闭 f2.txt" button exact 8; then ok "那个项目里的文件进了它自己的窗口"; else bad "f2.txt 没开在 ${W2} 的窗口里"; fi
  W=main
  exists "关闭 f2.txt" button exact && bad "f2.txt 也开在了主窗口里（路由没生效）" || ok "主窗口里没有它"
  open -g -a "${LITE_APP}" "${FIX2}"; sleep 1.5
  check "$(nwin)" "2" "再送一次同一个项目：回到那个窗口，不开第三个"
  # 主窗口开一个终端，再关掉第二个窗口：主窗口的终端要还在
  menu new-terminal
  MAINPID=$(lite_pid)
  wait_for 6 '[ -n "$(pgrep -P "'"${MAINPID}"'" zsh)" ]'
  ZSH1=$(pgrep -P "${MAINPID}" zsh | head -1)
  if [ -z "${ZSH1}" ]; then
    bad "主窗口的终端没起来（这条后面的断言没法做）"
  else
    lite close "${W2}" >/dev/null
    wait_for 10 '[ "$(nwin)" = 1 ]'
    sleep 1
    check "$(nwin)" "1" "第二个窗口关掉了"
    kill -0 "${ZSH1}" 2>/dev/null && ok "主窗口的终端还活着（关掉别的窗口不杀它）" || bad "关掉第二个窗口，主窗口的终端被杀了"
    menu close-terminal
  fi
fi

say "⑪ 界面自己有没有报错"
ERRS=$(grep -icE "\[diag/web\].*(error|fatal)|CSP 挡下" "${LOG}")
check "${ERRS}" "0" "诊断通道里没有前端报错 / CSP 违规"
# 不变量自检（issue #27）写的是 app.log 不是 stderr。上面十几段开文件、打字、切标签、
# 关标签把每个转换点都走了一遍 —— 有一条 [invariant] 就是真的有 bug（#36）。
# 测试身份的数据目录是这次跑之前清空的，整份 app.log 都是这一轮的
INV=$(grep -c "\[invariant\]" "${APPLOG}" 2>/dev/null || true)
if [ "${INV:-0}" = "0" ]; then
  ok "这一轮没有不变量报警"
else
  bad "不变量报了 ${INV} 条："
  grep "\[invariant\]" "${APPLOG}" | head -5 | sed 's/^/      /'
fi

say "⑫ 关掉全部标签之后，编辑器实例要归零（issue #10）"
#
# 这条读的是 `[diag/web] mem editors=N`，前端在 `LITE_IDE_DEBUG=1` 时每 3 秒报一次的**对象数**。
# 为什么不看进程内存：Physical footprint 噪声有 ±15MB，比要测的信号还大；而这个数是确定的。
#
# **验过红**：把 `Editor.svelte` 的 cleanup 改成不 `view.destroy()`、把 DOM 搬到 body 上，
# 重新打包跑一遍 —— 开 3 个标签时 editors 从 1 变成 3、关完之后停在 3 不归零，这条稳稳变红。
menu close-all-tabs
sleep 5   # 诊断定时器 3 秒一报，等它在关完之后至少再报一次
LAST=$(grep "mem editors" "${LOG}" | tail -1)
echo "  ${LAST:-（一条 mem 行都没有）}"
case "${LAST}" in
  *"editors=0"*) ok "editors 归零，EditorView 释放了" ;;
  "")            bad "诊断通道里一条 mem 行都没有 —— LITE_IDE_DEBUG 没生效？" ;;
  *)             bad "标签全关了，但 editors 没归零 —— 编辑器实例没释放" ;;
esac

say "⑳ 焦点：测试应用起来之后一次都没跑到最前面"
check "${STOLEN}" "0" "每段开头采样，测试应用在最前 ${STOLEN} 次 —— 你可以照常用电脑"

[ "${TRASHED:-0}" = 1 ] && echo "  （⑨ 往废纸篓里放了 ${TRASH_NAME}，脚本不动它 —— 自己清或者放回原处）"

printf '\n\033[1m通过 %d 条，失败 %d 条\033[0m\n' "${PASS}" "${FAIL}"
[ "${FAIL}" -eq 0 ] || exit 1
