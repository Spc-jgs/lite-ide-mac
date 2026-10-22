#!/usr/bin/env bash
# #44 设置的多窗口验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。被测的是临时身份，碰不到你的 lite-ide 数据。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
trap 'lite_teardown; rm -rf "${A:-}" "${B:-}"' EXIT

# 焦点：测试应用起来之后**一次都不该跑到最前面**。判的是「最前面是不是它」，不是「最前面一直是开始时那个」——
# 你在测试跑的时候切换应用是正常的，那不算被抢（第一版就是这么误报的）。启动那一下见 bridge.sh 的 lite_launch
STOLEN=0
note_front() { [ "$(lite_front_id)" = "${LITE_ID}" ] && STOLEN=$((STOLEN + 1)); return 0; }

lite_clean_data
A=$(mktemp -d /tmp/acA.XXXX); B=$(mktemp -d /tmp/acB.XXXX); A=$(cd "${A}" && pwd -P); B=$(cd "${B}" && pwd -P)
echo "alpha" > "${A}/a.txt"; echo "bravo" > "${B}/b.txt"

echo "== 起两个窗口（后台）"
lite_launch "${A}" || exit 1; note_front
lite_eval main "await __lite.tabflow.openPath('${A}/a.txt'); return __lite.tabs.active?.path" >/dev/null
lite_eval main "await __lite.tabflow.openPath('${B}'); return true" >/dev/null   # 有项目的窗口开另一个目录 = 开新窗口
lite_wait w-1 "return __lite.project.root === '${B}'" 15
lite_eval w-1 "await __lite.tabflow.openPath('${B}/b.txt'); return true" >/dev/null; note_front
W=$(lite windows)
ok "$(python3 -c "import json,sys; w=json.loads(sys.argv[1]); print(1 if sorted(x['root'] for x in w)==sorted(sys.argv[2:]) else 0)" "${W}" "${A}" "${B}")" "两个窗口各开一个项目：${W}"

echo "== 两个窗口同时连按「放大字号」各 5 下：一下都不能丢，文件和内存要一致（改和写在同一把锁里）"
step5='for (let i = 0; i < 5; i++) await window.__TAURI_INTERNALS__.invoke("step_font", { delta: 1 }); return true'
lite_eval main "${step5}" >/dev/null & lite_eval w-1 "${step5}" >/dev/null & wait; note_front
lite_wait main "return __lite.settings.v.editorFontSize === 23" 5
lite_wait w-1 "return __lite.settings.v.editorFontSize === 23" 5
m=$(lite_eval main "return __lite.settings.v.editorFontSize"); b=$(lite_eval w-1 "return __lite.settings.v.editorFontSize")
f=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["editor.fontSizeOffset"])' "${LITE_DATA}/ui-state.json" 2>/dev/null)
ok "$([ "${m}" = 23 ] && [ "${b}" = 23 ] && echo 1)" "两个窗口都是 23px（13 + 10）：main=${m} w-1=${b}"
ok "$([ "${f}" = 10 ] && echo 1)" "ui-state.json 里也是 +10（后写的没有被先改的那份盖掉）：${f}"
ok "$([ "$(lite_eval w-1 "return __lite.css('--editor-font-size')")" = 23px ] && echo 1)" "CSS 变量跟着换了（编辑器读它）"

echo "== 在编辑器里打字、存盘：盘上那份变了"
lite_eval main "__lite.tabs.show(__lite.tabs.byPath('${A}/a.txt').id); await new Promise(r => setTimeout(r, 300)); __lite.type('HELLO '); return __lite.text()" >/dev/null
lite menu save main >/dev/null
sleep 1; note_front
ok "$(grep -q 'HELLO alpha' "${A}/a.txt" && echo 1)" "a.txt 存进去了：$(head -1 "${A}/a.txt")"
ok "$([ "$(cat "${B}/b.txt")" = bravo ] && echo 1)" "b.txt 没被动（菜单只发给了 main）"

echo "== 改 settings.json：两个窗口都跟着变"
mkdir -p "${LITE_DATA}"; printf '{\n  // 验收\n  "editor.fontSize": 16,\n}\n' > "${LITE_DATA}/settings.json"
lite_wait main "return __lite.settings.v.editorFontSize === 26" 5; lite_wait w-1 "return __lite.settings.v.editorFontSize === 26" 5
ok "$([ "$(lite_eval w-1 "return __lite.settings.v.editorFontSize")" = 26 ] && echo 1)" "基础 16 + 偏移 10 = 26，两个窗口都换了"
note_front

