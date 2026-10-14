#!/usr/bin/env bash
# 认出 Spring Boot 应用、照 IDEA 那样起（docs/IDEA.md 第 1 步）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）。
# 用你机器上的真 mvn / java：任务的登录 shell 读你的 .zshrc（LITE_USER_SHELL=1，同 tasks-real.sh），不开终端、前后比对你的历史文件。
#
# 项目是脚本自己写的三模块样例（shop-common + order-service + user-service），不是下载来的代码。
# 前提（缺了就记「未验」）：登录 shell 里有 mvn；本地仓库里有 spring-boot 4.0.1 的 starter 和 starter-webmvc（验收不该替你下载东西 ——
# 没有的话 mvn 会去下，那一段就慢、但不算错）。不 install：验完查本地仓库里没有 com/demo。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
export LITE_USER_SHELL=1
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0; SKIP=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
skip() { SKIP=$((SKIP + 1)); printf '  - 未验：%s\n' "$1"; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

P=$(cd "$(mktemp -d /tmp/acS.XXXX)" && pwd -P)
UP=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
GROUPS_SEEN=()
cleanup() {
  local g
  for g in ${GROUPS_SEEN[@]+"${GROUPS_SEEN[@]}"}; do kill -KILL -"${g}" 2>/dev/null; done
  lite_teardown; rm -rf "${P}"
}
trap cleanup EXIT

have() { "${SHELL}" -ilc "command -v $1" </dev/null >/dev/null 2>&1; }
if ! have mvn || ! have java; then
  skip "登录 shell 里没有 mvn / java"
  printf '\n通过 %s 条，失败 %s 条，未验 %s 条\n' "${PASS}" "${FAIL}" "${SKIP}"; exit 0
fi
HISTF=$("${SHELL}" -ic 'print -r -- ${HISTFILE:-}' </dev/null 2>/dev/null | tail -1)
hist_sum() { [ -n "${HISTF}" ] && [ -f "${HISTF}" ] && md5 -q "${HISTF}"; }
H0=$(hist_sum)
# 本地仓库在哪由你的 settings.xml 说了算（这台机器上不是 ~/.m2）：问 mvn 自己
REPO=$("${SHELL}" -ilc 'mvn -q help:evaluate -Dexpression=settings.localRepository -DforceStdout' </dev/null 2>/dev/null | tail -1)

listening() { lsof -nP -iTCP:"$1" -sTCP:LISTEN -t >/dev/null 2>&1; }
pgid_of() { lsof -nP -iTCP:"$1" -sTCP:LISTEN -Fg 2>/dev/null | sed -n 's/^g//p' | head -1; }
group_left() { pgrep -g "$1" 2>/dev/null | wc -l | tr -d ' '; }
# 同一个任务跑两次各一份输出文件：取最新的那份
logtext() { local f; f=$(ls -t "${LITE_DATA}/runs/"*/"$1"-*.log "${LITE_DATA}/runs/$1"-*.log 2>/dev/null | head -1); [ -n "${f}" ] && cat "${f}"; }
pick() {
  lite menu run-pick main >/dev/null
  lite_wait main "return !!document.querySelector('.picker .row')" 8 >/dev/null
  lite_eval main "const r = [...document.querySelectorAll('.picker .row')].find((x) => x.querySelector('.name')?.innerText === '$1'); r?.click(); return !!r" 2>/dev/null
}

# ── 造项目：和 docs/IDEA.md 第 8 节实测用的是同一个样子 ──
mkdir -p "${P}/shop-common/src/main/java/com/demo/common" "${P}/order-service/src/main/java/com/demo/order" "${P}/order-service/src/main/resources" \
  "${P}/user-service/src/main/java/com/demo/user" "${P}/user-service/src/main/resources"
cat > "${P}/pom.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId><version>4.0.1</version><relativePath/></parent>
  <groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version><packaging>pom</packaging>
  <properties><java.version>17</java.version></properties>
  <modules><module>shop-common</module><module>order-service</module><module>user-service</module></modules>
</project>
EOF
cat > "${P}/shop-common/pom.xml" <<'EOF'
<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version></parent>
  <artifactId>shop-common</artifactId></project>
EOF
money() { printf 'package com.demo.common;\npublic record Money(long cents) {\n    public String show() { return String.format("%s%%d.%%02d", cents / 100, cents %% 100); }\n}\n' "$1" > "${P}/shop-common/src/main/java/com/demo/common/Money.java"; }
money "¥"
for m in order user; do
  starter=spring-boot-starter; [ "${m}" = user ] && starter=spring-boot-starter-webmvc
  cat > "${P}/${m}-service/pom.xml" <<EOF
<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version></parent>
  <artifactId>${m}-service</artifactId>
  <dependencies>
    <dependency><groupId>com.demo</groupId><artifactId>shop-common</artifactId><version>\${project.version}</version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>${starter}</artifactId></dependency>
  </dependencies></project>
EOF
done
cat > "${P}/order-service/src/main/java/com/demo/order/OrderApplication.java" <<'EOF'
package com.demo.order;

import com.demo.common.Money;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.boot.CommandLineRunner;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

@SpringBootApplication
public class OrderApplication implements CommandLineRunner {
    @Value("${demo.greeting}") String greeting;
    public static void main(String[] args) { SpringApplication.run(OrderApplication.class, args); }
    @Override public void run(String... args) { System.out.println("LITE-IDE-PROBE " + greeting + " " + new Money(12345).show()); }
}
EOF
printf 'demo:\n  greeting: hello-from-yml\nspring:\n  main:\n    banner-mode: off\n' > "${P}/order-service/src/main/resources/application.yml"
cat > "${P}/user-service/src/main/java/com/demo/user/UserApplication.java" <<'EOF'
package com.demo.user;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

@SpringBootApplication
public class UserApplication {
    public static void main(String[] args) { SpringApplication.run(UserApplication.class, args); }
}
EOF
printf 'server:\n  port: %s\n' "${UP}" > "${P}/user-service/src/main/resources/application.yml"

lite_launch "${P}" || exit 1

echo "== 认出来：任务列表（⌃⌥R）里有「Spring Boot」一组，两个服务，公共模块不算"
lite menu run-pick main >/dev/null
lite_wait main "return document.querySelectorAll('.picker .row').length >= 2" 10 >/dev/null
L=$(lite_eval main "return [...document.querySelectorAll('.picker .group')].map((g) => g.innerText).join('|') + ' :: ' + [...document.querySelectorAll('.picker .row .name:not(.action)')].map((n) => n.innerText).join(',')")
ok "$([ "${L}" = "Spring Boot :: OrderApplication,UserApplication" ] && echo 1)" "${L}"
lite_eval main "document.querySelector('.scrim')?.click(); return true" >/dev/null

echo "== 起 OrderApplication：兄弟模块的类、application.yml 的值都对（classpath 拼对了）"
pick OrderApplication >/dev/null
ok "$(wait_for 120 "logtext OrderApplication | grep -q 'LITE-IDE-PROBE hello-from-yml ¥123.45'" && echo 1)" "输出里有探针那行：$(logtext OrderApplication | grep -E 'LITE-IDE-PROBE|BUILD|ERROR' | head -3 | tr '\n' ' ')"
ok "$(logtext OrderApplication | grep -q 'BUILD SUCCESS' && echo 1)" "Maven 的编译输出也在同一份日志里（IDEA 是 Build 窗 + Run 窗两处，这里一处从头看到尾）"

echo "== 改公共模块、再起：不用 install，新的就生效"
money "CNY "
sleep 1
pick OrderApplication >/dev/null
ok "$(wait_for 120 "logtext OrderApplication | grep -q 'LITE-IDE-PROBE hello-from-yml CNY 123.45'" && echo 1)" "第二次起是新的格式：$(logtext OrderApplication | grep LITE-IDE-PROBE | tail -1)"

echo "== 起 UserApplication（web）：端口起来；⌘F2 停：端口空了、那一组一个不剩"
pick UserApplication >/dev/null
if wait_for 120 "listening ${UP}"; then
  G=$(pgid_of "${UP}"); GROUPS_SEEN+=("${G}")
  ok 1 "监听 ${UP}（进程组 ${G}）"
  lite menu run-stop main >/dev/null
  ok "$(wait_for 15 "! listening ${UP} && [ \"\$(group_left ${G})\" = 0 ]" && echo 1)" "停了：端口空了、组里剩 $(group_left "${G}") 个"
else
  ok 0 "120 秒没起来：$(logtext UserApplication | tail -5 | tr '\n' ' ')"
fi

echo "== 没有副作用"
if [ -n "${REPO}" ] && [ -d "${REPO}" ]; then
  ok "$([ ! -e "${REPO}/com/demo" ] && echo 1)" "本地仓库（${REPO/#$HOME/~}）里没有 com/demo：没 install"
else
  skip "问不出本地仓库在哪（mvn help:evaluate 没回话）"
fi
ok "$([ "$(hist_sum)" = "${H0}" ] && echo 1)" "你的 shell 历史没变"

lite_quit
printf '\n通过 %s 条，失败 %s 条，未验 %s 条\n' "${PASS}" "${FAIL}" "${SKIP}"
[ "${FAIL}" = 0 ]
