#!/usr/bin/env bash
# 本地历史（docs/IDEA.md 第 21 节）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。验的是只有真实现才有的那几样：盘上的原始字节（GBK）存下来、读回来按编码解得对；
# 外面改过之后再保存，补记一版「外部改动」；恢复是编辑器里的一笔修改（还没保存）；不往项目里写东西。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
P=$(cd "$(mktemp -d /tmp/acHist.XXXX)" && pwd -P)
trap 'lite_teardown; rm -rf "${P}"' EXIT

# 一份 GBK 的文件：「订单服务」+ CRLF（存的要是这些原始字节，读回来按 GBK 解）
python3 -c 'import sys; open(sys.argv[1], "wb").write("// 订单服务\r\nclass A {}\r\n".encode("gbk"))' "${P}/a.java"
BEFORE=$(cd "${P}" && find . -type f | sort | md5)
F="${P}/a.java"

lite_clean_data
lite_launch "${P}" || exit 1
lite_eval main "await __lite.tabflow.openPath('${F}', { preview: false }); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${F}' && __lite.tabs.active?.encoding === 'GBK'" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.encoding")" = GBK ] && echo 1)" "认出是 GBK"

save() { lite menu save main >/dev/null; lite_wait main "return __lite.tabs.byPath('${F}')?.dirty === false" 10 >/dev/null; }
edit() { lite_eval main "__lite.caret(2, 1); __lite.type('$1\\n'); return true" >/dev/null; }

echo "== 保存两次：第一次连改之前的样子一起记"
edit "// 第一次"; save
edit "// 第二次"; save
H=$(lite_eval main "return (await window.__TAURI_INTERNALS__.invoke('history_list', { path: '${F}' })).map((e) => e.kind).join(',')")
ok "$([ "${H}" = "saved,saved,before" ] && echo 1)" "三版，最新的在前：${H}"

echo "== 外面改过（像 git 切了分支），再在这儿保存：补记一版外部改动"
python3 -c 'import sys; open(sys.argv[1], "wb").write("// 外面改的\r\nclass A {}\r\n".encode("gbk"))' "${F}"
lite_wait main "return __lite.text()?.includes('外面改的') === true" 10 >/dev/null
edit "// 第三次"; save
H=$(lite_eval main "return (await window.__TAURI_INTERNALS__.invoke('history_list', { path: '${F}' })).map((e) => e.kind).join(',')")
ok "$([ "${H}" = "saved,before,saved,saved,before" ] && echo 1)" "外面改的那份夹在中间：${H}"

echo "== 读回来：存的是 GBK 的原始字节，按 GBK 解得出中文"
OLD=$(lite_eval main "const l = await window.__TAURI_INTERNALS__.invoke('history_list', { path: '${F}' }); return await window.__TAURI_INTERNALS__.invoke('history_read', { path: '${F}', hash: l[l.length - 1].hash, label: 'GBK' })")
ok "$(echo "${OLD}" | grep -q '订单服务' && echo 1)" "最早那版读回来是「// 订单服务」：$(echo "${OLD}" | head -c 40)"
DIR="${LITE_DATA}/history"
ok "$(python3 - "${DIR}" <<'PY'
import os, sys
want = "// 订单服务\r\nclass A {}\r\n".encode("gbk")
hit = any(open(os.path.join(r, f), "rb").read() == want for r, _, fs in os.walk(sys.argv[1]) for f in fs if f.endswith(".blob"))
print(1 if hit else 0)
PY
)" "盘上存的就是原来那几个字节（GBK + CRLF，没转码）"

echo "== 面板：菜单「本地历史…」打开，选最早那版，恢复"
lite menu local-history main >/dev/null
# 只找版本列表里的行：差异视图的行也叫 .row（第一版用 `.hist .row` 点「最后一行」，点到的是差异里的一行，选中一直是最新那版 —— 「恢复」了却一个字没变）
lite_wait main "return document.querySelectorAll('.hist .list .row').length === 5" 10 >/dev/null
ok "$([ "$(lite_eval main "return [...document.querySelectorAll('.hist .list .row .kind')].map((k) => k.textContent).join(',')")" = "已保存,外部修改,已保存,已保存,修改前" ] && echo 1)" "列表：$(lite_eval main "return [...document.querySelectorAll('.hist .list .row .kind')].map((k) => k.textContent).join(',')")"
lite_eval main "const r = document.querySelectorAll('.hist .list .row'); r[r.length - 1].click(); return true" >/dev/null
lite_wait main "return !!document.querySelector('.hist .diff .body')" 10 >/dev/null
lite_eval main "[...document.querySelectorAll('.hist .foot button')].find((b) => b.textContent.includes('恢复')).click(); return true" >/dev/null
lite_wait main "return !document.querySelector('.hist') && __lite.text() === '// 订单服务\\nclass A {}\\n'" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.text() === '// 订单服务\\nclass A {}\\n' && __lite.tabs.byPath('${F}')?.dirty === true")" = true ] && echo 1)" "编辑器里换成了最早那版、标签是脏的（还没保存）：$(lite_eval main "return JSON.stringify([__lite.text(), __lite.tabs.byPath('${F}')?.dirty, !!document.querySelector('.hist'), document.querySelector('.toast')?.textContent ?? ''])")"
ok "$(python3 -c 'import sys; print(1 if "第三次".encode("gbk") in open(sys.argv[1], "rb").read() else 0)' "${F}")" "盘上还是恢复之前那份（恢复不直接写盘）"

echo "== 没有副作用"
ok "$([ "$(cd "${P}" && find . -type f | sort | md5)" = "${BEFORE}" ] && echo 1)" "项目里一个文件都没多（历史在应用数据目录：${DIR/#$HOME/~}）"

lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
