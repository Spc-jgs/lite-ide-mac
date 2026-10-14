#!/usr/bin/env bash
# 合并日志（docs/MERGE.md）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。被测的是临时身份，碰不到你的 lite-ide 数据。
#
# 验的是**真 Rust 那一半**（浏览器桩里的合并是 JS 写的，验不到 logengine::merge）：顺序、堆栈不拆散、借日期、
# 过滤换行号、只看某几份、偏移重排、按时间跳、两份 10MB 的大文件、会话恢复、关标签收句柄
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
trap 'lite_teardown; rm -rf "${D:-}"' EXIT

STOLEN=0
note_front() { [ "$(lite_front_id)" = "${LITE_ID}" ] && STOLEN=$((STOLEN + 1)); return 0; }

lite_clean_data
D=$(mktemp -d /tmp/acM.XXXX); D=$(cd "${D}" && pwd -P)

# 同一次下单在三个服务里的日志：网关（Java）、订单服务（Spring 横幅 + Java 堆栈）、库存（Python，只有时分秒）
cat > "${D}/gateway.log" <<'EOF'
2026-08-24 10:15:02.101 INFO  [reactor-http-nio-3] c.d.gw.AccessLog - → POST /api/orders traceId=7f3a9c userId=1024
2026-08-24 10:15:02.388 WARN  [reactor-http-nio-3] c.d.gw.Upstream - order-service 慢响应 287ms traceId=7f3a9c
2026-08-24 10:15:02.902 ERROR [reactor-http-nio-3] c.d.gw.AccessLog - ← 500 POST /api/orders 801ms traceId=7f3a9c
2026-08-24 10:15:03.010 INFO  [reactor-http-nio-5] c.d.gw.AccessLog - → GET /api/orders/88231 traceId=b21e04
2026-08-24 10:15:03.044 INFO  [reactor-http-nio-5] c.d.gw.AccessLog - ← 200 GET /api/orders/88231 34ms traceId=b21e04
EOF
cat > "${D}/order-service.log" <<'EOF'
  .   ____          _            __ _ _
 :: Spring Boot ::                (v3.3.2)
2026-08-24 10:14:58.000 INFO  [main] c.d.order.Application - Started OrderApplication in 3.2 seconds
2026-08-24 10:15:02.130 INFO  [http-nio-8080-exec-2] c.d.order.OrderController - 下单 userId=1024 sku=A-77 qty=2 traceId=7f3a9c
2026-08-24 10:15:02.155 DEBUG [http-nio-8080-exec-2] c.d.order.OrderService - 预占库存 → inventory sku=A-77 traceId=7f3a9c
2026-08-24 10:15:02.884 ERROR [http-nio-8080-exec-2] c.d.order.OrderService - 下单失败 orderId=88232 traceId=7f3a9c
com.demo.order.InventoryException: 预占库存失败: 库存行被锁 sku=A-77
	at com.demo.order.InventoryClient.reserve(InventoryClient.java:58)
	at com.demo.order.OrderService.place(OrderService.java:142)
	at com.demo.order.OrderController.create(OrderController.java:37)
2026-08-24 10:15:03.020 INFO  [http-nio-8080-exec-4] c.d.order.OrderController - 查询 orderId=88231 traceId=b21e04
EOF
cat > "${D}/inventory.log" <<'EOF'
10:15:02.170 INFO  inventory.api: reserve sku=A-77 qty=2 trace=7f3a9c
10:15:02.671 WARN  inventory.db: 行锁等待 sku=A-77 waited=500ms trace=7f3a9c
10:15:02.860 ERROR inventory.db: lock wait timeout sku=A-77 trace=7f3a9c
Traceback (most recent call last):
  File "/srv/inventory/db.py", line 88, in reserve
    cur.execute(SQL_RESERVE, (sku, qty))
psycopg2.errors.LockNotAvailable: could not obtain lock on row in relation "stock"
10:15:03.500 INFO  inventory.api: healthz ok
EOF
# 两份各约 10MB、时间严格交错（a 偶数毫秒、b 奇数毫秒）：验跨块、跨检查点、首屏不等合完
python3 - "${D}" <<'PY'
import sys
d = sys.argv[1]
for name, odd in (("big-a.log", 0), ("big-b.log", 1)):
    with open(f"{d}/{name}", "w") as f:
        for i in range(110_000):
            ms = i * 2 + odd
            f.write(f"2026-08-24 10:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}.{ms % 1000:03} INFO  [t] c.x.Svc - {name[4]}{i} " + "p" * 60 + "\n")
