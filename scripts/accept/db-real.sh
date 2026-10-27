#!/usr/bin/env bash
# 数据库客户端连**真的 MySQL / PostgreSQL**（docs/DATABASE.md：DB-1 到 DB-4 原来只在 H2 上验过）。走测试通道（scripts/lib/bridge.sh）：不发全局按键、不抢焦点。
# 先 scripts/build-test-app.sh。要 Docker（OrbStack / Docker Desktop）和本机已有的官方镜像 mysql:8.4、postgres:17 —— 脚本**不拉镜像**（没有就记「未验」），
# 只起、用完删自己的容器（只绑 127.0.0.1、随机端口），你的镜像一个不动。密码是脚本现生成的测试密码，进脚本自己建的临时钥匙串（同 db.sh）。
#
# 每种库验：连得上（产品、版本）、驱动从 Maven Central 下来核过 sha256、密码错报库自己的错误码、表结构树、10 万行首屏和取完、
# 两百万行真的是边取边给（MySQL 的 useCursorFetch、PostgreSQL 的「只读语句临时进事务」—— 原来没验过）、取消慢查询、改数字列的格子（PostgreSQL 的 stringtype=unspecified）、导出、补全的表和列
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0; SKIP=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
skip() { SKIP=$((SKIP + 1)); printf '  - 未验：%s\n' "$1"; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }
js() { python3 -I -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"; }
now() { python3 -c 'import time; print(time.time())'; }

if ! docker info >/dev/null 2>&1; then echo "未验：Docker 没在跑"; exit 0; fi
HAVE_MY=$(docker image inspect mysql:8.4 >/dev/null 2>&1 && echo 1)
HAVE_PG=$(docker image inspect postgres:17 >/dev/null 2>&1 && echo 1)
[ -z "${HAVE_MY}${HAVE_PG}" ] && { echo "未验：本机没有 mysql:8.4 / postgres:17 镜像（脚本不替你拉）"; exit 0; }

P=$(cd "$(mktemp -d /tmp/acDbReal.XXXX)" && pwd -P)
printf 'hello\n' > "${P}/a.txt"
PW=$(python3 -c 'import secrets; print("T" + secrets.token_hex(10))')
MY="lite-acc-mysql-$$"; PG="lite-acc-pg-$$"
KC="${LITE_WORK}/db-real.keychain-db"
ORIG=$(security list-keychains -d user | tr -d '"' | xargs)
security create-keychain -p acc "${KC}" && security list-keychains -d user -s ${ORIG} && security unlock-keychain -p acc "${KC}"
cleanup() {
  lite_teardown keep
  docker rm -f "${MY}" "${PG}" >/dev/null 2>&1
  security delete-keychain "${KC}" 2>/dev/null
  security list-keychains -d user -s ${ORIG}
  rm -rf "${P}" "${LITE_WORK}"
}
trap cleanup EXIT
export LITE_DB_KEYCHAIN="${KC}"
export LITE_SAVE_PATH="${LITE_WORK}/out.csv"

echo "== 起容器（官方镜像、只绑 127.0.0.1、随机端口）"
if [ -n "${HAVE_MY}" ]; then
  docker run -d --name "${MY}" -e MYSQL_ROOT_PASSWORD="${PW}" -e MYSQL_DATABASE=shop -p 127.0.0.1::3306 mysql:8.4 >/dev/null
fi
if [ -n "${HAVE_PG}" ]; then
  docker run -d --name "${PG}" -e POSTGRES_PASSWORD="${PW}" -e POSTGRES_DB=shop -p 127.0.0.1::5432 postgres:17 >/dev/null
fi
[ -n "${HAVE_MY}" ] && wait_for 120 "docker exec ${MY} mysql -uroot -p${PW} -e 'select 1' shop"
[ -n "${HAVE_PG}" ] && wait_for 60 "docker exec ${PG} psql -U postgres -d shop -c 'select 1'"
# 造数据：10 万行的 orders（主键、DECIMAL、可空的 note）
if [ -n "${HAVE_MY}" ]; then
  docker exec -i "${MY}" mysql -uroot -p"${PW}" shop 2>/dev/null <<'EOF'
create table orders(id bigint primary key, amount decimal(12,2) not null, note varchar(200));
set session cte_max_recursion_depth = 200000;
insert into orders with recursive s(x) as (select 1 union all select x + 1 from s where x < 100000) select x, x * 1.5, case when x % 7 = 0 then null else concat('n-', x) end from s;
EOF
  MYPORT=$(docker port "${MY}" 3306 | head -1 | cut -d: -f2)
  ok "$([ -n "${MYPORT}" ] && [ "$(docker exec "${MY}" mysql -uroot -p"${PW}" -N -e 'select count(*) from shop.orders' 2>/dev/null)" = 100000 ] && echo 1)" "MySQL 8.4 起来了（127.0.0.1:${MYPORT}），orders 10 万行"
fi
if [ -n "${HAVE_PG}" ]; then
  docker exec -i "${PG}" psql -q -U postgres -d shop >/dev/null <<'EOF'
create table orders(id bigint primary key, amount numeric(12,2) not null, note varchar(200));
insert into orders select g, g * 1.5, case when g % 7 = 0 then null else 'n-' || g end from generate_series(1, 100000) g;
EOF
  PGPORT=$(docker port "${PG}" 5432 | head -1 | cut -d: -f2)
  ok "$([ -n "${PGPORT}" ] && [ "$(docker exec "${PG}" psql -U postgres -d shop -tAc 'select count(*) from orders')" = 100000 ] && echo 1)" "PostgreSQL 17 起来了（127.0.0.1:${PGPORT}），orders 10 万行"
fi

lite_clean_data
lite_launch "${P}" || exit 1
inv() { lite_eval main "const r = await window.__TAURI_INTERNALS__.invoke($1); return JSON.stringify(r)"; }
run_sql() { lite_eval main "const q = await __lite.dbResults(); await q.run('${P}', '$1', 'x', [$2]); const a = q.active; return a.status + '|' + a.rows.length + '|' + a.more + '|' + a.ms + '|' + (a.error || '')"; }
status() { lite_eval main "const q = await __lite.dbResults(); return q.active ? q.active.status : 'none'"; }

check_db() {
  local kind=$1 port=$2 user=$3 schema=$4 label=$5 sleepsql=$6 bigsql=$7
  echo "== ${label}"
  local conn="{ id: '', name: '验收 ${label}', kind: '${kind}', host: '127.0.0.1', port: ${port}, database: 'shop', user: '${user}', url: '', hasPassword: false, prod: false }"
  local wrong; wrong=$(inv "'db_test', { conn: ${conn}, password: 'nope' }" 2>&1)
  ok "$(echo "${wrong}" | grep -qE '28000|28P01|1045' && echo 1)" "密码不对：报库自己的错误码（$(echo "${wrong}" | grep -oE '（SQLState[^）]*）' | head -1)）"
  local t; t=$(inv "'db_test', { conn: ${conn}, password: '${PW}' }")
  ok "$(echo "${t}" | grep -q '"product"' && echo 1)" "连上了：$(echo "${t}" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["product"], d["version"][:40])' 2>/dev/null)"
  local jar; jar=$(ls "${LITE_DATA}/jdbc/" 2>/dev/null | grep -iE "$([ "${kind}" = mysql ] && echo mysql-connector || echo postgresql)")
  ok "$([ -n "${jar}" ] && echo 1)" "驱动从 Maven Central 下来、核过 sha256 才用（${jar}）"
  local s; s=$(inv "'db_save', { root: '${P}', conn: ${conn}, password: '${PW}' }")
  local id; id=$(echo "${s}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
  local tables; tables=$(inv "'db_meta', { root: '${P}', id: '${id}', what: 'tables', catalog: $([ "${kind}" = mysql ] && echo "'shop'" || echo null), schema: $([ "${kind}" = mysql ] && echo null || echo "'public'"), table: null }")
  local cols; cols=$(inv "'db_meta', { root: '${P}', id: '${id}', what: 'columns', catalog: $([ "${kind}" = mysql ] && echo "'shop'" || echo null), schema: $([ "${kind}" = mysql ] && echo null || echo "'public'"), table: 'orders' }")
  ok "$(echo "${tables}" | grep -q '"orders"' && echo "${cols}" | python3 -c 'import json,sys; c={x["name"]:x for x in json.load(sys.stdin)}; print(1 if c["id"]["pk"] and c["amount"]["nullable"] is False else 0)' | grep -qx 1 && echo 1)" "表结构树：orders、id 是主键、amount 非空"
  local r; r=$(run_sql "${id}" "$(js 'select * from orders order by id')")
  ok "$(echo "${r}" | cut -d'|' -f1-3 | grep -qx 'done|500|true' && echo 1)" "10 万行：第一批 500 行、还有（${r}）"
  local drain; drain=$(lite_eval main "const q = await __lite.dbResults(); const a = q.active; let n = 0; while (a.more && n < 400) { await q.fetchMore(a); n++; } return a.rows.length + ' ' + a.rows.filter((x) => x[2] === null).length + ' ' + a.rows[6][1]")
  ok "$([ "${drain}" = "100000 14285 10.50" ] && echo 1)" "取完：${drain}（行数、NULL 个数、第 7 行的 DECIMAL 照原样）"
  local side; side=$(pgrep -f lite-db.jar | head -1)
  local r0; r0=$(ps -o rss= -p "${side}" | tr -d ' ')
  local t0; t0=$(now)
  local big; big=$(run_sql "${id}" "$(js "${bigsql}")")
  local t1; t1=$(now)
  local r1; r1=$(ps -o rss= -p "${side}" | tr -d ' ')
  ok "$(echo "${big}" | cut -d'|' -f1-3 | grep -qx 'done|500|true' && python3 -c "import sys; sys.exit(0 if ${t1} - ${t0} < 5 else 1)" && [ $(( (r1 - r0) / 1024 )) -lt 150 ] && echo 1)" "两百万行：第一批 $(python3 -c "print(round(${t1} - ${t0}, 2))") 秒到、边车 $((r0 / 1024)) → $((r1 / 1024)) MB —— 是边取边给，不是整个读进内存"
  lite_eval main "const q = await __lite.dbResults(); q.close(q.active.id); return true" >/dev/null
  lite_eval main "const q = await __lite.dbResults(); void q.run('${P}', '${id}', 'x', [$(js "${sleepsql}")]); return true" >/dev/null
  sleep 1.5
  t0=$(now)
  lite_eval main "const q = await __lite.dbResults(); q.cancel(q.active); return true" >/dev/null
  for i in $(seq 1 100); do [ "$(status)" != running ] && break; done
  t1=$(now)
  local st; st=$(lite_eval main "const q = await __lite.dbResults(); return q.active.status + ' ' + q.active.error.slice(0, 120)")
  local after; after=$(run_sql "${id}" "$(js 'select 1 as one')")
  ok "$(echo "${st}" | grep -q '^cancelled' && python3 -c "import sys; sys.exit(0 if ${t1} - ${t0} < 2 else 1)" && echo "${after}" | grep -q '^done|1|' && echo 1)" "取消慢查询：$(python3 -c "print(round(${t1} - ${t0}, 2))") 秒停下（${st}），之后照常能用"
  run_sql "${id}" "$(js 'select * from orders where id <= 3 order by id')" >/dev/null
  local edit; edit=$(lite_eval main "const q = await __lite.dbResults(); const a = q.active; if (!a.table) return 'no table'; q.previewApply = async () => true; q.stage(a, 1, 1, '9.99'); q.stage(a, 0, 2, null); await q.commit(a); return a.rows[1][1] + '|' + a.rows[0][2] + '|' + Object.keys(a.edits).length")
  local back; back=$(lite_eval main "const q = await __lite.dbResults(); await q.run('${P}', '${id}', 'x', ['select amount, note from orders where id <= 2 order by id']); return JSON.stringify(q.active.rows)")
  ok "$([ "${edit}" = "9.99|null|0" ] && [ "${back}" = '[["1.50",null],["9.99","n-2"]]' ] && echo 1)" "改格子（数字列、设 NULL）提交、库里真改了：${back}"
  local ex; ex=$(inv "'db_export', { root: '${P}', id: '${id}', sql: 'select * from orders', path: '${LITE_WORK}/${kind}.csv', format: 'csv', table: 'orders' }")
  ok "$(echo "${ex}" | grep -q '"rows":100000' && [ "$(wc -l < "${LITE_WORK}/${kind}.csv" | tr -d ' ')" = 100001 ] && echo 1)" "导出 CSV：${ex}"
  local ol; ol=$(inv "'db_outline', { root: '${P}', id: '${id}' }")
  ok "$(echo "${ol}" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(1 if any(c["table"]=="orders" and c["name"]=="amount" for c in d["cols"]) else 0)' | grep -qx 1 && echo 1)" "补全读到的表和列里有 orders.amount（默认 schema：$(echo "${ol}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["defaultSchema"])')）"
  # 导出中途取消、单值查询超时：MySQL 这两样也是驱动另开一个连接发 KILL QUERY（导出同样走服务器端游标）
  lite_eval main "window.__exp = window.__TAURI_INTERNALS__.invoke('db_export', { root: '${P}', id: '${id}', sql: $(js "${bigsql}"), path: '${LITE_WORK}/${kind}-big.csv', format: 'csv', table: 't' }).then(() => '完成了', (e) => String(e)); return true" >/dev/null
  sleep 0.8
  inv "'db_cancel', { root: '${P}', id: '${id}' }" >/dev/null
  local ec; ec=$(lite_eval main "return await window.__exp")
  local after2; after2=$(run_sql "${id}" "$(js 'select count(*) from orders')")
  local cnt; cnt=$(lite_eval main "const q = await __lite.dbResults(); return q.active.rows[0][0]")
  ok "$([ "${ec}" != 完成了 ] && [ ! -e "${LITE_WORK}/${kind}-big.csv" ] && echo "${after2}" | grep -q '^done|1|' && [ "${cnt}" = 100000 ] && echo 1)" "导出到一半取消：${ec:0:90}；不留半截文件（$(ls "${LITE_WORK}" | grep -c "${kind}-big") 个），之后照常能查（count = ${cnt}）"
  local to; to=$(inv "'db_scalar', { root: '${P}', id: '${id}', sql: $(js "${sleepsql}"), seconds: 1 }" 2>&1)
  local after3; after3=$(run_sql "${id}" "$(js 'select count(*) from orders')")
  cnt=$(lite_eval main "const q = await __lite.dbResults(); return q.active.rows[0][0]")
  ok "$(echo "${after3}" | grep -q '^done|1|' && [ "${cnt}" = 100000 ] && echo 1)" "单值查询超时（生产库保护数行数那一步）：${to:0:80}；之后照常能查（count = ${cnt}）"
}

if [ -n "${HAVE_MY}" ]; then
  check_db mysql "${MYPORT}" root shop "MySQL 8.4" "select sleep(30)" "select a.id, b.id from orders a cross join orders b limit 2000000"
else
  skip "没有 mysql:8.4 镜像"
fi
if [ -n "${HAVE_PG}" ]; then
  check_db postgresql "${PGPORT}" postgres public "PostgreSQL 17" "select pg_sleep(30)" "select g, md5(g::text) from generate_series(1, 2000000) g"
else
  skip "没有 postgres:17 镜像"
fi

echo "== 退出不留"
lite_quit
wait_for 10 '[ -z "$(pgrep -f lite-db.jar)" ]'
ok "$([ -z "$(pgrep -f lite-db.jar)" ] && echo 1)" "退出之后边车没了"

printf '\n通过 %s 条，失败 %s 条，未验 %s 条\n' "${PASS}" "${FAIL}" "${SKIP}"
[ "${FAIL}" = 0 ]
