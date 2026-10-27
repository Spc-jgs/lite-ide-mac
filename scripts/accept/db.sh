#!/usr/bin/env bash
# 数据库客户端 DB-1 / DB-2 / DB-3（docs/DATABASE.md）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app（要带内置的 Java 和 lite-db.jar：scripts/java-bundle.sh 准备的）。
#
# 用 H2 的内存库（不要数据库服务器）：连接串里 INIT=RUNSCRIPT 让 H2 连上时自己建表。
# 密码进的是脚本自己建的**临时钥匙串**（LITE_DB_KEYCHAIN），不碰你的登录钥匙串；`security create-keychain` 会把它加进搜索列表，建完当场把列表还原。
# 驱动：~/Library/Caches/lite-ide-build/ 里有 H2 的 jar 就当本机的 Maven 仓库用（LITE_DB_MAVEN，不上网），没有就让应用真的去 Maven Central 下
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

P=$(cd "$(mktemp -d /tmp/acDb.XXXX)" && pwd -P)
mkdir -p "${P}/src"; printf 'hello\n' > "${P}/src/a.txt"
KC="${LITE_WORK}/db-test.keychain-db"
ORIG=$(security list-keychains -d user | tr -d '"' | xargs)
security create-keychain -p acc "${KC}" && security list-keychains -d user -s ${ORIG} && security unlock-keychain -p acc "${KC}"
cleanup() {
  lite_teardown keep
  security delete-keychain "${KC}" 2>/dev/null
  security list-keychains -d user -s ${ORIG}
  rm -rf "${P}" "${LITE_WORK}"
}
trap cleanup EXIT
export LITE_DB_KEYCHAIN="${KC}"
H2C="${HOME}/Library/Caches/lite-ide-build/h2-2.3.232.jar"
if [ -f "${H2C}" ]; then
  mkdir -p "${LITE_WORK}/m2/com/h2database/h2/2.3.232"
  cp "${H2C}" "${LITE_WORK}/m2/com/h2database/h2/2.3.232/h2-2.3.232.jar"
  export LITE_DB_MAVEN="file://${LITE_WORK}/m2"
fi
INIT="${LITE_WORK}/init.sql"
# INIT 每次连都跑（内存库 DB_CLOSE_DELAY=-1 一直在）：测试连接建过一次、点开树又连一次 —— 写成 if not exists
printf "create schema if not exists shop;\ncreate table if not exists shop.orders(id bigint primary key, amount decimal(12,2) not null, note varchar(200));\ncomment on table shop.orders is '订单';\n" > "${INIT}"
URL="jdbc:h2:mem:acc;DB_CLOSE_DELAY=-1;INIT=RUNSCRIPT FROM '${INIT}'"
# 导出时的保存面板：测试构建里 LITE_SAVE_PATH 就是「选好的路径」（原生面板是真窗口，测试通道点不了）
export LITE_SAVE_PATH="${LITE_WORK}/导出/big.csv"
mkdir -p "${LITE_WORK}/导出"
BEFORE=$(cd "${P}" && find . -type f | sort | md5)

lite_clean_data
lite_launch "${P}" || exit 1
inv() { lite_eval main "const r = await window.__TAURI_INTERNALS__.invoke($1); return JSON.stringify(r)"; }
CONN="{ id: '', name: '验收 H2', kind: 'h2', host: '', port: 0, database: '', user: 'sa', url: $(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "${URL}"), hasPassword: false }"

echo "== 测试连接：第一次连 H2 先下驱动、核 sha256；用户名密码不对说清楚"
T=$(inv "'db_test', { conn: ${CONN}, password: 's3cret' }")
ok "$(echo "${T}" | grep -q '"product":"H2"' && echo 1)" "连上了：${T}"
ok "$([ "$(shasum -a 256 "${LITE_DATA}/jdbc/h2-2.3.232.jar" 2>/dev/null | cut -d' ' -f1)" = 8dae62d22db8982c3dcb3826edb9c727c5d302063a67eef7d63d82de401f07d3 ] && echo 1)" "驱动在应用数据目录的 jdbc/ 底下、sha256 对得上$([ -n "${LITE_DB_MAVEN:-}" ] && echo '（这次从本机缓存拿的）' || echo '（这次从 Maven Central 下的）')"

