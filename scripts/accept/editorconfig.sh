#!/usr/bin/env bash
# `.editorconfig`（fsservice::editorconfig + editor/save-actions.ts）在真 .app 上：缩进照它（状态栏那格和编辑器一致）、存盘时去行尾空格 / 补末尾换行
# 是编辑器里的一笔修改（⌘Z 撤得回）、光标所在那行的空格留着、两条存盘的路（编辑器里的 ⌘S、菜单「保存」）结果一样。走测试通道（scripts/lib/bridge.sh）
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }

P=$(cd "$(mktemp -d /tmp/acEc.XXXX)" && pwd -P)
trap 'lite_teardown; rm -rf "${P}"' EXIT
printf 'root = true\n\n[*]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n\n[*.java]\nindent_style = space\nindent_size = 2\n\n[*.md]\ntrim_trailing_whitespace = false\n' > "${P}/.editorconfig"
# 内容按 4 空格缩进写的：只按内容猜会猜成 4，.editorconfig 说 2
printf 'class A {\n    int x;   \n    void f() {\n        g();  \n    }\n}' > "${P}/A.java"
printf 'line one  \nline two  ' > "${P}/note.md"
printf 'class B {}' > "${P}/B.java"

lite_clean_data
lite_launch "${P}" || exit 1
open_file() { lite_eval main "await __lite.tabflow.openPath('${P}/$1', { preview: false }); return true" >/dev/null; lite_wait main "return !!document.querySelector('.cm-content')" 10 >/dev/null; sleep 0.8; }
SB='return ([...document.querySelectorAll("button")].map((x) => x.textContent.trim()).find((t) => /^(\d+ 空格|Tab) · /.test(t)) ?? "").split(" · ")[0]'
W='const W = (ms) => new Promise((r) => setTimeout(r, ms));'

echo "== 缩进照 .editorconfig：内容像 4 空格，它说 2；状态栏那格和编辑器里敲出来的一致"
open_file A.java
ok "$([ "$(lite_eval main "${SB}")" = "2 空格" ] && echo 1)" "状态栏：$(lite_eval main "${SB}")（只按内容猜是 4 空格）"
IND=$(lite_eval main "${W} __lite.caret(1, 10); __lite.key('Enter'); await W(100); const l = __lite.text().split('\\n')[1]; __lite.key('Mod-z'); await W(100); return JSON.stringify(l)")
ok "$([ "${IND}" = '"  "' ] && echo 1)" "在类的花括号后回车，新行缩进 2 个空格：${IND}"

echo "== 编辑器里 ⌘S：去行尾空格、补末尾换行；光标那行的空格留着；⌘Z 撤得回"
lite_eval main "${W} __lite.caret(4, 13); __lite.type(' '); await W(100); __lite.key('Mod-s'); await W(800); return true" >/dev/null
DISK=$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' "${P}/A.java")
ok "$([ "${DISK}" = '"class A {\n    int x;\n    void f() {\n        g();   \n    }\n}\n"' ] && echo 1)" "盘上：别的行去掉了行尾空格、末尾补了换行、光标那行（g();）的空格留着：${DISK}"
ok "$([ "$(lite_eval main "return __lite.tabs.active.dirty")" = false ] && [ "$(lite_eval main "return JSON.stringify(__lite.text())")" = "${DISK}" ] && echo 1)" "存完编辑器里和盘上一样、不是「未保存」"
lite_eval main "${W} __lite.key('Mod-z'); await W(150); return true" >/dev/null
ok "$(lite_eval main "const t = __lite.text(); return t.includes('int x;   ') && t.includes('g();   ') && !t.endsWith('\\n') && __lite.tabs.active.dirty" | grep -qx true && echo 1)" "⌘Z 一下只撤存盘时的处理：行尾空格回来、末尾换行没了，刚敲的那个空格还在（单独一步，不和打字并在一起）"
lite_eval main "${W} __lite.key('Mod-Shift-z'); await W(150); return true" >/dev/null

echo "== 光标在最后一行末尾刚敲了字就存：补的换行紧挨着那几个字，⌘Z 也只撤换行（存盘处理单独成一步撤销）"
open_file B.java
lite_eval main "${W} __lite.caret(1, 11); __lite.type('x'); await W(100); __lite.key('Mod-s'); await W(800); __lite.key('Mod-z'); await W(150); return true" >/dev/null
BZ=$(lite_eval main "return JSON.stringify(__lite.text())")
ok "$([ "${BZ}" = '"class B {}x"' ] && echo 1)" "⌘Z 之后：补的换行撤掉了，刚敲的 x 还在：${BZ}"

echo "== 菜单「保存」（焦点不在编辑器里的那条路）：同样处理；[*.md] 关了去行尾空格，只补换行"
open_file note.md
lite_eval main "${W} __lite.caret(1, 1); __lite.type('# '); await W(100); document.activeElement?.blur(); return true" >/dev/null
lite menu save main >/dev/null
sleep 1
MD=$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' "${P}/note.md")
ok "$([ "${MD}" = '"# line one  \nline two  \n"' ] && echo 1)" "md 的行尾空格照留（Markdown 的换行靠它）、末尾补了换行：${MD}"

echo "== 没有 .editorconfig 的项目外文件：一处不改"
O=$(mktemp -d /tmp/acEcO.XXXX)
printf 'x  \ny' > "${O}/plain.txt"
lite_eval main "await __lite.tabflow.openPath('${O}/plain.txt', { preview: false }); return true" >/dev/null
sleep 0.8
lite_eval main "${W} __lite.caret(2, 2); __lite.type('z'); await W(100); __lite.key('Mod-s'); await W(800); return true" >/dev/null
ok "$([ "$(cat "${O}/plain.txt")" = "$(printf 'x  \nyz')" ] && echo 1)" "照原样存（只多了敲进去的 z）：$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' "${O}/plain.txt")"
rm -rf "${O}"

lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
