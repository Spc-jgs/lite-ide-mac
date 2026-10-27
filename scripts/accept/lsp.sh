#!/usr/bin/env bash
# 通用语言服务器（docs/LANGPACK.md）的真 .app 验收：拿本机的 rust-analyzer 当第一个语言包，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不抢焦点。
# 先 scripts/build-test-app.sh。没有 rust-analyzer 的机器整份记「未验」。
#
# 测试应用的登录 shell 不读你的 rc（ZDOTDIR 指到空目录），rust-analyzer 在 ~/.cargo/bin 里它找不到 —— 所以往应用数据目录放一份你自己的包、命令写绝对路径：
# 顺带验「你自己的包盖过内置的」。工程放在临时目录（rust-analyzer 会跑 cargo metadata、生成 Cargo.lock，不碰你的仓库）
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

RA=$(command -v rust-analyzer || true)
if [ -z "${RA}" ] || ! "${RA}" --version >/dev/null 2>&1; then
  echo "未验：这台机器上没有 rust-analyzer（rustup component add rust-analyzer）"
  exit 0
fi

P=$(cd "$(mktemp -d /tmp/acLsp.XXXX)" && pwd -P)
mkdir -p "${P}/src"
printf '[package]\nname = "probe"\nversion = "0.1.0"\nedition = "2021"\n' > "${P}/Cargo.toml"
cat > "${P}/src/money.rs" <<'EOF'
pub struct Money {
    pub cents: i64,
}

impl Money {
    pub fn new(cents: i64) -> Self {
        Money { cents }
    }

    /// 把分换算成元
    pub fn yuan(&self) -> f64 {
        self.cents as f64 / 100.0
    }
}
EOF
cat > "${P}/src/main.rs" <<'EOF'
mod money;
use money::Money;

fn main() {
    let m = Money::new(150);
    println!("{}", m.yuan());
    let wrong: i32 = "x";
}
EOF
printf 'def yuan(cents: int) -> float:\n    return cents / 100\n\nx: int = "a"\n' > "${P}/tool.py"
cleanup() {
  lite_teardown keep
  rm -rf "${P}" "${LITE_WORK}"
}
trap cleanup EXIT

lite_clean_data
mkdir -p "${LITE_DATA}/packs"
python3 -I -c 'import json,sys; json.dump({"schema": 1, "id": "rust", "name": "Rust（验收）", "files": ["*.rs"], "languageId": "rust", "roots": ["Cargo.toml"], "server": {"command": [sys.argv[1]]}}, open(sys.argv[2], "w"), ensure_ascii=False)' "${RA}" "${LITE_DATA}/packs/rust.json"
# 测试应用的登录 shell 不读你的 rc：node / npm（一键装 Python 的语言服务器要它们）在哪，直接告诉它（测试构建才认 LITE_LOGIN_PATH）
NODE=$(command -v node || true); NPM=$(command -v npm || true)
export LITE_LOGIN_PATH="${NODE:+$(dirname "${NODE}"):}${NPM:+$(dirname "${NPM}"):}/usr/bin:/bin:/usr/sbin:/sbin"
lite_launch "${P}" || exit 1
T0=$(python3 -c 'import time; print(time.time())')
lite_eval main "await __lite.tabflow.openPath('${P}/src/main.rs', { preview: false }); return true" >/dev/null

echo "== 实时报错：rust-analyzer 起来、索引完，类型错的那一行出波浪线"
lite_wait main "return document.querySelectorAll('.cm-diag-err').length > 0" 90 >/dev/null
T1=$(python3 -c 'import time; print(time.time())')
DIAG=$(lite_eval main "return [...document.querySelectorAll('.cm-diag-err')].map((e) => e.textContent).join('|')")
ok "$(echo "${DIAG}" | grep -q '"x"' && echo 1)" "打开到第一条波浪线：$(python3 -c "print(round(${T1} - ${T0}, 1))") 秒，画在 ${DIAG} 上"
RAPID=$(pgrep -P "$(lite_pid)" -f rust-analyzer | head -1)
ok "$([ -n "${RAPID}" ] && echo 1)" "rust-analyzer 是测试应用起的子进程（pid ${RAPID:-无}），用的是你自己那份包里的命令"
ok "$(grep -rq 'Rust（验收） 起了' "${HOME}/Library/Logs/${LITE_ID}" 2>/dev/null && echo 1)" "应用数据目录里你自己的包盖过了内置的（日志里起的是「Rust（验收）」）"

