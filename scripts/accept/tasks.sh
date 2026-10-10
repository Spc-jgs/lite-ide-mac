#!/usr/bin/env bash
# #48 通用任务运行的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。被测的是临时身份，碰不到你的 lite-ide 数据；
# 测试终端 / 任务的 zsh 用临时 ZDOTDIR（bridge.sh），不读你的 .zshrc、不写你的历史。
#
# 跑的任务是一个 Python 小服务（/usr/bin/python3，不靠你的 PATH）：每 0.5 秒 print 一行（**不 flush**，验 PYTHONUNBUFFERED）、
# 监听一个端口、收到 SIGINT 打一行「收尾」再退。验的是 docs/TASKS.md 第 7 节里桩上验不到的那几条：真进程组、端口空了、输出实时、
# 重跑不留两份、关窗口 / 退出应用不留孤儿。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
yes_if() { if eval "$1" >/dev/null 2>&1; then echo 1; else echo 0; fi; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

P=$(cd "$(mktemp -d /tmp/acT.XXXX)" && pwd -P)
freeport() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'; }
PORT=$(freeport)
PORT2=$(freeport)
MARK="t48srv-$$"
cleanup() { pkill -KILL -f "${MARK}" 2>/dev/null; [ -n "${SQUAT:-}" ] && kill -KILL "${SQUAT}" 2>/dev/null; lite_teardown; rm -rf "${P}"; }
trap cleanup EXIT

cat > "${P}/srv.py" <<EOF
import http.server, signal, socketserver, sys, threading, time
def bye(*_):
    # 先落一个文件再 print：应用崩过之后，它原来的输出管道已经断了，print 写不出去（会再撞一次 SIGPIPE）——
    # 「收尾跑了没」要一个不经管道的证据
    open("${P}/bye.txt", "a").write("收尾\n")
    print("T48-收尾", flush=True)
    sys.exit(0)
signal.signal(signal.SIGINT, bye)
port = int(sys.argv[2]) if len(sys.argv) > 2 else ${PORT}
try:
    s = socketserver.TCPServer(("127.0.0.1", port), http.server.SimpleHTTPRequestHandler)
except OSError:
    # 学 Spring Boot 的说法（tasksvc::port 认得它）。Python 自己的报错不带端口号，那种不猜
    print(f"Web server failed to start. Port {port} was already in use.", flush=True)
    sys.exit(1)
threading.Thread(target=s.serve_forever, daemon=True).start()
print("2026-10-10 09:00:00 ERROR demo 示范 ERROR 行")
i = 0
while True:
    # 只说前 8 句，然后安静：话多的进程在应用退出、管道断掉后会被 SIGPIPE 带走，「退出不留孤儿」那条就成了碰巧绿
    # （验红时发现的：退出流程的软停和退出时的强杀都去掉，它照样死了）。安静的服务（起来后不怎么打日志的 Spring）才是真考验
    if i < 8:
        print(f"tick {i}")
    i += 1
    time.sleep(0.5)
EOF
mkdir -p "${P}/.lite-ide"
# 命令行里带上 MARK：按它数进程（${MARK} 只是一个不影响运行的参数）
printf '[ { "name": "srv", "command": "/usr/bin/python3 srv.py %s" }, { "name": "busy", "command": "/usr/bin/python3 srv.py %s %s" } ]\n' \
  "${MARK}" "${MARK}" "${PORT2}" > "${P}/.lite-ide/tasks.json"

listening() { lsof -nP -iTCP:"${PORT}" -sTCP:LISTEN -t >/dev/null 2>&1; }
procs() { pgrep -f "${MARK}" | wc -l | tr -d ' '; }
dot() { lite_eval main "return document.querySelector('.ptab.on .dot')?.className.split(' ')[1] ?? ''" 2>/dev/null; }
logf() { find "${LITE_DATA}/runs" -name srv.log 2>/dev/null | head -1; }
# ⌃⌥R 开任务列表，点某一行（默认 srv）
pick_srv() {
  local name="${1:-srv}"
  lite menu run-pick main >/dev/null
  lite_wait main "return !!document.querySelector('.picker .row')" 8 >/dev/null
  lite_eval main "const r = [...document.querySelectorAll('.picker .row')].find((x) => x.querySelector('.name')?.innerText === '${name}'); r?.click(); return !!r" 2>/dev/null
}
listening2() { lsof -nP -iTCP:"${PORT2}" -sTCP:LISTEN -t >/dev/null 2>&1; }

lite_clean_data
lite_launch "${P}" >/dev/null || exit 1
lite_wait main "return __lite.project.root === '${P}'" 15

echo "== ⌃⌥R 开任务列表，tasks.json 里的 srv 在里面；点它跑起来"
ok "$( [ "$(pick_srv)" = true ] && echo 1)" "任务列表里有 srv，点了"
ok "$(wait_for 15 listening && echo 1)" "15 秒内服务监听上 ${PORT}（经登录 shell 起，真进程）"
ok "$( [ "$(dot)" = running ] && echo 1)" "运行窗那格是「在跑」：$(dot)"
L=$(logf)
ok "$( [ -n "${L}" ] && echo 1)" "输出落在应用数据目录里：${L#${HOME}/}"
n1=$(grep -c tick "${L}" 2>/dev/null); sleep 1.6; n2=$(grep -c tick "${L}" 2>/dev/null)
ok "$( [ "${n2:-0}" -ge $(( ${n1:-0} + 2 )) ] && echo 1)" "输出是实时的（没 flush 的 print，1.6 秒里多了 $(( ${n2:-0} - ${n1:-0} )) 行）—— PYTHONUNBUFFERED 生效了"

echo "== ⌃R 重跑：先停旧的（端口要先空出来）再起，不留两份"
lite_eval main "__lite.key('Ctrl-r'); return true" >/dev/null
sleep 1
ok "$(wait_for 15 '[ "$(procs)" = 1 ] && listening' && echo 1)" "重跑之后只有一个 srv 进程、端口照样监听（现在 $(procs) 个）"
ok "$(grep -q 'T48-收尾' "$(logf).1" 2>/dev/null && echo 1)" "上一次的输出挪成了 .1，里面有它收到 SIGINT 后的「收尾」—— 先礼后兵，不是直接杀"

echo "== ⌘F2 停止：整组停、端口空、记成「已停止」不是「失败」"
lite menu run-stop main >/dev/null
ok "$(wait_for 6 '! listening && [ "$(procs)" = 0 ]' && echo 1)" "6 秒内端口空了、srv 进程一个不剩"
ok "$(lite_wait main "return document.querySelector('.ptab.on .dot')?.classList.contains('stopped')" 5 && echo 1)" "那格是「已停止」：$(dot)"
ok "$(grep -q 'T48-收尾' "$(logf)" && echo 1)" "停之前它跑了自己的收尾（SIGINT 的处理函数）"

echo "== 关掉这一格：还在跑的话跟着软停"
pick_srv >/dev/null
ok "$(wait_for 15 listening && echo 1)" "又跑起来了"
lite_eval main "return __lite.click('关闭 srv')" >/dev/null
ok "$(wait_for 6 '! listening && [ "$(procs)" = 0 ]' && echo 1)" "关掉在跑的那格：进程跟着停了、端口空了"

echo "== 跑着任务退出应用：不留孤儿"
pick_srv >/dev/null
ok "$(wait_for 15 listening && echo 1)" "又跑起来了"
sleep 5   # 等它说完那 8 句、安静下来（理由见 srv.py 里那段）
lite_quit >/dev/null
ok "$(wait_for 6 '! listening && [ "$(procs)" = 0 ]' && echo 1)" "⌘Q 之后端口空了、srv 一个不剩（退出流程先软停、RunEvent::Exit 再兜底强杀）"
ok "$(grep -q 'T48-收尾' "$(logf)" && echo 1)" "退出时也给了它收尾的机会"

echo "== 端口被占（第 4 步）：别的程序占着 ${PORT2}，跑 busy → 卡片说出是谁；点「结束它并重跑」→ 它没了、busy 起来了"
lite_launch "${P}" >/dev/null || exit 1
lite_wait main "return __lite.project.root === '${P}'" 15
# 占端口的「别的程序」：脚本自己起的，和脚本同一个进程组、不是组长 —— 结束它时要**只动它自己**，动了整组脚本自己就没了
python3 -c "import socket, time; s = socket.socket(); s.bind(('127.0.0.1', ${PORT2})); s.listen(); time.sleep(300)" &
SQUAT=$!
wait_for 5 listening2
pick_srv busy >/dev/null
ok "$(lite_wait main "return (document.querySelector('.run-cards .confirm')?.innerText ?? '').includes('${PORT2}')" 15 && echo 1)" \
  "卡片出来了：$(lite_eval main "return document.querySelector('.run-cards .confirm span')?.innerText ?? ''" 2>/dev/null)"
ok "$(lite_eval main "return document.querySelector('.run-cards .btn.danger') ? 1 : 0" 2>/dev/null)" "占着的不是我们起的任务：「结束它并重跑」是 danger（红）"
lite_eval main "return __lite.click('结束它并重跑')" >/dev/null
ok "$(wait_for 10 '! kill -0 ${SQUAT}' && echo 1)" "占端口的那个进程没了（PID ${SQUAT}）"
ok "$(wait_for 15 'listening2 && [ "$(lsof -nP -iTCP:${PORT2} -sTCP:LISTEN -Fc | grep -c "^cPython")" -ge 1 ] && pgrep -f "${MARK} ${PORT2}" >/dev/null' && echo 1)" "busy 重跑起来、自己占上了 ${PORT2}"
ok 1 "脚本自己还活着（没被「整组结束」波及 —— 走到这一行就是证据）"
lite_eval main "return __lite.click('关闭并停止 busy') || __lite.click('关闭 busy')" >/dev/null
wait_for 6 '! listening2'

echo "== 应用崩了（kill -9），任务还在跑 → 重开出卡片 → 结束它们，端口空了"
pick_srv >/dev/null
ok "$(wait_for 15 listening && echo 1)" "srv 跑起来了"
# 等它说完那 8 句、安静下来再杀应用：还在说话的话，应用一死管道就断，它下一次 print 被 SIGPIPE 带走，留不下来 ——
# 真实情况也是这样：话多的任务跟着应用死，安静的（起来后不怎么打日志的 Spring）才会留下来占着端口，卡片就是为它们准备的
sleep 5
rm -f "${P}/bye.txt"
kill -9 "$(lite_pid)"
sleep 1
ok "$(listening && [ "$(procs)" = 1 ] && echo 1)" "应用没了，srv 还活着、还占着 ${PORT}（这就是要收的尸）"
lite_launch "${P}" >/dev/null || exit 1
lite_wait main "return __lite.project.root === '${P}'" 15
ok "$(lite_wait main "return !!document.querySelector('.confirm.stale')" 8 && echo 1)" \
  "重开出了卡片：$(lite_eval main "return document.querySelector('.confirm.stale .btext span')?.innerText ?? ''" 2>/dev/null)"
lite_eval main "return __lite.click('结束它们')" >/dev/null
ok "$(wait_for 8 '! listening && [ "$(procs)" = 0 ]' && echo 1)" "点了「结束它们」：端口空了、srv 一个不剩"
ok "$(grep -q '收尾' "${P}/bye.txt" 2>/dev/null && echo 1)" "结束时给了它收尾的机会（软停；证据是它收到 SIGINT 时落的文件，输出管道早就断了）"
lite_quit >/dev/null

printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