PY

# 页面里取合并结果的几样小工具：直接调命令（和 LogPane 走的同一条 IPC），线格式照 block.ts 解
JS_LIB='
const inv = window.__TAURI_INTERNALS__.invoke;
const dec = (buf) => { const v = new DataView(buf), n = v.getUint32(8, true), b = new Uint8Array(buf), td = new TextDecoder(); let p = 12 + n * 4; const out = [];
  for (let i = 0; i < n; i++) { const raw = v.getUint32(12 + i * 4, true), len = raw & 0x7fffffff; out.push(td.decode(b.subarray(p, p + len))); p += len + (raw & 0x80000000 ? 8 : 0); } return out; };
const tab = () => __lite.tabs.list.find((t) => t.merge && t.merge.sources.length === MERGE_N);
const done = async (h) => { for (let i = 0; i < 200; i++) { const s = await inv("log_stat", { handle: h }); if (s.complete) return s; await new Promise((r) => setTimeout(r, 25)); } return null; };
const filt = async (h, pattern, sources) => { await inv("log_filter", { handle: h, levelBits: 63, pattern, caseSensitive: false, collapseStacks: false, label: "UTF-8", sources });
  for (let i = 0; i < 200; i++) { const s = await inv("log_filter_stat", { handle: h }); if (s && s.complete) return inv("log_filter_map", { handle: h, start: 0, count: 1000 }); await new Promise((r) => setTimeout(r, 25)); } return null; };
'
ev() { lite_eval main "const MERGE_N = ${2:-3}; ${JS_LIB} $1"; }
MP3="merge:\n${D}/gateway.log\n${D}/order-service.log\n${D}/inventory.log"

echo "== 起测试应用（后台），打开三份的合并"
lite_launch "${D}" || exit 1; note_front
lite_eval main "await __lite.tabflow.openPath('${MP3}'); return true" >/dev/null
lite_wait main "return !!__lite.tabs.list.find((t) => t.merge)" 10
H=$(ev "return tab().handle")
S=$(ev "const s = await done(tab().handle); return JSON.stringify([s.lineCount, s.complete])")
ok "$([ "${S}" = "[24,true]" ] && echo 1)" "合完 24 行（5 + 11 + 8）：${S}"

echo "== 顺序：来源序列（G 网关 / O 订单 / I 库存）"
SRC=$(ev "const r = await inv('merge_rows', { handle: tab().handle, start: 0, count: 100, filtered: false }); return r.src.map((s) => 'GOI'[s]).join('')")
ok "$([ "${SRC}" = "OOOGOOIGIIIIIIOOOOOGGOGI" ] && echo 1)" "OOOGOOIGIIIIIIOOOOOGGOGI：${SRC}"
LN=$(ev "const r = dec(await inv('log_lines', { handle: tab().handle, start: 14, count: 5 })); return JSON.stringify(r.map((x) => x.slice(0, 14)))")
ok "$(python3 -c "import json,sys; r=json.loads(sys.argv[1]); print(1 if r[0].startswith('2026-08-24 10:') and r[1].startswith('com.demo') and r[4].startswith('\tat com.demo.o') else 0)" "${LN}")" "订单服务的报错后面紧跟着它的堆栈（四行没被别的服务插进来）"
L3=$(ev "const r = await inv('merge_rows', { handle: tab().handle, start: 6, count: 1, filtered: false }); return JSON.stringify([r.src[0], r.line[0]])")
ok "$([ "${L3}" = "[2,0]" ] && echo 1)" "第 7 行来自库存、是那份的第 1 行（行号栏显示原文件的行号）：${L3}"
INFO=$(ev "const m = tab().merge; return JSON.stringify([m.fillDate, m.sources.map((s) => s.clock)])")
ok "$([ "${INFO}" = '["2026-08-24",["full","full","timeOnly"]]' ] && echo 1)" "库存那份只有时分秒、借 2026-08-24（图例要说出来）：${INFO}"

echo "== 过滤：按 traceId 跨三个服务；只看库存"
F=$(ev "const r = await filt(tab().handle, '7f3a9c'); return JSON.stringify(r)")
ok "$([ "${F}" = "[3,4,5,6,7,8,9,14,19]" ] && echo 1)" "traceId=7f3a9c 九条、按合并后的顺序：${F}"
F2=$(ev "const r = await filt(tab().handle, '', 4); return r.length")
ok "$([ "${F2}" = 8 ] && echo 1)" "只看第 3 份（库存）：8 行，堆栈和 Traceback 都在：${F2}"
lite_eval main "await window.__TAURI_INTERNALS__.invoke('log_filter', { handle: ${H}, levelBits: 63, pattern: '', caseSensitive: false, collapseStacks: false, label: 'UTF-8' }); return true" >/dev/null