echo "== 补全、F1 文档、⌘B 跳到别的文件、⌥F7 查用法"
LINE_OF='const t = __lite.text().split("\n"); const at = (s) => t.findIndex((x) => x.includes(s)) + 1;'
# 开一个文件、等通用的语言服务器那一层装上（编辑器上的 data-lsp）、服务器那边登记好这份文档（再等半秒）：开完当场按键会落空
open_ready() {
  lite_eval main "await __lite.tabflow.openPath('$1', { preview: false }); return true" >/dev/null
  lite_wait main "return !!document.querySelector('.cm-editor[data-lsp]') && __lite.tabs.active?.path === '$1'" 15 >/dev/null
  sleep 0.5
}
lite_eval main "${LINE_OF} const l = at('m.yuan()'); __lite.caret(l, t[l - 1].indexOf('yuan') + 3); __lite.key('F1'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.popup.doc')" 15 >/dev/null
DOC=$(lite_eval main "return document.querySelector('.popup.doc')?.textContent ?? ''")
ok "$(echo "${DOC}" | grep -q 'fn yuan' && echo "${DOC}" | grep -q '把分换算成元' && echo 1)" "F1：签名 + 文档注释（$(echo "${DOC}" | tr '\n' ' ' | cut -c1-60)）"
lite_eval main "document.querySelector('.popup.doc')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null
lite_eval main "${LINE_OF} const l = at('println'); __lite.caret(l, t[l - 1].length + 1); __lite.type('\n    let _y = m.'); return true" >/dev/null
lite_eval main "__lite.type('yu'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.some((l) => l.startsWith('yuan'))" 15 >/dev/null
ok "$(lite_eval main "return __lite.completions().labels.slice(0, 5).join(',')" | grep -q 'yuan' && echo 1)" "补全：m.yu → $(lite_eval main "return __lite.completions().labels.slice(0, 5).join(',')")"
lite_eval main "document.querySelector('.cm-content').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null
SIG='const on = () => [...document.querySelectorAll(".cm-java-sig .row.on b")].map((b) => b.textContent).join();'
lite_eval main "${LINE_OF} const l = at('let _y'); __lite.caret(l, t[l - 1].length + 1); __lite.type('\n    let _n = Money::new('); return true" >/dev/null
lite_wait main "${SIG} return on() === 'cents: i64'" 15 >/dev/null
ok "$([ "$(lite_eval main "${SIG} return on()")" = "cents: i64" ] && echo 1)" "参数提示：敲 Money::new( 弹出签名、当前参数 cents: i64（$(lite_eval main "return document.querySelector('.cm-java-sig')?.innerText.split('\\n')[0] ?? '没弹'")）"
lite_eval main "${LINE_OF} const l = at('Money::new'); __lite.caret(l, t[l - 1].indexOf('Money::new') + 2); __lite.key('Mod-b'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/money.rs')" 15 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path.endsWith('/money.rs') + ':' + __lite.where().split(':')[0]")" = "true:1" ] && echo 1)" "⌘B 在 Money 上：跳到 money.rs 第 1 行 struct Money（$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")）"
lite_eval main "${LINE_OF} const l = at('pub fn yuan'); __lite.caret(l, t[l - 1].indexOf('yuan') + 2); __lite.key('Alt-F7'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.usages .row').length > 0" 20 >/dev/null
USES=$(lite_eval main "return [...document.querySelectorAll('.usages .row')].map((r) => r.innerText.replace(/\\s+/g, ' ')).join(' | ')")
ok "$(echo "${USES}" | grep -q 'main.rs' && echo 1)" "⌥F7 在 yuan 的声明上：列出 main.rs 里的调用（${USES}）"
lite_eval main "document.querySelector('.usages')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null

