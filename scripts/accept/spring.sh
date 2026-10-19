#!/usr/bin/env bash
# 认出 Spring Boot 应用、照 IDEA 那样起、Services 工具窗（第 12 节）、Maven 工具窗（docs/IDEA.md 第 1 步）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）。
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
  <profiles><profile><id>probe</id></profile></profiles>
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

echo "== 起 UserApplication（web）：端口起来；Services 工具窗里在 Running、带端口；⌘F2 停：端口空了、那一组一个不剩"
# Services 的树（ServicesView.svelte）：`grp(名字)` 是这个服务那一行往上最近的分组名，`port(名字)` 是那一行的端口
SVC='const rows = () => [...document.querySelectorAll(".svc .tree .row")]; const nm = (r) => r.querySelector(".name")?.textContent; const row = (n) => rows().find((r) => nm(r) === n); const grp = (n) => { const rs = rows(); for (let i = rs.indexOf(row(n)); i >= 0; i--) if (["Running", "Failed", "Finished", "Not Started"].includes(nm(rs[i]))) return nm(rs[i]); return "?"; }; const port = (n) => row(n)?.querySelector(".port")?.textContent ?? "";'
pick UserApplication >/dev/null
if wait_for 120 "listening ${UP}"; then
  G=$(pgid_of "${UP}"); GROUPS_SEEN+=("${G}")
  ok 1 "监听 ${UP}（进程组 ${G}）"
  ok "$([ "$(lite_eval main "return document.querySelector('.panel-head .tw-name')?.textContent === 'Services' && !!document.querySelector('.rail button[aria-label=\"Services\"]')")" = true ] && echo 1)" "服务跑起来亮的是 Services 工具窗（导轨上有它的按钮），不是运行窗"
  lite_wait main "${SVC} return port('UserApplication') === ':${UP}'" 15 >/dev/null
  ok "$([ "$(lite_eval main "${SVC} return grp('UserApplication') + ' ' + port('UserApplication')")" = "Running :${UP}" ] && echo 1)" "Services 里 UserApplication 在 Running、端口 :${UP}（lsof 查的是它那个进程组）：$(lite_eval main "${SVC} return rows().map((r) => r.innerText.replace(/\n/g, ' ')).join(' / ')")"
  lite menu run-stop main >/dev/null
  ok "$(wait_for 15 "! listening ${UP} && [ \"\$(group_left ${G})\" = 0 ]" && echo 1)" "⌘F2 停的是 Services 里选中的它：端口空了、组里剩 $(group_left "${G}") 个"
  lite_wait main "${SVC} return grp('UserApplication') === 'Finished'" 10 >/dev/null
  ok "$([ "$(lite_eval main "${SVC} return grp('UserApplication')")" = Finished ] && echo 1)" "停了之后挪到 Finished"
  echo "== 在 Services 里双击它再起、点工具栏的停止"
  lite_eval main "${SVC} row('UserApplication').dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); return true" >/dev/null
  if wait_for 120 "listening ${UP}"; then
    G=$(pgid_of "${UP}"); GROUPS_SEEN+=("${G}")
    ok 1 "双击起来了：监听 ${UP}"
    lite_eval main "${SVC} row('UserApplication').click(); document.querySelector('.svc .bar button[aria-label=\"停止\"]').click(); return true" >/dev/null
    ok "$(wait_for 15 "! listening ${UP} && [ \"\$(group_left ${G})\" = 0 ]" && echo 1)" "工具栏的停止：端口空了、组里剩 $(group_left "${G}") 个"
  else
    ok 0 "双击之后 120 秒没起来：$(logtext UserApplication | tail -5 | tr '\n' ' ')"
  fi
else
  ok 0 "120 秒没起来：$(logtext UserApplication | tail -5 | tr '\n' ' ')"
fi

