#!/usr/bin/env bash
# #48 issue 原文那三条验收，用**真工具**在真 .app 上走：`mvn spring-boot:run`（ERROR 标红、能过滤；停了端口空、java 一个不剩）、
# `pnpm dev`、`python main.py`。scripts/accept/tasks.sh 验的是机制（用一个 /usr/bin/python3 小服务），这份验的是「你机器上那几样真东西」。
#
# 和别的验收不一样的一处：**任务的登录 shell 读你真的 .zshrc**（LITE_USER_SHELL=1，见 bridge.sh 的 lite_launch）——
# 应用经 `open` 起（LaunchServices，和 Finder 双击同一条路，PATH 只有 /usr/bin:/bin:/usr/sbin:/sbin），找得到 mvn / pnpm 全靠它，
# 这就是 TASKS.md 第 7 节第 4 条。不开终端；任务是 `zsh -ilc`，命令不经行编辑器、不进历史（第 10 节实测），脚本前后比对你的历史文件。
#
# 前提（缺哪样哪段就记「未验」，不算失败）：登录 shell 里有 mvn / pnpm / python；Maven 本地仓库里有 spring-boot 3.5.11
# （`mvn -o` 离线跑：验收不该替你下载东西）；本仓库 `pnpm install` 过。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
export LITE_USER_SHELL=1
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0; SKIP=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
skip() { SKIP=$((SKIP + 1)); printf '  - 未验：%s\n' "$1"; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

P=$(cd "$(mktemp -d /tmp/acR.XXXX)" && pwd -P)
freeport() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'; }
BP=$(freeport)
VP=$(freeport)
GROUPS_SEEN=()
cleanup() {
  local g
  for g in ${GROUPS_SEEN[@]+"${GROUPS_SEEN[@]}"}; do kill -KILL -"${g}" 2>/dev/null; done
  lite_teardown; rm -rf "${P}"
}
trap cleanup EXIT

# 用户自己的 shell 怎么看：找不到的那段跳过（这是你环境的事，不是应用的）
have() { "${SHELL}" -ilc "command -v $1" </dev/null >/dev/null 2>&1; }
HISTF=$("${SHELL}" -ic 'print -r -- ${HISTFILE:-}' </dev/null 2>/dev/null | tail -1)
hist_sum() { [ -n "${HISTF}" ] && [ -f "${HISTF}" ] && md5 -q "${HISTF}"; }
H0=$(hist_sum)

listening() { lsof -nP -iTCP:"$1" -sTCP:LISTEN -t >/dev/null 2>&1; }
# 监听那个端口的进程组 —— 停了之后查「这一组一个不剩」，不只查主进程（issue 验收 2：mvn 会 fork 一个 java）
pgid_of() { lsof -nP -iTCP:"$1" -sTCP:LISTEN -Fg 2>/dev/null | sed -n 's/^g//p' | head -1; }
group_left() { pgrep -g "$1" 2>/dev/null | wc -l | tr -d ' '; }
group_names() { pgrep -g "$1" -l 2>/dev/null | awk '{print $2}' | sort | uniq -c | tr -s ' ' | tr '\n' ',' ; }
dot() { lite_eval main "return document.querySelector('.ptab.on .dot')?.className.split(' ')[1] ?? ''" 2>/dev/null; }
# 看内容读输出文件，不读日志视图：视图只画屏幕上那几十行，Spring 停的时候后面还有一串，@PreDestroy 那行早滚出去了（第一次跑就这么红的）
logtext() { cat "$(find "${LITE_DATA}/runs" -name "$1.log" 2>/dev/null | head -1)" 2>/dev/null; }
pick() {
  lite menu run-pick main >/dev/null
  lite_wait main "return !!document.querySelector('.picker .row')" 8 >/dev/null
  lite_eval main "const r = [...document.querySelectorAll('.picker .row')].find((x) => x.querySelector('.name')?.innerText === '$1'); r?.click(); return !!r" 2>/dev/null
}
stop_and_check() { # 端口 进程组 名字
  lite menu run-stop main >/dev/null
  ok "$(wait_for 10 "! listening $1 && [ \"\$(group_left $2)\" = 0 ]" && echo 1)" \
    "⌘F2 之后 10 秒内 $1 空了、它那一组（$2）一个不剩$( [ "$(group_left "$2")" = 0 ] || echo "，还剩：$(group_names "$2")")"
  ok "$(lite_wait main "return document.querySelector('.ptab.on .dot')?.classList.contains('stopped')" 5 && echo 1)" "$3 那格是「已停止」，不是失败：$(dot)"
}

# ── 造项目 ──
mkdir -p "${P}/.lite-ide" "${P}/boot/src/main/java/demo"
cat > "${P}/boot/pom.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId><version>3.5.11</version><relativePath/></parent>
  <groupId>demo</groupId><artifactId>t48</artifactId><version>0.1</version>
  <properties><java.version>17</java.version></properties>
  <dependencies><dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-web</artifactId></dependency></dependencies>
  <!-- resources 插件钉 3.4.0：本机仓库缓存的是这一版，parent 默认的那一版离线找不到（第一次跑报 PluginResolutionException）-->
  <build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-resources-plugin</artifactId><version>3.4.0</version></plugin><plugin><groupId>org.springframework.boot</groupId><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build>
</project>
EOF
cat > "${P}/boot/src/main/java/demo/App.java" <<'EOF'
package demo;
import org.slf4j.Logger; import org.slf4j.LoggerFactory;
import org.springframework.boot.SpringApplication; import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.boot.context.event.ApplicationReadyEvent; import org.springframework.context.event.EventListener;
@SpringBootApplication
public class App {
  private static final Logger log = LoggerFactory.getLogger(App.class);
  public static void main(String[] a) { SpringApplication.run(App.class, a); }
  @EventListener(ApplicationReadyEvent.class) void ready() { log.error("T48 示范 ERROR 行"); System.out.println("T48-READY"); }
  @jakarta.annotation.PreDestroy void bye() { System.out.println("T48-PREDESTROY"); }
}
EOF
# 每 0.5 秒 print 一行、**不 flush**，6 行后正常退出
cat > "${P}/main.py" <<'EOF'
import sys, time
print("python 是", sys.executable)
for i in range(6):
    print(f"py line {i}"); time.sleep(0.5)
EOF
# 前端：本仓库自己的 vite，换一个端口（不和你可能开着的 1420 撞）。**不能在临时目录里搭一个借用本仓库 node_modules 的小项目**：
# pnpm 11 在 run 之前先查依赖、对不上就自己 `pnpm install`，第一次试的时候它要去删那个软链过去的 node_modules（本仓库那份），
# 被它自己的安全检查拦下了（ERR_PNPM_UNSAFE_MODULES_DIR）—— 所以在已经装好依赖的本仓库里跑，命令里 cd 过去
cat > "${P}/.lite-ide/tasks.json" <<EOF
[
  { "name": "后端", "command": "mvn -o spring-boot:run", "cwd": "boot", "env": { "SERVER_PORT": "${BP}" } },
  { "name": "前端", "command": "cd '${LITE_ROOT}' && pnpm dev --port ${VP} --strictPort" },
  { "name": "脚本", "command": "python main.py" }
]
EOF

lite_clean_data
lite_launch "${P}" >/dev/null || exit 1
lite_wait main "return __lite.project.root === '${P}'" 15

# 读页面的几段 JS 放进变量（bash 3.2 会把 "$( … "{ a, b }" … )" 里的花括号按逗号展开，见 smoke ㉔）
RED_JS=$(cat <<'JS'
const r = [...document.querySelectorAll('.run .row[data-lvl="error"]')].find((x) => x.innerText.includes('T48 示范 ERROR'));
if (!r) return 'ERROR 行没有标成 error 级';
const p = r.querySelector('.p[data-cls="level"]');
const probe = document.createElement('span'); probe.style.color = 'var(--lvl-error)'; document.body.append(probe);
const want = getComputedStyle(probe).color; probe.remove();
const got = p ? getComputedStyle(p).color : '没有 level 段';
return got === want ? 'red' : `${got}，应为 ${want}`;
JS
)
ONLY_ERR_JS=$(cat <<'JS'
const rows = [...document.querySelectorAll('.run .row:not(.pending)')];
return rows.length > 0 && rows.every((r) => r.dataset.lvl === 'error') && rows.some((r) => r.innerText.includes('T48 示范 ERROR'));
JS
)

echo "== 后端：mvn spring-boot:run（第一次要编译，最多等 120 秒）"
if ! have mvn; then
  skip "登录 shell 里找不到 mvn"
elif [ ! -d "$(mvn help:evaluate -Dexpression=settings.localRepository -q -DforceStdout -o 2>/dev/null)/org/springframework/boot/spring-boot-starter-parent/3.5.11" ]; then
  skip "Maven 本地仓库里没有 spring-boot 3.5.11（验收离线跑，不替你下载）"
else
  pick 后端 >/dev/null
  if wait_for 120 "listening ${BP} && logtext 后端 | grep -q T48-READY"; then
    ok 1 "从 Finder 那条路起的应用找到了 mvn，Spring Boot 监听上 ${BP}（端口经 tasks.json 的 env 传进去）"
    BG=$(pgid_of "${BP}"); GROUPS_SEEN+=("${BG}")
    ok "$( [ "$(pgrep -g "${BG}" -l | grep -c java)" -ge 2 ] && echo 1)" "组里有 Maven 的 JVM 和 fork 出来的应用 JVM：$(group_names "${BG}")"
    ok "$( [ "$(lite_eval main "${RED_JS}" 2>/dev/null)" = red ] && echo 1)" "ERROR 行标红：$(lite_eval main "${RED_JS}" 2>/dev/null)"
    lite_eval main "document.querySelector('.run .chip.error')?.click(); return true" >/dev/null
    ok "$(lite_wait main "${ONLY_ERR_JS}" 8 && echo 1)" "点 ERROR 那一级：只剩 ERROR 行（$(lite_eval main "return document.querySelector('.run .chip.error .num')?.innerText ?? ''" 2>/dev/null) 条）"
    lite_eval main "document.querySelector('.run .chip.error')?.click(); return true" >/dev/null
    stop_and_check "${BP}" "${BG}" "后端"
    ok "$(logtext 后端 | grep -q T48-PREDESTROY && echo 1)" "停之前跑了 @PreDestroy（SIGINT → 关闭钩子，先礼后兵）"
  else
    ok 0 "120 秒内后端没起来：$(logtext 后端 | tail -5 | tr '\n' ' ')"
  fi
fi

echo "== 前端：pnpm dev（本仓库的 vite，端口 ${VP}）"
if ! have pnpm; then
  skip "登录 shell 里找不到 pnpm"
elif [ ! -x "${LITE_ROOT}/node_modules/.bin/vite" ]; then
  skip "本仓库没 pnpm install 过"
else
  pick 前端 >/dev/null
  if wait_for 40 "listening ${VP}"; then
    ok 1 "找到了 pnpm，vite 监听上 ${VP}"
    VG=$(pgid_of "${VP}"); GROUPS_SEEN+=("${VG}")
    ok "$(curl -s "http://localhost:${VP}/" | grep -qi '<html' && echo 1)" "能打开页面（vite 默认只听 localhost，也就是 ::1，连 127.0.0.1 是连不上的）；组里：$(group_names "${VG}")"
    # ⌃R 重跑：旧的那次是我们停的，不能报「退出了」。**要用 pnpm 验**：它被停后打 `Command failed`、退出码非零；
    # mvn 被 SIGINT 后照样 BUILD SUCCESS、退出码 0（第 0 步实测），srv.py 也是 exit(0) —— 拿它们验不出来（code review 2026-10-10，
    # 第一版放在后端那段，旧包上照样绿才发现）。报错几秒就消失，所以一边等新的起来一边看
    lite_eval main "__lite.key('Ctrl-r'); return true" >/dev/null
    SAW=0
    for ((i = 0; i < 80; i++)); do
      [ "$(lite_eval main "return __lite.has('退出了')" 2>/dev/null)" = true ] && SAW=1
      listening "${VP}" && [ "$(pgid_of "${VP}")" != "${VG}" ] && break
      sleep 0.5
    done
    ok "$( [ "$(pgid_of "${VP}")" != "${VG}" ] && listening "${VP}" && echo 1)" "⌃R 重跑起来了（新的进程组 $(pgid_of "${VP}")）"
    sleep 2   # 报错要是有，也是旧的那次退出时弹的；新的起来之后再多看两秒
    [ "$(lite_eval main "return __lite.has('退出了')" 2>/dev/null)" = true ] && SAW=1
    ok "$( [ "${SAW}" = 0 ] && echo 1)" "重跑时旧的那次没报「退出了」—— 是我们停的，不是失败"
    VG=$(pgid_of "${VP}"); GROUPS_SEEN+=("${VG}")
    stop_and_check "${VP}" "${VG}" "前端"
  else
    ok 0 "40 秒内 vite 没起来：$(logtext 前端 | tail -5 | tr '\n' ' ')"
  fi
fi

echo "== 脚本：python main.py（每 0.5 秒 print 一行、不 flush）"
if ! have python; then
  skip "登录 shell 里找不到 python"
else
  pick 脚本 >/dev/null
  if wait_for 6 "logtext 脚本 | grep -q 'py line 1'"; then
    n=$(logtext 脚本 | grep -c 'py line')
    ok "$( [ "${n}" -lt 6 ] && echo 1)" "程序还在跑时就看到了输出（此刻 ${n}/6 行）—— print 是实时的，不是结束才一起出来"
  else
    ok 0 "6 秒内没看到 py line 1：$(logtext 脚本 | tail -3 | tr '\n' ' ')"
  fi
  ok "$(lite_wait main "return document.querySelector('.ptab.on .dot')?.classList.contains('done')" 8 && echo 1)" "跑完是「已完成」：$(dot)"
  ok "$(logtext 脚本 | grep -q 'py line 5' && echo 1)" "6 行都在；$(logtext 脚本 | grep -o 'python 是 .*' | head -1)"
fi

lite_quit >/dev/null
echo "== 你的 zsh 历史没被碰"
if [ -n "${H0}" ]; then
  ok "$( [ "$(hist_sum)" = "${H0}" ] && echo 1)" "${HISTF/#${HOME}/~} 前后一样（任务是 zsh -ilc，命令不进历史）"
else
  skip "找不到你的历史文件（HISTFILE 没设或文件不在）"
fi

printf '\n通过 %s 条，失败 %s 条，未验 %s 条\n' "${PASS}" "${FAIL}" "${SKIP}"
[ "${FAIL}" = 0 ]