echo "== 设置对话框（docs/SETTINGS.md 第 13 节）：⌘, 打开、改了写回 settings.json —— 你写的注释和原有的键都在；恢复默认删掉那一行；写坏了不动刀"
SD='const d = document.querySelector(".dialog.settings"); const cat = (n) => [...d.querySelectorAll(".cat")].find((c) => c.innerText.startsWith(n)); const rowOf = (k) => [...d.querySelectorAll(".row")].find((r) => r.querySelector(".key")?.textContent === k); const put = (el, v) => { el.value = v; el.dispatchEvent(new Event("input", { bubbles: true })); };'
lite menu open-settings main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.settings .row')" 10 >/dev/null
CATS=$(lite_eval main "${SD} return [...d.querySelectorAll('.cat')].map((c) => c.innerText.replace(/\n/g, ' ')).join('|')")
ok "$([ "${CATS}" = "外观|终端|Java 与 JDK|Maven|此项目 $(basename "${A}")" ] && echo 1)" "⌘, 开的是对话框，有「此项目」那一类：${CATS}"
lite menu toggle-sidebar main >/dev/null; sleep 0.2
ok "$([ "$(lite_eval main "return document.querySelector('.dialog.settings')?.classList.contains('nudge') ?? false")" = true ] && echo 1)" "对话框开着时点菜单：被挡下、对话框顶一下（真模态）"
lite_eval main "${SD} cat('Maven').click(); await new Promise((r) => setTimeout(r, 150)); put(rowOf('maven.extraArgs').querySelector('input'), '-T 2'); await new Promise((r) => setTimeout(r, 100)); d.querySelector('[data-default]').click(); return true" >/dev/null
lite_wait main "return !document.querySelector('.dialog.settings')" 5 >/dev/null
sleep 0.5
ok "$(grep -q '"maven.extraArgs": "-T 2",' "${LITE_DATA}/settings.json" && grep -q '// 验收' "${LITE_DATA}/settings.json" && grep -q '"editor.fontSize": 16,' "${LITE_DATA}/settings.json" && echo 1)" "写进去了，你写的注释和原有的键都在：$(tr '\n' ' ' < "${LITE_DATA}/settings.json")"
ok "$([ "$(lite_eval main "return __lite.settings.v.problems.length")" = 0 ] && echo 1)" "写回去的 settings.json 读回来没有问题（同一个校验函数）"
lite menu open-settings main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.settings .row')" 10 >/dev/null
lite_eval main "${SD} rowOf('editor.fontSize').querySelector('.reset').click(); await new Promise((r) => setTimeout(r, 100)); d.querySelector('[data-default]').click(); return true" >/dev/null
lite_wait main "return __lite.settings.v.editorFontSize === 23" 5 >/dev/null; lite_wait w-1 "return __lite.settings.v.editorFontSize === 23" 5 >/dev/null
ok "$(! grep -q 'editor.fontSize' "${LITE_DATA}/settings.json" && [ "$(lite_eval w-1 "return __lite.settings.v.editorFontSize")" = 23 ] && echo 1)" "恢复默认：那一行删了，两个窗口的字号回到 13 + 10：$(tr '\n' ' ' < "${LITE_DATA}/settings.json")"
lite menu open-settings main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.settings .row')" 10 >/dev/null
lite_eval main "${SD} cat('此项目').click(); await new Promise((r) => setTimeout(r, 150)); d.querySelector('label.check input').click(); await new Promise((r) => setTimeout(r, 100)); d.querySelector('[data-default]').click(); return true" >/dev/null
sleep 0.5
ok "$(python3 -c 'import json,sys; print(1 if json.load(open(sys.argv[1]))[sys.argv[2]]["offline"] else 0)' "${LITE_DATA}/maven-prefs.json" "${A}" 2>/dev/null)" "「此项目」里勾离线：记在这个项目名下（和 Maven 工具窗那个开关是同一个）"
printf '{\n  // 写坏了\n  "editor.fontSize": 16\n' > "${LITE_DATA}/settings.json"
lite_wait main "return __lite.settings.v.problems.some((p) => p.fatal)" 5 >/dev/null
BEFORE=$(shasum "${LITE_DATA}/settings.json")
lite menu open-settings main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.settings .row')" 10 >/dev/null
BAN=$(lite_eval main "${SD} return (d.querySelector('.banner')?.textContent ?? '').trim() + ' | ' + d.querySelector('.row input').disabled")
lite_eval main "document.querySelector('.dialog.settings [data-default]').click(); return true" >/dev/null
lite_wait main "return !document.querySelector('.dialog.settings')" 5 >/dev/null
ok "$(echo "${BAN}" | grep -q '先修好它.*| true$' && [ "$(shasum "${LITE_DATA}/settings.json")" = "${BEFORE}" ] && echo 1)" "settings.json 写坏了：说清楚、输入框灰掉，文件一个字节不动：${BAN}"
note_front

lite_quit
echo "== 焦点"
ok "$([ "${STOLEN}" = 0 ] && echo 1)" "起来之后测试应用一次都没跑到最前面（采样到它在最前 ${STOLEN} 次）—— 你可以照常用电脑"
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
