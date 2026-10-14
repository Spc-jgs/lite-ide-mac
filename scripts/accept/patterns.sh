#!/usr/bin/env bash
# 日志归类（docs/PATTERNS.md）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。验的是真 Rust 那一半（桩里的归类是 JS 的简化版，不做 Drain 的「过半合并」）
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
trap 'lite_teardown; rm -rf "${D:-}"' EXIT
STOLEN=0
note_front() { [ "$(lite_front_id)" = "${LITE_ID}" ] && STOLEN=$((STOLEN + 1)); return 0; }

lite_clean_data
D=$(mktemp -d /tmp/acP.XXXX); D=$(cd "${D}" && pwd -P)
# 四种记录，各若干条；每第 50 条 ERROR 带一段堆栈（异常行 + 三个帧）—— 堆栈不该自成一类。线程号、id、耗时都在变
python3 - "${D}" <<'PY'
import sys, random
random.seed(7)
d = sys.argv[1]
kinds = [
    ("INFO ", "[http-nio-8080-exec-{t}] c.d.Order - 下单 userId={a} sku=A-{b} qty={c}"),
    ("DEBUG", "[pool-{t}-thread-{c}] c.d.Cache - evict key=order:{a} ttl={b}s"),
    ("WARN ", "[http-nio-8080-exec-{t}] c.d.Retry - 重试 attempt={c}/5 backing off {b}ms"),
    ("ERROR", "[scheduler-{t}] c.d.Order - 订单落库失败 orderId={a} cause=DeadlockLoserDataAccessException"),
]
counts = [0, 0, 0, 0]
with open(f"{d}/app.log", "w") as f, open(f"{d}/gw.log", "w") as g:
    for i in range(20000):
        k = random.choice([0, 0, 0, 1, 1, 2, 3])
        counts[k] += 1
        lvl, msg = kinds[k]
        ms = i * 7
        f.write(f"2026-08-24 10:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}.{ms % 1000:03} {lvl} " + msg.format(t=random.randint(1, 9), a=random.randint(1000, 99999), b=random.randint(1, 99), c=random.randint(1, 5)) + "\n")
        if k == 3 and counts[3] % 50 == 0:
            f.write("java.lang.IllegalStateException: pool exhausted\n\tat a.B(B.java:1)\n\tat a.C(C.java:2)\n\tat a.D(D.java:3)\n")
        g.write(f"2026-08-24 10:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}.{ms % 1000 + 1:03} INFO  gw - → GET /api/orders/{random.randint(1, 999)} {random.randint(1, 500)}ms\n")
print(*counts, file=open(f"{d}/counts", "w"))
# 「昨天」：前三种照旧（比例不同）、没有 ERROR、多一种今天没有的健康检查 —— 对比要说出 ERROR 新出现、健康检查没了
with open(f"{d}/app-yesterday.log", "w") as f:
    for i in range(10000):
        k = random.choice([0, 1, 2, 4])
        ms = i * 7
        ts = f"2026-08-23 10:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}.{ms % 1000:03}"
        if k == 4:
            f.write(f"{ts} INFO  [scheduler-1] c.d.Health - healthz ok upstream={random.randint(1, 3)}\n")
        else:
            lvl, msg = kinds[k]
            f.write(f"{ts} {lvl} " + msg.format(t=random.randint(1, 9), a=random.randint(1000, 99999), b=random.randint(1, 99), c=random.randint(1, 5)) + "\n")
PY
read -r C0 C1 C2 C3 < "${D}/counts"

lite_launch "${D}" || exit 1; note_front
lite_eval main "await __lite.tabflow.openPath('${D}/app.log'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.mode === 'log' && __lite.tabs.active.handle !== undefined" 10

