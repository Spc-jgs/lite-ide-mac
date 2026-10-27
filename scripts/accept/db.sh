#!/usr/bin/env bash
# 数据库客户端 DB-1（docs/DATABASE.md）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不抢焦点。
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

echo "== 没有副作用、退出不留"
ok "$([ "$(cd "${P}" && find . -type f | sort | md5)" = "${BEFORE}" ] && echo 1)" "项目里一个文件都没多（连接配置在应用数据目录里）"
lite_quit
wait_for 10 '[ -z "$(pgrep -f lite-db.jar)" ]'
ok "$([ -z "$(pgrep -f lite-db.jar)" ] && echo 1)" "退出之后边车没了"

printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
