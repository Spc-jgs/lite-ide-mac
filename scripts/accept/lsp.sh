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
printf 'print("hi")\n' > "${P}/tool.py"
cleanup() {
  lite_teardown keep
  rm -rf "${P}" "${LITE_WORK}"
}
trap cleanup EXIT

lite_clean_data
mkdir -p "${LITE_DATA}/packs"
python3 -I -c 'import json,sys; json.dump({"schema": 1, "id": "rust", "name": "Rust（验收）", "files": ["*.rs"], "languageId": "rust", "roots": ["Cargo.toml"], "server": {"command": [sys.argv[1]]}}, open(sys.argv[2], "w"), ensure_ascii=False)' "${RA}" "${LITE_DATA}/packs/rust.json"
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
lite_eval main "${LINE_OF} const l = at('Money::new'); __lite.caret(l, t[l - 1].indexOf('Money::new') + 2); __lite.key('Mod-b'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/money.rs')" 15 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path.endsWith('/money.rs') + ':' + __lite.where().split(':')[0]")" = "true:1" ] && echo 1)" "⌘B 在 Money 上：跳到 money.rs 第 1 行 struct Money（$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")）"
lite_eval main "${LINE_OF} const l = at('pub fn yuan'); __lite.caret(l, t[l - 1].indexOf('yuan') + 2); __lite.key('Alt-F7'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.usages .row').length > 0" 20 >/dev/null
USES=$(lite_eval main "return [...document.querySelectorAll('.usages .row')].map((r) => r.innerText.replace(/\\s+/g, ' ')).join(' | ')")
ok "$(echo "${USES}" | grep -q 'main.rs' && echo 1)" "⌥F7 在 yuan 的声明上：列出 main.rs 里的调用（${USES}）"
lite_eval main "document.querySelector('.usages')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null

echo "== 没装的语言：说一次、带怎么装；不是包认的文件不起服务器"
lite_eval main "await __lite.tabflow.openPath('${P}/tool.py', { preview: false }); return true" >/dev/null
lite_wait main "return __lite.notify.log.some((x) => x.text.includes('basedpyright'))" 15 >/dev/null
lite_eval main "await __lite.tabflow.openPath('${P}/src/main.rs', { preview: false }); await new Promise((r) => setTimeout(r, 300)); await __lite.tabflow.openPath('${P}/tool.py', { preview: false }); return true" >/dev/null
sleep 1.5
N=$(lite_eval main "return __lite.notify.log.filter((x) => x.text.includes('basedpyright')).length")
ok "$([ "${N}" = 1 ] && lite_eval main "return __lite.notify.log.find((x) => x.text.includes('basedpyright'))?.text ?? ''" | grep -q '安装：' && echo 1)" "打开 .py：说「未找到 basedpyright」、带怎么装，切走再回来不再说（说了 ${N} 次）"
ok "$([ "$(pgrep -P "$(lite_pid)" | xargs -n1 ps -o comm= -p 2>/dev/null | grep -c basedpyright)" = 0 ] && echo 1)" "没装的那个当然也没起"

echo "== 退出不留"
lite_quit
wait_for 10 "! kill -0 ${RAPID:-0} 2>/dev/null"
ok "$(! kill -0 "${RAPID:-0}" 2>/dev/null && echo 1)" "退出之后 rust-analyzer 没了"

printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