echo "== 单份：归成四类，次数和造的时候一样，堆栈不自成一类"
P=$(lite_eval main "const inv = window.__TAURI_INTERNALS__.invoke, h = __lite.tabs.active.handle; let r;
  for (let i = 0; i < 200; i++) { r = await inv('log_patterns', { handle: h, limit: 50, restart: false }); if (r.done) break; await new Promise((x) => setTimeout(x, 25)); }
  const td = new TextDecoder(); return JSON.stringify([r.total, r.lines, r.overflow, r.patterns.map((p) => [p.count, td.decode(new Uint8Array(p.template)).split(' ').slice(2, 4).join(' ')])])")
ok "$(python3 -c "
import json,sys; t,l,o,ps=json.loads(sys.argv[1]); c=sorted(map(int,sys.argv[2:]),reverse=True)
print(1 if t==4 and l==sum(c) and o==0 and [p[0] for p in ps]==c else 0)" "${P}" "${C0}" "${C1}" "${C2}" "${C3}")" "四类、次数 ${C0}/${C1}/${C2}/${C3}、只数记录头：${P:0:220}"

echo "== 界面：点「归类」，点 ERROR 那一类 → 过滤出来的条数等于那一类的次数"
lite_eval main "__lite.click('归类'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.pattern-pop .row').length === 4" 10
ROWS=$(lite_eval main "return document.querySelectorAll('.pattern-pop .row').length")
ok "$([ "${ROWS}" = 4 ] && echo 1)" "浮层里四行：${ROWS}"
lite_eval main "[...document.querySelectorAll('.pattern-pop .row')].find((r) => r.querySelector('.dot.error')).click(); return true" >/dev/null
lite_wait main "return (document.querySelector('.pane .bar .hits')?.textContent ?? '').includes('${C3}')" 10
H=$(lite_eval main "return document.querySelector('.pane .bar .hits')?.textContent.trim() + ' / ' + document.querySelector('.pane .bar .search input').value")
ok "$(echo "${H}" | grep -q "^$(python3 -c "print(f'{int(\"${C3}\"):,}')") 条" && echo 1)" "筛出 ${C3} 条（模板的常量词当条件，这份数据上一条不多一条不少）：${H}"
ok "$([ "$(lite_eval main "return !document.querySelector('.pattern-pop')")" = true ] && echo 1)" "点完浮层关掉"

echo "== 对比：和「昨天」那份一起归 —— ERROR 新出现排第一、健康检查没了排第二（原生的选文件面板驱动不了，直接调命令；浮层的对比在浏览器里验过）"
CMP=$(lite_eval main "const inv = window.__TAURI_INTERNALS__.invoke, h = __lite.tabs.active.handle; let r;
  for (let i = 0; i < 200; i++) { r = await inv('log_patterns_compare', { handle: h, base: ['${D}/app-yesterday.log'], limit: 50, restart: false }); if (r.done) break; await new Promise((x) => setTimeout(x, 25)); }
  const td = new TextDecoder(); return JSON.stringify([r.lines, r.baseLines, r.patterns.map((p) => [p.base, p.count, td.decode(new Uint8Array(p.template)).split(' ')[2]])])")
ok "$(python3 -c "
import json,sys; l,bl,ps=json.loads(sys.argv[1])
print(1 if l==20000 and bl==10000 and len(ps)==5 and ps[0][0]==0 and ps[0][2]=='ERROR' and ps[1][1]==0 and ps[1][0]>0 else 0)" "${CMP}")" "两边 20000 / 10000 条，五类，第一是新出现的 ERROR、第二是没了的那类：${CMP:0:200}"

echo "== 合并视图：两份一起归（网关那份是第五类）"
lite_eval main "await __lite.tabflow.openPath('merge:\n${D}/app.log\n${D}/gw.log'); return true" >/dev/null
lite_wait main "return !!__lite.tabs.active?.merge" 10
M=$(lite_eval main "const inv = window.__TAURI_INTERNALS__.invoke, h = __lite.tabs.active.handle; let r;
  for (let i = 0; i < 200; i++) { r = await inv('log_patterns', { handle: h, limit: 50, restart: false }); if (r.done) break; await new Promise((x) => setTimeout(x, 25)); }
  return JSON.stringify([r.total, r.lines])")
ok "$([ "${M}" = "[5,40000]" ] && echo 1)" "五类、四万条记录：${M}"

echo "== 关掉标签：归类跟着收，句柄问不到了"
HH=$(lite_eval main "return __lite.tabs.active.handle")
lite_eval main "__lite.tabflow.doClose(__lite.tabs.active); return true" >/dev/null
sleep 0.5
G=$(lite_eval main "try { await window.__TAURI_INTERNALS__.invoke('log_patterns', { handle: ${HH}, limit: 1, restart: false }); return 'still'; } catch (e) { return String(e); }")
ok "$([ "${G}" = "句柄已失效" ] && echo 1)" "关掉之后：${G}"

lite_quit
ok "$([ "${STOLEN}" = 0 ] && echo 1)" "测试应用一次都没跑到最前面（采样到 ${STOLEN} 次）"
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