echo "== ⇧F6 重命名（跨文件、盘上真改了）、⌥⌘L 格式化、⌥↵ 快速修复"
# 先把 main.rs 放回原样（上面补全那段打的半行留着会让 rust-analyzer 报错，改名的结果不好对）、存盘
lite_eval main "await __lite.tabflow.openPath('${P}/src/main.rs', { preview: false }); return true" >/dev/null
MAIN0=$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' "${P}/src/main.rs")
lite_eval main "__lite.setText(${MAIN0}); return true" >/dev/null
lite_menu_save() { lite menu save main >/dev/null; }
lite_menu_save
lite_wait main "return __lite.tabs.active?.dirty === false" 10 >/dev/null
open_ready "${P}/src/money.rs"
lite_eval main "${LINE_OF} const l = at('pub fn yuan'); __lite.caret(l, t[l - 1].indexOf('yuan') + 2); __lite.key('Shift-F6'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.rename input')" 15 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelector('.rename .title')?.textContent + '|' + document.querySelector('.rename input')?.value")" = "重命名 yuan|yuan" ] && echo 1)" "⇧F6 弹出重命名框，带着原名"
lite_eval main "__lite.fill('to_yuan', '.rename input'); document.querySelector('.rename input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true" >/dev/null
lite_wait main "return __lite.notify.log.some((x) => x.text.startsWith('已将 yuan 重命名为 to_yuan'))" 20 >/dev/null
ok "$(grep -q 'pub fn to_yuan' "${P}/src/money.rs" && grep -q 'm.to_yuan()' "${P}/src/main.rs" && ! grep -q 'yuan()' <(grep -v to_yuan "${P}/src/main.rs" "${P}/src/money.rs") && echo 1)" "重命名跨两个文件、盘上真改了：$(lite_eval main "return __lite.notify.log.findLast((x) => x.text.startsWith('已将')).text")"
lite_eval main "${LINE_OF} const l = at('pub cents'); __lite.editLine(l, 'pub cents: i64,', 'pub   cents :i64 ,'); __lite.caret(1, 1); __lite.key('Alt-Mod-l'); return true" >/dev/null
lite_wait main "return __lite.text().includes('pub cents: i64,')" 15 >/dev/null
ok "$(lite_eval main "return __lite.text().includes('pub cents: i64,') && !__lite.text().includes('pub   cents')" | grep -qx true && echo 1)" "⌥⌘L：rust-analyzer 交给 rustfmt 排回 pub cents: i64,（是编辑器里的一笔修改，⌘Z 可撤销）"
open_ready "${P}/src/main.rs"
lite_eval main "${LINE_OF} const l = at('let m ='); __lite.caret(l, t[l - 1].indexOf('m =') + 1); __lite.key('Alt-Enter'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.fixes [role=option], .fixes .row').length > 0" 15 >/dev/null
FIXES=$(lite_eval main "return [...document.querySelectorAll('.fixes [role=option], .fixes .row')].map((r) => r.textContent.trim()).join(' | ')")
ok "$(echo "${FIXES}" | grep -qi 'type' && echo 1)" "⌥↵ 在 let m 上：列出 rust-analyzer 的改法（${FIXES}）"
lite_eval main "const r = [...document.querySelectorAll('.fixes [role=option], .fixes .row')].find((x) => /explicit type/i.test(x.textContent)); if (!r) return false; r.click(); return true" >/dev/null
lite_wait main "return __lite.text().includes('let m: Money')" 15 >/dev/null
ok "$(lite_eval main "return __lite.text().includes('let m: Money = Money::new(150);')" | grep -qx true && echo 1)" "选「插入显式类型」：要的改动先 resolve 再放进编辑器 → let m: Money = Money::new(150);"