echo "== 按时间跳（合并后的时间线）"
SK=$(ev "return await inv('log_seek_time', { handle: tab().handle, query: '10:15:02.900', near: 0 })")
ok "$([ "${SK}" = 19 ] && echo 1)" "10:15:02.900 → 网关回 500 那行（第 20 行）：${SK}"

echo "== 偏移：库存的钟慢 1 秒，给它 +1s 重排"
ev "await inv('merge_set_offsets', { handle: tab().handle, offsets: [0, 0, 1000] }); return true" >/dev/null
SRC2=$(ev "await done(tab().handle); const r = await inv('merge_rows', { handle: tab().handle, start: 0, count: 100, filtered: false }); return r.src.map((s) => 'GOI'[s]).join('')")
ok "$([ "${SRC2}" = "OOOGOOGOOOOOGGOGIIIIIIII" ] && echo 1)" "库存的八行整体挪到 10:15:03.044 之后：${SRC2}"

echo "== 界面：图例、来源格"
lite_eval main "__lite.tabs.show(__lite.tabs.list.find((t) => t.merge).id); return true" >/dev/null
lite_wait main "return __lite.has('合并 3 份')" 5
ok "$([ "$(lite_eval main "return __lite.has('合并 3 份')")" = true ] && echo 1)" "图例条在（合并 3 份）"
TAGS=$(lite_eval main "return document.querySelectorAll('.row .src').length")
ok "$([ "${TAGS:-0}" -gt 10 ] 2>/dev/null && echo 1)" "每行行首有来源格：${TAGS} 个"

echo "== 两份各 10MB：首屏不等合完，合完行数、交错、随机几行都对"
MPB="merge:\n${D}/big-a.log\n${D}/big-b.log"
lite_eval main "await __lite.tabflow.openPath('${MPB}'); return true" >/dev/null
lite_wait main "return !!__lite.tabs.list.find((t) => t.merge && t.merge.sources.length === 2)" 10
B=$(ev "const h = tab().handle; const t0 = performance.now(); const s = await done(h); const ms = Math.round(performance.now() - t0);
  const picks = [0, 1, 1023, 1024, 65535, 65536, 65537, 219999]; const out = [];
  for (const p of picks) out.push(dec(await inv('log_lines', { handle: h, start: p, count: 1 }))[0].split(' - ')[1].split(' ')[0]);
  return JSON.stringify([s.lineCount, out, ms])" 2)
ok "$(python3 -c "
import json,sys; n,out,ms=json.loads(sys.argv[1])
want=[('a' if p%2==0 else 'b')+str(p//2) for p in [0,1,1023,1024,65535,65536,65537,219999]]
print(1 if n==220000 and out==want else 0)" "${B}")" "22 万行、严格交错、跨块的那几行（65535 / 65536）都对：${B}"

echo "== 会话恢复：退出再开，合并标签回来（偏移不跨重启，v1）"
lite_quit; note_front
lite_launch || exit 1; note_front
lite_wait main "return __lite.tabs.list.filter((t) => t.merge).length === 2" 15
R=$(lite_eval main "return __lite.tabs.list.filter((t) => t.merge).map((t) => t.merge.sources.length).sort().join(',')")
ok "$([ "${R}" = "2,3" ] && echo 1)" "两个合并标签都回来了（3 份、2 份）：${R}"

echo "== 关掉标签：Rust 那边的句柄跟着收"
H3=$(ev "return tab().handle")
lite_eval main "__lite.tabflow.doClose(__lite.tabs.list.find((t) => t.merge && t.merge.sources.length === 3)); return true" >/dev/null
sleep 0.5
GONE=$(lite_eval main "try { await window.__TAURI_INTERNALS__.invoke('log_stat', { handle: ${H3} }); return 'still'; } catch (e) { return String(e); }")
ok "$([ "${GONE}" = "句柄已失效" ] && echo 1)" "关掉之后再问这个句柄：${GONE}"

lite_quit
echo "== 焦点"
ok "$([ "${STOLEN}" = 0 ] && echo 1)" "测试应用一次都没跑到最前面（采样到它在最前 ${STOLEN} 次）"
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