echo "== 保存：配置不带密码、密码进钥匙串"
S=$(inv "'db_save', { root: '${P}', conn: ${CONN}, password: 's3cret' }")
ID=$(echo "${S}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' 2>/dev/null)
ok "$([ -n "${ID}" ] && echo "${S}" | grep -q '"hasPassword":true' && echo 1)" "存好了、说钥匙串里有密码：${S}"
ok "$([ "$(security find-generic-password -s com.liteide.mwtest.db -a "${ID}" -w "${KC}" 2>/dev/null)" = s3cret ] && echo 1)" "钥匙串里那一条就是这个密码（测试钥匙串，服务名带测试身份）"
ok "$(! grep -rq s3cret "${LITE_DATA}" && echo 1)" "应用数据目录里哪个文件都没有这个密码"
W=$(inv "'db_test', { conn: { ...${CONN}, id: '${ID}' }, password: 'nope' }" 2>&1)
ok "$(echo "${W}" | grep -q '28000' && echo 1)" "密码不对：报 H2 的原话（带 SQLState）：$(echo "${W}" | cut -c1-160)"

echo "== 表结构树：工具窗里点开连接 → 库 → 表 → 列，每一层展开时才问"
lite_eval main "document.querySelector('nav.rail button[aria-label=\"数据库\"]').click(); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('.db .row')].some((r) => r.textContent.includes('验收 H2'))" 10 >/dev/null
ROW="const rows = () => [...document.querySelectorAll('.db .row')]; const click = (t) => rows().find((r) => r.querySelector('.name')?.textContent === t).click();"
lite_eval main "${ROW} click('验收 H2'); return true" >/dev/null
lite_wait main "${ROW} return rows().some((r) => r.querySelector('.name')?.textContent === 'SHOP')" 30 >/dev/null
lite_eval main "${ROW} click('SHOP'); return true" >/dev/null
lite_wait main "${ROW} return rows().some((r) => r.querySelector('.name')?.textContent === 'ORDERS')" 15 >/dev/null
lite_eval main "${ROW} click('ORDERS'); return true" >/dev/null
lite_wait main "${ROW} return rows().some((r) => r.querySelector('.name')?.textContent === 'AMOUNT')" 15 >/dev/null
TREE=$(lite_eval main "return [...document.querySelectorAll('.db .row')].map((r) => r.textContent.replace(/\\s+/g, ' ').trim()).join(' | ')")
ok "$(echo "${TREE}" | grep -q 'ORDERS 订单' && echo "${TREE}" | grep -q 'ID BIGINT · 主键' && echo "${TREE}" | grep -q 'AMOUNT DECIMAL(12) · 非空' && echo 1)" "库 → 表（带注释）→ 列（类型、主键、非空）：${TREE}"
ok "$([ "$(pgrep -f 'lite-db.jar' | wc -l | tr -d ' ')" = 1 ] && echo 1)" "数据库服务进程只有一个（边车）"

echo "== 查询控制台（DB-2）：每个连接一份 .sql，在应用数据目录里"
lite_eval main "${ROW} click('验收 H2'); return true" >/dev/null
lite_eval main "document.querySelector('.db button[aria-label=\"打开查询控制台\"]').click(); return true" >/dev/null
lite_wait main "return !!document.querySelector('.cm-sql-bar')" 15 >/dev/null
CF=$(ls "${LITE_DATA}/db/console/${ID}/"*.sql 2>/dev/null | head -1)
ok "$([ "$(basename "${CF:-无}")" = "验收 H2.sql" ] && echo 1)" "控制台文件：应用数据目录 db/console/<连接>/$(basename "${CF:-无}")"
ok "$(lite_eval main "return document.querySelector('.cm-sql-bar .who')?.textContent" | grep -q '验收 H2' && echo 1)" "控制台顶上写着连的是哪个库"
# 花括号一律放在单引号的变量里（AGENTS.md：bash 3.2 在 "$(…)" 里照样做花括号展开）
CONSOLE_TEXT=$(cat <<'EOF'
create table if not exists big as select x as id, 'n-' || x as name, cast(x * 1.5 as decimal(20, 2)) as amt, case when mod(x, 7) = 0 then null else x end as maybe from system_range(1, 100000);

select * from big order by id;

select count(*) from system_range(1, 100000) a, system_range(1, 100000) b where a.x + b.x = 7;

select 1 as one;
EOF
)
lite_eval main "__lite.setText($(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "${CONSOLE_TEXT}")); return true" >/dev/null
Q='const q = await __lite.dbResults(); const a = q.active;'
run_line() { lite_eval main "__lite.caret($1, 1); __lite.key('Mod-Enter'); return true" >/dev/null; }
run_line 1
lite_wait main "${Q} return !!a && a.sql.startsWith('create') && a.status !== 'running'" 60 >/dev/null
ok "$([ "$(lite_eval main "${Q} return a.status")" = done ] && echo 1)" "⌘↵ 执行光标所在的那条（建十万行的表）：$(lite_eval main "${Q} return a.status + ' · ' + a.ms + ' ms · ' + (a.error || '')")"

echo "== 结果表格：十万行，首屏、按需取、NULL 和 DECIMAL"
FIRST='const t0 = performance.now(); __lite.caret(3, 1); __lite.key("Mod-Enter");
const ready = () => !!document.querySelector(".dbres .row") && (document.querySelector(".panel-head .ptab.on")?.textContent ?? "").includes("select * from big");
await new Promise((res) => { if (ready()) return res(); const mo = new MutationObserver(() => { if (ready()) { mo.disconnect(); res(); } }); mo.observe(document.body, { childList: true, subtree: true }); });
return Math.round(performance.now() - t0)'
FS=$(lite_eval main "${FIRST}")
ok "$([ -n "${FS}" ] && [ "${FS}" -lt 300 ] 2>/dev/null && echo 1)" "⌘↵ 到表格画出第一屏：${FS} ms（线是 300 ms）"
DOMN=$(lite_eval main "return document.querySelectorAll('.dbres .row').length")
ok "$([ "${DOMN}" -gt 0 ] && [ "${DOMN}" -lt 120 ] && echo 1)" "页面里只画看得见的那几十行：${DOMN} 个行元素（取到 $(lite_eval main "${Q} return a.rows.length") 行）"
CELLS='const r = [...document.querySelectorAll(".dbres .row")].find((x) => x.querySelector(".gut")?.textContent === "7"); return r ? [...r.querySelectorAll(".cell")].map((c) => c.textContent + (c.classList.contains("null") ? "(null)" : "")).join("|") : "没有第 7 行"'
C7=$(lite_eval main "${CELLS}")
ok "$([ "${C7}" = "7|n-7|10.50|<null>(null)" ] && echo 1)" "第 7 行：${C7}（DECIMAL 原样两位小数、NULL 和空串分得开）"
SCROLL='const g = document.querySelector(".dbres .grid"); g.scrollTop = g.scrollHeight; g.dispatchEvent(new Event("scroll")); return true'
for i in 1 2 3 4; do lite_eval main "${SCROLL}" >/dev/null; sleep 0.4; done
N1=$(lite_eval main "${Q} return a.rows.length")
ok "$([ "${N1}" -ge 2000 ] && echo 1)" "滚到底就取下一批：滚四次之后取到 ${N1} 行"
DRAIN='const q = await __lite.dbResults(); const a = q.active; const t0 = performance.now(); let n = 0; while (a.more && n < 400) { await q.fetchMore(a); n++; }
return a.rows.length + " " + n + " " + Math.round(performance.now() - t0) + " " + a.rows.filter((r) => r[3] === null).length + " " + a.more'
D=$(lite_eval main "${DRAIN}")
read -r DN DB DMS DNULL DMORE <<< "${D}"
ok "$([ "${DN}" = 100000 ] && [ "${DMORE}" = false ] && [ "${DNULL}" = 14285 ] && echo 1)" "取完：${DN} 行（再取 ${DB} 批、${DMS} ms），NULL ${DNULL} 个（每七行一个）"
STAT=$(lite_eval main "return document.querySelector('.dbres .stat')?.textContent")
ok "$(echo "${STAT}" | grep -q '共 100,000 行' && echo 1)" "工具条：${STAT}"
RSS=$(ps -o rss= -p "$(pgrep -f lite-db.jar | head -1)" 2>/dev/null | tr -d ' ')
ok "$([ -n "${RSS}" ] && [ "${RSS}" -lt 409600 ] && echo 1)" "取完十万行之后边车常驻 $((${RSS:-0} / 1024)) MB（H2 内存库的数据也在它的堆里；线 400 MB）"

echo "== 取消：慢查询点取消 1 秒内停，连接照常能用"
run_line 5
lite_wait main "${Q} return !!a && a.sql.startsWith('select count') && a.status === 'running'" 10 >/dev/null
sleep 1
T0=$(python3 -c 'import time; print(time.time())')
lite_eval main "document.querySelector('.dbres button[aria-label=\"取消执行\"]').click(); return true" >/dev/null
for i in $(seq 1 100); do [ "$(lite_eval main "${Q} return a.status")" != running ] && break; done
T1=$(python3 -c 'import time; print(time.time())')
CMS=$(python3 -c "print(int((${T1} - ${T0}) * 1000))")
ok "$([ "$(lite_eval main "${Q} return a.status")" = cancelled ] && [ "${CMS}" -lt 1000 ] && echo 1)" "点取消到停下：${CMS} ms（含测试通道往返），结果标成已取消：$(lite_eval main "${Q} return a.error.slice(0, 80)")"
run_line 7
lite_wait main "${Q} return !!a && a.sql.startsWith('select 1') && a.status !== 'running'" 15 >/dev/null
ok "$([ "$(lite_eval main "${Q} return a.status + ':' + JSON.stringify(a.rows)")" = 'done:[["1"]]' ] && echo 1)" "取消之后同一个连接照常能用：select 1 → $(lite_eval main "${Q} return JSON.stringify(a.rows)")"

echo "== 历史：每条都记，失败、取消也记；不进项目"
H=$(inv "'db_history', { id: '${ID}' }")
ok "$(echo "${H}" | python3 -c 'import json,sys; h=json.load(sys.stdin); s=[x["sql"][:12] for x in h]; print(1 if len(h)>=4 and s[0].startswith("select 1") and any(not x["ok"] and x["sql"].startswith("select count") for x in h) else 0)')" "历史 $(echo "${H}" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))') 条，最近的在前、取消的那条记成失败"
ok "$([ -f "${LITE_DATA}/db/history/${ID}.jsonl" ] && ! grep -q s3cret "${LITE_DATA}/db/history/${ID}.jsonl" && echo 1)" "历史在应用数据目录 db/history/<连接>.jsonl，里面没有密码"

echo "== 执行前确认（DB-3）：生产库改数据先问、说清动几行；答「取消」一行都不动"
inv "'db_save', { root: '${P}', conn: { ...${CONN}, id: '${ID}', prod: true }, password: null }" >/dev/null
GUARD_TEXT=$(cat <<'EOF'
delete from big where id <= 10;

update big set name = 'x';

select * from big order by id;
EOF
)
lite_eval main "__lite.setText($(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "${GUARD_TEXT}")); return true" >/dev/null
COUNT_BIG='const q = await __lite.dbResults(); await q.run(q.active.root, q.active.conn, "x", ["select count(*) from big"]); return q.active.rows[0][0]'
run_line 1
lite_wait main "return !!document.querySelector('.dbconfirm') && !document.querySelector('.dbconfirm .spinner')" 10 >/dev/null
DLG=$(lite_eval main "return document.querySelector('.dbconfirm .title').textContent + ' | ' + [...document.querySelectorAll('.dbconfirm .what')].map((e) => e.textContent.trim()).join(' / ') + ' | 默认：' + document.activeElement?.textContent")
ok "$(echo "${DLG}" | grep -q '在生产库「验收 H2」上执行？' && echo "${DLG}" | grep -q '将删除 big 中的 10 行' && echo "${DLG}" | grep -q '默认：取消' && echo 1)" "生产库上 DELETE 先问、数出来的行数对、默认按钮是取消：${DLG}"
lite_eval main "[...document.querySelectorAll('.dbconfirm button')].find((b) => b.textContent === '取消').click(); return true" >/dev/null
sleep 0.5
ok "$([ "$(lite_eval main "${COUNT_BIG}")" = 100000 ] && echo 1)" "答「取消」：一行都没删（还是 100000 行）"
run_line 1
lite_wait main "return !!document.querySelector('.dbconfirm') && !document.querySelector('.dbconfirm .spinner')" 10 >/dev/null
lite_eval main "[...document.querySelectorAll('.dbconfirm button')].find((b) => b.textContent === '执行').click(); return true" >/dev/null
lite_wait main "${Q} return !!a && a.sql.startsWith('delete') && a.status !== 'running'" 15 >/dev/null
ok "$([ "$(lite_eval main "${Q} return a.status + ':' + a.count")" = done:10 ] && [ "$(lite_eval main "${COUNT_BIG}")" = 99990 ] && echo 1)" "答「执行」：删了 10 行，和确认框里数的一样（剩 99990）"
inv "'db_save', { root: '${P}', conn: { ...${CONN}, id: '${ID}', prod: false }, password: null }" >/dev/null
run_line 3
lite_wait main "return !!document.querySelector('.dbconfirm') && !document.querySelector('.dbconfirm .spinner')" 10 >/dev/null
DLG2=$(lite_eval main "return (document.querySelector('.dbconfirm .title')?.textContent ?? '没有对话框') + ' | ' + [...document.querySelectorAll('.dbconfirm .what')].map((e) => e.textContent.trim()).join(' / ')")
ok "$(echo "${DLG2}" | grep -q '执行不带 WHERE 条件的语句？' && echo "${DLG2}" | grep -q '全部 99,990 行' && echo 1)" "不是生产库：不带 WHERE 的 UPDATE 照样先问：${DLG2}"
lite_eval main "[...document.querySelectorAll('.dbconfirm button')].find((b) => b.textContent === '取消').click(); return true" >/dev/null
sleep 0.3

echo "== 导出（DB-3）：整条查询的全部结果、边车直接写盘；取消了原文件不动"
run_line 5
lite_wait main "${Q} return !!a && a.sql.startsWith('select * from big') && a.status === 'done'" 15 >/dev/null
lite_eval main "[...document.querySelectorAll('.dbres .bar button')].find((b) => b.textContent.includes('导出')).click(); return true" >/dev/null
lite_wait main "return !!document.querySelector('[role=menuitem]')" 5 >/dev/null
lite_eval main "[...document.querySelectorAll('[role=menuitem]')].find((x) => x.textContent.includes('CSV')).click(); return true" >/dev/null
OUT="${LITE_SAVE_PATH}"
wait_for 30 '[ -f "${OUT}" ]'
lite_wait main "return __lite.notify.log.some((x) => x.text.startsWith('已导出'))" 30 >/dev/null
ok "$([ "$(wc -l < "${OUT}" | tr -d ' ')" = 99991 ] && [ "$(head -1 "${OUT}" | tr -d '\r')" = "ID,NAME,AMT,MAYBE" ] && [ ! -e "${OUT}.part" ] && echo 1)" "点「导出…」→ CSV：$(wc -l < "${OUT}" | tr -d ' ') 行（表头 + 99990）、表头 $(head -1 "${OUT}" | tr -d '\r')、没留 .part（取到界面上的只有 $(lite_eval main "${Q} return a.rows.length") 行，导出的是全部）"
# 1–10 删掉了：第 2 行是 id 11，第 5 行是 id 14（14 能被 7 整除，MAYBE 是 NULL）
ok "$(sed -n 5p "${OUT}" | tr -d '\r' | grep -qx '14,n-14,21.00,' && echo 1)" "NULL 导出成空、DECIMAL 照原样：id 14 那行 $(sed -n 5p "${OUT}" | tr -d '\r')"
ok "$(lite_eval main "return __lite.notify.log.findLast((x) => x.text.startsWith('已导出'))?.text ?? ''" | grep -q '99,990 行到 big.csv' && echo 1)" "说出导出了几行、到哪：$(lite_eval main "return __lite.notify.log.findLast((x) => x.text.startsWith('已导出'))?.text ?? ''")"
PID_DB=$(pgrep -f lite-db.jar | head -1)
R0=$(ps -o rss= -p "${PID_DB}" | tr -d ' ')
EXP='const r = await window.__TAURI_INTERNALS__.invoke("db_export", { root: ROOT_, id: ID_, sql: "select x as id, x * 3 as triple, cast(x as varchar) as txt from system_range(1, 1000000)", path: PATH_, format: "csv", table: "t" }); return JSON.stringify(r)'
E1=$(lite_eval main "$(echo "${EXP}" | sed -e "s|ROOT_|'${P}'|" -e "s|ID_|'${ID}'|" -e "s|PATH_|'${LITE_WORK}/导出/m.csv'|")" 2>&1)
R1=$(ps -o rss= -p "${PID_DB}" | tr -d ' ')
ok "$(echo "${E1}" | grep -q '"rows":1000000' && [ $(( (R1 - R0) / 1024 )) -lt 120 ] && echo 1)" "导出一百万行：${E1}；边车 $((R0 / 1024)) → $((R1 / 1024)) MB（线：涨不过 120 MB，堆的上限 256）"
printf 'old' > "${LITE_WORK}/导出/keep.csv"
SLOW='window.__exp = window.__TAURI_INTERNALS__.invoke("db_export", { root: ROOT_, id: ID_, sql: "select a.x, b.x from system_range(1, 100000) a, system_range(1, 100000) b", path: PATH_, format: "csv", table: "t" }).then(() => "完成了", (e) => String(e)); return true'
lite_eval main "$(echo "${SLOW}" | sed -e "s|ROOT_|'${P}'|" -e "s|ID_|'${ID}'|" -e "s|PATH_|'${LITE_WORK}/导出/keep.csv'|")" >/dev/null
sleep 1.5
inv "'db_cancel', { root: '${P}', id: '${ID}' }" >/dev/null
EC=$(lite_eval main "return await window.__exp")
ok "$(echo "${EC}" | grep -q 57014 && [ "$(cat "${LITE_WORK}/导出/keep.csv")" = old ] && [ ! -e "${LITE_WORK}/导出/keep.csv.part" ] && echo 1)" "导出到一半取消：停了（$(echo "${EC}" | cut -c1-60)）、原来那个文件还是原样、没留 .part"

echo "== 改单元格（DB-3）：双击改、先给人看 UPDATE、一个事务提交；被别人删了的行整笔回滚"
HRUN='const q = await __lite.dbResults(); const a = q.active; await q.run(a.root, a.conn, "验收 H2", [SQL_]); return q.active.status + ":" + JSON.stringify(q.active.rows)'
hrun() { lite_eval main "${HRUN/SQL_/$1}"; }
# 不写字符串字面量（SQL 的单引号在 bash 单引号、${…/…/…} 替换、JS 字符串几层里两次被提前收掉）：备注是 x 转成的字，第 3 行是 NULL
hrun '"insert into shop.orders select x, x * 1.5, case when x = 3 then null else cast(x as varchar) end from system_range(1, 3)"' >/dev/null
hrun '"select * from shop.orders order by id"' >/dev/null
TBL=$(lite_eval main "${Q} return a.table ? a.table.name + ' pk=' + JSON.stringify(a.table.pk) + ' cols=' + a.table.cols.join(',') : 'null'")
ok "$([ "${TBL}" = "ORDERS pk=[0] cols=ID,AMOUNT,NOTE" ] && echo 1)" "单表、主键在结果里：知道是哪张表、主键是第几列（${TBL}）"
EDIT='const row = [...document.querySelectorAll(".dbres .row")].find((x) => x.querySelector(".gut")?.textContent === ROW_); row.querySelectorAll(".cell")[COL_].dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
await new Promise((r) => setTimeout(r, 50)); const inp = document.querySelector(".dbres .ed"); if (!inp) return "没进编辑";
if (VAL_ === null) { document.querySelector(".dbres .nullb").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); return "null"; }
inp.value = VAL_; inp.dispatchEvent(new Event("input", { bubbles: true })); inp.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); return "ok"'
cell() { lite_eval main "$(echo "${EDIT}" | sed -e "s|ROW_|\"$1\"|" -e "s|COL_|$2|" -e "s|VAL_|$3|g")"; }
cell 2 1 '"9.99"' >/dev/null
cell 1 2 null >/dev/null
PEND=$(lite_eval main "return document.querySelector('.dbres .pend')?.textContent ?? '无'")
NEDIT=$(lite_eval main "${Q} return Object.keys(a.edits).length")
ok "$([ "${PEND}" = "2 处更改未提交" ] && [ "${NEDIT}" = 2 ] && echo 1)" "双击改两格（一格改值、一格设成 NULL）：${PEND}；库里还没动"
lite_eval main "[...document.querySelectorAll('.dbres .bar button')].find((b) => b.textContent.includes('提交')).click(); return true" >/dev/null
lite_wait main "return !!document.querySelector('.dbapply')" 5 >/dev/null
PRE=$(lite_eval main "return document.querySelector('.dbapply .sqls').textContent")
ok "$(echo "${PRE}" | grep -q "SET AMOUNT = '9.99' WHERE ID = '2'" && echo "${PRE}" | grep -q "SET NOTE = NULL WHERE ID = '1'" && echo 1)" "提交前列出要执行的 UPDATE：$(echo "${PRE}" | tr '\n' ' ')"
lite_eval main "[...document.querySelectorAll('.dbapply button')].find((b) => b.textContent === '提交').click(); return true" >/dev/null
lite_wait main "return __lite.notify.log.some((x) => x.text.startsWith('已提交'))" 10 >/dev/null
lite_eval main "${Q} a.pinned = true; return true" >/dev/null
AFTER=$(hrun '"select id, amount, note from shop.orders order by id"' | cut -d: -f2-)
ok "$([ "${AFTER}" = '[["1","1.50",null],["2","9.99","2"],["3","4.50",null]]' ] && echo 1)" "库里真改了：${AFTER}"
# 回到那个钉住的结果；第 3 行在背后被删掉，再改两格（第 1 行、第 3 行）提交 —— 第 3 行那条改 0 行，整笔回滚、第 1 行也不能动
lite_eval main 'const q = await __lite.dbResults(); q.activeId = q.list.find((x) => x.pinned).id; return true' >/dev/null
hrun '"delete from shop.orders where id = 3"' >/dev/null
lite_eval main 'const q = await __lite.dbResults(); q.activeId = q.list.find((x) => x.pinned).id; return true' >/dev/null
sleep 0.3
cell 1 1 '"7.77"' >/dev/null
cell 3 1 '"5.55"' >/dev/null
lite_eval main "[...document.querySelectorAll('.dbres .bar button')].find((b) => b.textContent.includes('提交')).click(); return true" >/dev/null
lite_wait main "return !!document.querySelector('.dbapply')" 5 >/dev/null
lite_eval main "[...document.querySelectorAll('.dbapply button')].find((b) => b.textContent === '提交').click(); return true" >/dev/null
lite_wait main "return __lite.notify.log.some((x) => x.text.startsWith('提交失败'))" 10 >/dev/null
FAILMSG=$(lite_eval main "return __lite.notify.log.findLast((x) => x.text.startsWith('提交失败'))?.text ?? ''")
STILL=$(lite_eval main "${Q} return Object.keys(a.edits).length")
lite_eval main 'const q = await __lite.dbResults(); q.list.find((x) => x.pinned).pinned = false; return true' >/dev/null
AFTER2=$(hrun '"select id, amount from shop.orders order by id"' | cut -d: -f2-)
ok "$(echo "${FAILMSG}" | grep -q '第 2 处更改影响了 0 行' && [ "${STILL}" = 2 ] && [ "${AFTER2}" = '[["1","1.50"],["2","9.99"]]' ] && echo 1)" "被删的那行改了 0 行：整笔回滚（第 1 行没动：${AFTER2}）、改动还攒着、说清楚：${FAILMSG}"
hrun '"select id, amount * 2 as twice from shop.orders"' >/dev/null
cell 1 1 '"1"' >/dev/null
ok "$(lite_eval main "return __lite.notify.log.findLast(() => true)?.text ?? ''" | grep -q '计算列，无法直接修改' && [ -z "$(lite_eval main "return document.querySelector('.dbres .ed') ? 'x' : ''")" ] && echo 1)" "有计算列的结果：双击不进编辑、说清为什么：$(lite_eval main "return __lite.notify.log.findLast(() => true)?.text ?? ''")"

echo "== 没有副作用、退出不留"
ok "$([ "$(cd "${P}" && find . -type f | sort | md5)" = "${BEFORE}" ] && echo 1)" "项目里一个文件都没多（连接配置在应用数据目录里）"
lite_quit
wait_for 10 '[ -z "$(pgrep -f lite-db.jar)" ]'
ok "$([ -z "$(pgrep -f lite-db.jar)" ] && echo 1)" "退出之后边车没了"

printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