echo "== 没装的语言：说一次、带「安装」；点了 npm 照锁文件装进应用数据目录，装好当场接上"
lite_eval main "await __lite.tabflow.openPath('${P}/tool.py', { preview: false }); return true" >/dev/null
lite_wait main "return !!document.querySelector('.toast .act') && document.querySelector('.toast').textContent.includes('basedpyright')" 15 >/dev/null
ACT=$(lite_eval main "return document.querySelector('.toast .act')?.textContent ?? ''")
lite_eval main "await __lite.tabflow.openPath('${P}/src/main.rs', { preview: false }); await new Promise((r) => setTimeout(r, 300)); await __lite.tabflow.openPath('${P}/tool.py', { preview: false }); return true" >/dev/null
sleep 1.5
N=$(lite_eval main "return __lite.notify.log.filter((x) => x.text.includes('basedpyright')).length")
ok "$([ "${N}" = 1 ] && [ "${ACT}" = "安装（约 42 MB）" ] && echo 1)" "打开 .py：说「未找到 basedpyright」、带「${ACT}」，切走再回来不再说（说了 ${N} 次）"
ok "$([ "$(pgrep -P "$(lite_pid)" | xargs -n1 ps -o comm= -p 2>/dev/null | grep -c basedpyright)" = 0 ] && echo 1)" "没装的那个当然也没起"
if [ -z "${NPM}" ]; then
  echo "  未验：这台机器上没有 npm，一键安装那段跳过"
else
  # 再说一次那句（told 只记一次）：直接调安装（同点「安装」那条路：lsp-generic 的 install）。点的那一下要胶囊还在，前面切过标签它可能已经走了
  T0=$(python3 -c 'import time; print(time.time())')
  lite_eval main "const r = await window.__TAURI_INTERNALS__.invoke('lsp_install', { id: 'python' }); return JSON.stringify(r)" > "${LITE_WORK}/inst.json" 2>&1
  T1=$(python3 -c 'import time; print(time.time())')
  INST=$(cat "${LITE_WORK}/inst.json")
  ok "$(echo "${INST}" | grep -q '"bytes"' && [ -x "${LITE_DATA}/lsp/python/node_modules/.bin/basedpyright-langserver" ] && [ -z "$(ls -a "${LITE_DATA}/lsp" | grep -E '\.part-|\.old-')" ] && echo 1)" "一键安装：npm ci 照锁文件装进应用数据目录 lsp/python（$(python3 -c "print(round(${T1} - ${T0}, 1))") 秒，$(du -sh "${LITE_DATA}/lsp/python" 2>/dev/null | cut -f1)），没留下临时目录"
  ok "$(grep -q '"basedpyright": "1.40.2"' "${LITE_DATA}/lsp/python/package.json" && python3 -I -c 'import json,sys; v=json.load(open(sys.argv[1]))["version"]; print(1 if v == "1.40.2" else 0)' "${LITE_DATA}/lsp/python/node_modules/basedpyright/package.json" | grep -qx 1 && echo 1)" "装上的是锁文件钉的那个版本（basedpyright 1.40.2）"
  # 关掉再开 tool.py：这一次服务器起得来（「起不来」的记录装好时清掉了）
  lite_eval main "const t = __lite.tabs.byPath('${P}/tool.py'); if (t) __lite.tabflow.doClose(t); return true" >/dev/null
  sleep 1.5
  lite_eval main "await __lite.tabflow.openPath('${P}/tool.py', { preview: false }); return true" >/dev/null
  lite_wait main "return document.querySelectorAll('.cm-diag-err').length > 0" 60 >/dev/null
  PYD=$(lite_eval main "return [...document.querySelectorAll('.cm-diag-err')].map((e) => e.textContent).join('|')")
  ok "$(echo "${PYD}" | grep -q '"a"' && pgrep -P "$(lite_pid)" -f basedpyright >/dev/null && echo 1)" "装好之后打开 .py：basedpyright 起了（测试应用的子进程），类型错的那一处出波浪线（${PYD}）"
  PYPID=$(pgrep -P "$(lite_pid)" -f basedpyright | head -1)
fi

echo "== 退出不留"
lite_quit
wait_for 10 "! kill -0 ${RAPID:-0} 2>/dev/null"
ok "$(! kill -0 "${RAPID:-0}" 2>/dev/null && echo 1)" "退出之后 rust-analyzer 没了"
[ -n "${PYPID:-}" ] && ok "$(! kill -0 "${PYPID}" 2>/dev/null && echo 1)" "退出之后 basedpyright 也没了"

printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