echo "== Maven 工具窗（照 IDEA）：Profiles、子模块嵌在父项目底下、Lifecycle 9 个阶段；Plugins / Dependencies 是真 mvn 算的"
# 树是摊平的一行行（MavenView.svelte 的 rows），深度看左边距；`under(父, 名)` 在父那一行之后、深度更深的那几行里找。
# 展开之后要等一拍再找孩子：Svelte 把 DOM 更新攒到下一拍，同一段代码里点完马上找，找到的还是旧的 DOM（第一版就这么全红的）
TREE='const rows = () => [...document.querySelectorAll(".maven .row")]; const nm = (r) => r.querySelector(".name")?.textContent; const dep = (r) => parseInt(r.style.paddingLeft); const under = (p, n) => { const rs = rows(); const i = rs.findIndex((r) => r === p); for (let j = i + 1; j < rs.length && dep(rs[j]) > dep(p); j++) if (nm(rs[j]) === n) return rs[j]; return null; }; const top = (n) => rows().find((r) => nm(r) === n); const open = async (r) => { if (r && r.getAttribute("aria-expanded") === "false") { r.querySelector(".chev").click(); await new Promise((res) => setTimeout(res, 150)); } return r; };'
TM=$(lite_now_ms)
lite_eval main "document.querySelector('button[aria-label=\"Maven\"]')?.click(); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.maven .row').length > 3" 15 >/dev/null
NEST=$(lite_eval main "${TREE} const root = top('shopdemo'); return ['shop-common','order-service','user-service'].map((n) => { const r = under(root, n); return r ? n + (dep(r) > dep(root) ? '↳' : '=') : n + '?'; }).join(',')")
ok "$([ "${NEST}" = "shop-common↳,order-service↳,user-service↳" ] && echo 1)" "三个子模块都嵌在 shopdemo 底下：${NEST}"
ok "$([ "$(lite_eval main "${TREE} return !!under(top('Profiles'), 'probe')")" = true ] && echo 1)" "Profiles 里有根 pom 声明的 probe"
PH=$(lite_eval main "${TREE} const m = await open(top('order-service')); const l = await open(under(m, 'Lifecycle')); const rs = rows(); const i = rs.indexOf(l); return rs.slice(i + 1).filter((r) => dep(r) > dep(l)).slice(0, 9).map(nm).join(',')")
ok "$([ "${PH}" = "clean,validate,compile,test,package,verify,install,site,deploy" ] && echo 1)" "Lifecycle 是 IDEA 那 9 个阶段：${PH}"
lite_eval main "${TREE} const m = top('order-service'); await open(under(m, 'Plugins')); await open(under(m, 'Dependencies')); return true" >/dev/null
lite_wait main "${TREE} return !!under(under(top('order-service'), 'Plugins'), 'compiler')" 180 >/dev/null
TP=$(( $(lite_now_ms) - TM ))
# order-service 没在自己的 <build><plugins> 里声明 spring-boot-maven-plugin：父 pom 只在 pluginManagement 里给了默认值，这个模块不用它 ——
# 所以 Plugins 里**不该有** spring-boot（IDEA 也没有）。有的话就是把 pluginManagement 算进来了
PL=$(lite_eval main "${TREE} const p = under(top('order-service'), 'Plugins'); return ['compiler','surefire','spring-boot'].map((n) => n + (under(p, n) ? '有' : '无')).join(',')")
ok "$([ "${PL}" = "compiler有,surefire有,spring-boot无" ] && echo 1)" "Plugins 是有效 POM 里真用的（不算 pluginManagement）：${PL}；从打开工具窗到插件出来 ${TP} ms（这段时间里别的节点照常能用）"
GOALS=$(lite_eval main "${TREE} const p = await open(under(under(top('order-service'), 'Plugins'), 'compiler')); return ['compiler:compile','compiler:testCompile'].filter((g) => under(p, g)).join(',')")
ok "$([ "${GOALS}" = "compiler:compile,compiler:testCompile" ] && echo 1)" "插件的目标是从本地仓库里那个插件 jar 的描述读的：${GOALS}"
DEPS=$(lite_eval main "${TREE} const d = under(top('order-service'), 'Dependencies'); return ['com.demo:shop-common:0.0.1-SNAPSHOT'].filter((n) => under(d, n)).concat(rows().filter((r) => nm(r)?.startsWith('org.springframework.boot:spring-boot-starter:')).length ? ['spring-boot-starter'] : []).join(',')")
ok "$([ "${DEPS}" = "com.demo:shop-common:0.0.1-SNAPSHOT,spring-boot-starter" ] && echo 1)" "Dependencies 里有兄弟模块和 starter：${DEPS}"

echo "== 勾 profile、开跳过测试 → 双击 order-service 的 package：命令里都带上；target 里出 jar"
lite_eval main "${TREE} under(top('Profiles'), 'probe').querySelector('input').click(); return true" >/dev/null
sleep 0.5
lite_eval main "document.querySelector('button[aria-label=\"跳过测试\"]').click(); return true" >/dev/null
sleep 0.5
ok "$(grep -q '"probe"' "${LITE_DATA}/maven-prefs.json" 2>/dev/null && grep -q '"skip_tests": true' "${LITE_DATA}/maven-prefs.json" && echo 1)" "开关按项目记在应用数据目录的 maven-prefs.json（不写进你的项目）"
lite_eval main "${TREE} const l = under(top('order-service'), 'Lifecycle'); under(l, 'package').dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); return true" >/dev/null
lite_wait main "return __lite.runs.list.some((r) => r.name === 'order-service [package]')" 10 >/dev/null
CMD=$(lite_eval main "return __lite.runs.list.find((r) => r.name === 'order-service [package]')?.command ?? ''")
ok "$(echo "${CMD}" | grep -q -- '-P probe' && echo "${CMD}" | grep -q -- '-DskipTests' && echo 1)" "命令带上了 profile 和跳过测试：${CMD}"
ok "$(wait_for 120 "logtext 'order-service__package_' | grep -q 'BUILD SUCCESS'" && echo 1)" "运行窗那一格（order-service [package]）的输出里 BUILD SUCCESS：$(logtext 'order-service__package_' | grep -E 'BUILD|ERROR' | head -2 | tr '\n' ' ')"
ok "$(ls "${P}"/order-service/target/order-service-*.jar >/dev/null 2>&1 && echo 1)" "target 里出了 jar：$(ls "${P}"/order-service/target/*.jar 2>/dev/null | xargs -n1 basename 2>/dev/null)"
TAB=$(lite_eval main "return [...document.querySelectorAll('.ptab')].map((t) => t.innerText.trim()).join(',')")
ok "$(echo "${TAB}" | grep -q 'order-service \[package\]' && echo 1)" "运行窗的标签（IDEA 的样子）：${TAB}"

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
