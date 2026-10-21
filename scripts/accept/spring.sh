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
  <!-- junit-jupiter 钉在 5.12.2：Spring Boot 4.0.1 管的是 6.0.1，可本地仓库里配套的 junit-platform-launcher 只有 1.x —— 用 6 的话 mvn 要去下载（验收不替你下载东西） -->
  <properties><java.version>17</java.version><junit-jupiter.version>5.12.2</junit-jupiter.version></properties>
  <modules><module>shop-common</module><module>order-service</module><module>user-service</module></modules>
  <profiles><profile><id>probe</id></profile></profiles>
</project>
EOF
cat > "${P}/shop-common/pom.xml" <<'EOF'
<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version></parent>
  <artifactId>shop-common</artifactId></project>
EOF
# show() 写成两行（第 4 行算、第 5 行回）：调试那段要在第 4 行停下、单步到第 5 行看到 s（输出和一行的写法一样）
money() { printf 'package com.demo.common;\npublic record Money(long cents) {\n    public String show() {\n        String s = String.format("%s%%d.%%02d", cents / 100, cents %% 100);\n        return s;\n    }\n}\n' "$1" > "${P}/shop-common/src/main/java/com/demo/common/Money.java"; }
money "¥"
for m in order user; do
  starter=spring-boot-starter; [ "${m}" = user ] && starter=spring-boot-starter-webmvc
  testdep=""; [ "${m}" = order ] && testdep='<dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><scope>test</scope></dependency>'
  cat > "${P}/${m}-service/pom.xml" <<EOF
<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version></parent>
  <artifactId>${m}-service</artifactId>
  <dependencies>
    <dependency><groupId>com.demo</groupId><artifactId>shop-common</artifactId><version>\${project.version}</version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>${starter}</artifactId></dependency>
    ${testdep}
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
# 单测（docs/IDEA.md 第 17 节）：一个用到兄弟模块的类（不用 install 就能跑）、一个故意没过
mkdir -p "${P}/order-service/src/test/java/com/demo/order"
cat > "${P}/order-service/src/test/java/com/demo/order/OrderMathTest.java" <<'EOF'
package com.demo.order;

import com.demo.common.Money;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class OrderMathTest {
    @Test
    void usesSiblingModule() {
        assertEquals(12345, new Money(12345).cents());
    }

    @Test
    void failsOnPurpose() {
        assertEquals(3, 1 + 1, "故意没过");
    }
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
  # 堆（docs/IDEA.md 第 27 节）：读 JVM 自己写的性能计数，2 秒一个点，画选中的那个。用了 ≤ 要了 ≤ 最多，三个数都对得上才算
  lite_eval main "${SVC} row('UserApplication').click(); return true" >/dev/null
  lite_wait main "return !!document.querySelector('.svc .bar .heap polyline')" 10 >/dev/null
  HEAP=$(lite_eval main "const h = document.querySelector('.svc .bar .heap'); const t = h?.title ?? ''; const n = [...t.matchAll(/(\\d+) MB/g)].map((m) => +m[1]); return (n.length === 3 && n[0] > 0 && n[0] <= n[1] && n[1] <= n[2] ? 'ok ' : 'bad ') + h?.textContent + ' | ' + t.split('\\n')[0]")
  ok "$(echo "${HEAP}" | grep -qE '^ok [0-9]+ MB \| 堆：用了' && echo 1)" "选中它，工具栏上带着它的堆和小曲线（读 JVM 自己的性能计数，不起子进程）：${HEAP}"
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

echo "== 单测（docs/IDEA.md 第 17 节）：⌃⇧R 跑整个类 → 结果树 1 过 1 没过、行号旁换成 ✓ / ✗；命令带 profile 不带跳过测试；重跑没过的只跑那一个"
TEST="${P}/order-service/src/test/java/com/demo/order/OrderMathTest.java"
lite_eval main "await __lite.tabflow.openPath('${TEST}', { preview: false }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-test-run').length === 3" 15 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-test-run').length")" = 3 ] && echo 1)" "测试类和两个测试方法旁边各一个 ▶"
lite_eval main "const t = __lite.text().split('\\n'); __lite.caret(t.findIndex((x) => x.includes('class OrderMathTest')) + 1, 1); __lite.key('Ctrl-Shift-r'); return true" >/dev/null
RUN="order-service [test com.demo.order.OrderMathTest]"
lite_wait main "return __lite.runs.list.some((r) => r.name === '${RUN}' && r.tests)" 240 >/dev/null
SUM=$(lite_eval main "return document.querySelector('.tt .sum')?.textContent.trim() ?? ''")
ok "$([ "$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${RUN}'); return !!r?.tests && r.tests.cases.map((c) => c.name + ':' + c.outcome).sort().join(',') === 'failsOnPurpose:failed,usesSiblingModule:passed'")" = true ] && echo 1)" "结果树：用到兄弟模块的那个过了（shop-common 没 install）、故意的那个没过：${SUM}"
CMD=$(lite_eval main "return __lite.runs.list.find((x) => x.name === '${RUN}')?.command ?? ''")
ok "$(echo "${CMD}" | grep -q -- '-P probe' && ! echo "${CMD}" | grep -q -- 'skipTests' && echo "${CMD}" | grep -q -- '-pl order-service -am' && echo 1)" "命令带上了 profile、没带跳过测试（Maven 窗里那个开着）：${CMD}"
lite_wait main "return document.querySelectorAll('.cm-test-run.failed').length === 2 && document.querySelectorAll('.cm-test-run.passed').length === 1" 10 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-test-run.failed').length === 2 && document.querySelectorAll('.cm-test-run.passed').length === 1")" = true ] && echo 1)" "行号旁换成结果：类和没过的那个是 ✗、过了的那个是 ✓"
lite_eval main "document.querySelector('.tt .bar button[aria-label=\"重跑没过的\"]').click(); return true" >/dev/null
RERUN="order-service [test com.demo.order.OrderMathTest#failsOnPurpose]"
lite_wait main "return __lite.runs.list.some((r) => r.name === '${RERUN}' && r.tests)" 240 >/dev/null
ok "$([ "$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${RERUN}'); return !!r?.tests && r.tests.cases.map((c) => c.name + ':' + c.outcome).join(',') === 'failsOnPurpose:failed'")" = true ] && echo 1)" "重跑没过的：任务名是「${RERUN}」，只跑了那一个"

echo "== 调试（docs/IDEA.md 第 18 节）：Services 里点虫子 → JVM 停在入口等调试器 → jdtls 里的 java-debug 挂上去 → 停在兄弟模块的断点上；变量带 toString、求值、单步、继续之后照常跑完"
MONEY="${P}/shop-common/src/main/java/com/demo/common/Money.java"
lite_eval main "await __lite.tabflow.openPath('${MONEY}', { preview: false }); await __lite.debug.breakpoints('${P}', '${MONEY}', [4]); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-bp').length === 1" 10 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-bp').length")" = 1 ] && echo 1)" "断点画在第 4 行的行号上"
lite_eval main "document.querySelector('.rail button[aria-label=\"Services\"]')?.click(); return true" >/dev/null
lite_wait main "${SVC} return !!row('OrderApplication')" 15 >/dev/null
lite_eval main "${SVC} row('OrderApplication').click(); return true" >/dev/null
sleep 0.3
lite_eval main "document.querySelector('.svc .bar button[aria-label=\"调试\"]').click(); return true" >/dev/null
# JS 里别写带逗号的花括号（`{ a: 1, b: 2 }`）：放进双引号里的 $(…) 时，macOS 自带的 bash 3.2 照样做花括号展开，
# 拆成两段残缺的 JS、页面里语法错误一句不跑，每段等满 15 秒（2026-10-20 第一次跑这段时撞见的）
DBG="const r = __lite.runs.list.find((x) => x.name === 'OrderApplication'); const d = r?.debug;"
T0=$(date +%s)
lite_wait main "${DBG} return d?.phase === 'paused' || d?.phase === 'ended'" 300 >/dev/null
ok "$([ "$(lite_eval main "${DBG} return d?.phase")" = paused ] && echo 1)" "停下了（从点虫子到停在断点 $(( $(date +%s) - T0 )) 秒，含编译和 jdtls 导入）：$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.note ?? '')")"
ok "$(lite_eval main "${DBG} return r?.command ?? ''" | grep -qE -- 'exec java -agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0 (-javaagent:[^ ]* )?-cp' && echo 1)" "命令里 java 带上了调试代理（端口 0、先停在入口等调试器）"
FR=$(lite_eval main "${DBG} return (d?.frames ?? []).slice(0, 2).map((f) => f.name + '@' + f.path.split('/').pop() + ':' + f.line).join(' | ')")
ok "$(echo "${FR}" | grep -q '^Money.show()@Money.java:4 | OrderApplication.run(String\[\])@OrderApplication.java:' && echo 1)" "停在兄弟模块（shop-common 没 install）的第 4 行，调用者是入口类：${FR}"
ok "$([ "$(lite_eval main "${SVC} return row('OrderApplication')?.querySelector('.dbg-note')?.textContent ?? ''")" = 停在断点 ] && echo 1)" "Services 那一行写着「停在断点」"
EP=$(lite_eval main "const e = await __lite.debug.exec(); return e ? e.path.split('/').pop() + ':' + e.line : 'null'")
ok "$([ "${EP}" = "Money.java:4" ] && [ "$(lite_eval main "return __lite.tabs.active?.path === '${MONEY}' && document.querySelectorAll('.cm-exec-line').length === 1")" = true ] && echo 1)" "编辑区开着 Money.java、第 4 行标成执行点：${EP}"
VARS='const vs = () => [...document.querySelectorAll(".dbg .vrow")].map((v) => v.innerText.replace(/\s+/g, " ").trim());'
lite_wait main "${VARS} return vs().some((v) => v.startsWith('this = '))" 20 >/dev/null
ok "$(lite_eval main "${VARS} return vs().join(' / ')" | grep -q 'this = Money@[0-9]* "Money\[cents=12345\]"' && echo 1)" "变量里 this 直接带着 toString（IDEA 的样子，不用再点一层）：$(lite_eval main "${VARS} return vs().join(' / ')")"
lite_eval main "const i = document.querySelector('.dbg .expr'); i.value = 'cents / 100 + 1'; i.dispatchEvent(new Event('input', { bubbles: true })); i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); return true" >/dev/null
lite_wait main "${VARS} return vs().some((v) => v.startsWith('cents / 100 + 1 = '))" 20 >/dev/null
ok "$(lite_eval main "${VARS} return vs().find((v) => v.startsWith('cents / 100 + 1 = ')) ?? ''" | grep -q '^cents / 100 + 1 = 124' && echo 1)" "求值：$(lite_eval main "${VARS} return vs().find((v) => v.startsWith('cents / 100')) ?? '没有'")"
lite menu debug-over main >/dev/null
lite_wait main "${DBG} return d?.phase === 'paused' && d.frames[0]?.line === 5" 20 >/dev/null
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + d?.frames[0]?.line + ':' + d?.reason")" = "paused:5:step" ] && echo 1)" "F8 单步跳过：停在第 5 行（reason=step）"
lite_wait main "${VARS} return vs().some((v) => v.startsWith('s = '))" 20 >/dev/null
ok "$(lite_eval main "${VARS} return vs().find((v) => v.startsWith('s = ')) ?? ''" | grep -q '^s = "CNY 123.45"' && echo 1)" "单步之后第 4 行算出来的 s 出现在变量里：$(lite_eval main "${VARS} return vs().find((v) => v.startsWith('s = ')) ?? '没有'")"
lite menu debug-resume main >/dev/null
ok "$(wait_for 60 "logtext OrderApplication | grep -q 'LITE-IDE-PROBE hello-from-yml CNY 123.45'" && echo 1)" "F9 继续：程序照常跑完，输出里有探针那行"
lite_wait main "${DBG} return d?.phase === 'ended'" 30 >/dev/null
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + (d?.note ?? '') + ':' + r?.status")" = "ended::done" ] && echo 1)" "程序退了、调试会话跟着结束，不带报错：$(lite_eval main "${DBG} return d?.phase + ':' + (d?.note ?? '') + ':' + r?.status")"
ok "$([ "$(lite_eval main "const e = await __lite.debug.exec(); return e === null && document.querySelectorAll('.cm-exec-line').length === 0")" = true ] && echo 1)" "执行点清掉了（编辑器里不再标那一行）"
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); return true" >/dev/null

echo "== 条件断点：条件由 java-debug 在目标 JVM 里算 —— 为假跑过去不停，为真照停"
# 再点一次虫子：同名的那一格换成新的一次运行，按 id 认出是新的（不然读到的是上一次已经结束的会话）
debug_again() {
  local before; before=$(lite_eval main "${DBG} return r?.id ?? 0")
  # 构建（⌘F9）会把底部切到运行窗那一格：先把 Services 调回来（导轨上点当前那个是收起，所以只在不是它的时候点）
  lite_eval main "if (document.querySelector('.panel-head .tw-name')?.textContent !== 'Services') document.querySelector('.rail button[aria-label=\"Services\"]')?.click(); return true" >/dev/null
  lite_wait main "return document.querySelector('.panel-head .tw-name')?.textContent === 'Services'" 5 >/dev/null
  lite_eval main "${SVC} row('OrderApplication').click(); return true" >/dev/null
  sleep 0.3
  lite_eval main "document.querySelector('.svc .bar button[aria-label=\"调试\"]').click(); return true" >/dev/null
  lite_wait main "${DBG} return !!r && r.id !== ${before} && (d?.phase === 'paused' || d?.phase === 'ended')" 300 >/dev/null
}
lite_eval main "await __lite.debug.cond('${P}', '${MONEY}', 4, 'cents == 1'); return true" >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-bp.cond').length")" = 1 ] && echo 1)" "带条件的断点画成另一个样子（红点里一个「?」）"
debug_again
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status")" = "ended:done" ] && echo 1)" "条件 cents == 1 不成立（cents 是 12345）：跑过第 4 行没停、照常跑完：$(lite_eval main "${DBG} return d?.phase + ':' + r?.status + ':' + (d?.note ?? '')")"
lite_eval main "await __lite.debug.cond('${P}', '${MONEY}', 4, 'cents == 12345'); return true" >/dev/null
debug_again
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")
ok "$([ "${FR}" = "paused Money.java:4" ] && echo 1)" "条件 cents == 12345 成立：停在 Money.java:4：${FR}"
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
# 条件写错了（变量名打错）：java-debug 照样停下 —— 不说一句的话，看着和「条件成立」一模一样（2026-10-20 第一次探测时就是这样）
lite_eval main "await __lite.debug.cond('${P}', '${MONEY}', 4, 'nosuch == 1'); return true" >/dev/null
debug_again
ok "$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('') + ' ' + d?.note" | grep -q '^paused Money.java:4 条件 nosuch == 1 算不了，照样停下了：编译不过：nosuch cannot be resolved to a variable$' && echo 1)" "条件写错了：照样停在第 4 行，停下时说清条件算不了、为什么：$(lite_eval main "${DBG} return d?.note")"
ok "$(lite_eval main "return document.querySelector('.dbg .where.warn')?.textContent ?? ''" | grep -q '算不了' && echo 1)" "调试器那一行用琥珀色写着这句"
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); return true" >/dev/null

echo "== 热替换（docs/IDEA.md 第 26 节）：停在断点时改方法体 → ⌘S → ⌘F9 → 换进 JVM；停着那一帧跑完旧的、下一次调用是新的、断点还在；行号变了的断点这次不停；加了方法的换不了"
# 入口类临时改成调三次 show()（12345、7、7），这一节完了改回去：前面「F9 之后照常跑完」那几条只认一次调用
ORDER="${P}/order-service/src/main/java/com/demo/order/OrderApplication.java"
cp "${ORDER}" "${ORDER}.orig"
python3 - "${ORDER}" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
a = 'new Money(12345).show()); }'
print("改了", s.count(a), "处")
s = s.replace(a, 'new Money(12345).show()); System.out.println("LITE-IDE-AGAIN " + new Money(7).show()); System.out.println("LITE-IDE-THIRD " + new Money(7).show()); }')
open(p, "w").write(s)
PY
# Money.java 在编辑器里改（人就是这么改的）：在外面写文件的话，编辑器重读是整份替换、断点被挪到第 1 行（单独的问题，不在这一节）
save_money() { lite menu save main >/dev/null; lite_wait main "return __lite.tabs.byPath('${MONEY}')?.dirty === false" 10 >/dev/null; }
# 条件 cents == 7：第一次（12345）不停，停在第二次
lite_eval main "await __lite.debug.cond('${P}', '${MONEY}', 4, 'cents == 7'); return true" >/dev/null
debug_again
ok "$(lite_eval main "${DBG} return r?.command ?? ''" | grep -q -- "-javaagent:[^ ]*lite-hotswap.jar=" && echo 1)" "调试的命令里 java 带着热替换的代理（.app 里那份 jar）"
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")
ok "$([ "${FR}" = "paused Money.java:4" ] && logtext OrderApplication | grep -q 'LITE-IDE-PROBE' && ! logtext OrderApplication | grep -q 'LITE-IDE-AGAIN' && echo 1)" "停在第二次调用（cents == 7）：${FR}"
lite_eval main "__lite.editLine(4, 'CNY ', 'HOT '); return true" >/dev/null
save_money
lite menu build-project main >/dev/null
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('热替换')" 240 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? ''")
ok "$(echo "${TOAST}" | grep -q '构建完成.*已热替换 Money（停着的那一帧跑完旧的' && echo 1)" "构建完了自动换上、一句话说完（只有 Money：入口类重编出来一字不差，不算）：${TOAST}"
lite menu debug-resume main >/dev/null
wait_for 30 "logtext OrderApplication | grep -q 'LITE-IDE-AGAIN'" >/dev/null
lite_wait main "${DBG} return d?.phase === 'paused' || d?.phase === 'ended'" 30 >/dev/null
ok "$(logtext OrderApplication | grep -q 'LITE-IDE-AGAIN CNY 0.07' && echo 1)" "停着的那一帧跑完的是旧代码（JVM 的规矩）：$(logtext OrderApplication | grep LITE-IDE-AGAIN | tail -1)"
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")
ok "$([ "${FR}" = "paused Money.java:4" ] && echo 1)" "第三次调用又停在断点上：换之前删干净、换完行号表没变就重下（不这样的话这里直接跑过去）：${FR}"
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
ok "$(logtext OrderApplication | grep -q 'LITE-IDE-THIRD HOT 0.07' && echo 1)" "下一次调用跑的是新代码，没重启：$(logtext OrderApplication | grep LITE-IDE-THIRD | tail -1)"
# 行号变了的改动（第 4 行后面插一行）：照样换上，但 Money.java 的断点这次调试不再停 —— 调试器缓存的是旧的行号表，照它下会下错地方
debug_again
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")
ok "$([ "${FR}" = "paused Money.java:4" ] && echo 1)" "（行号变了那次）又停在第二次调用：${FR}"
lite_eval main "__lite.editLine(5, '', '        s = s.replace(\"HOT\", \"SHIFT\");\n'); return true" >/dev/null
save_money
lite menu build-project main >/dev/null
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('热替换')" 240 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? ''")
ok "$(echo "${TOAST}" | grep -q '已热替换 Money。Money.java 的行号变了，里面的断点这次调试不再停，重新调试就回来' && echo 1)" "行号变了：换上了，说清断点这次不停、为什么：${TOAST}"
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
ok "$(logtext OrderApplication | grep -q 'LITE-IDE-THIRD SHIFT 0.07' && [ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status")" = "ended:done" ] && echo 1)" "第三次调用没停（断点没往错的地方下）、跑的是新代码、程序正常结束：$(logtext OrderApplication | grep LITE-IDE-THIRD | tail -1) $(lite_eval main "${DBG} return d?.phase + ':' + r?.status")"
# 换不了的：加了一个方法。JVM 一个都不换，照实说为什么、程序照跑原来的
debug_again
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")
ok "$([ "${FR}" = "paused Money.java:4" ] && echo 1)" "新的一次调试，断点照停（上一次「不再停」只管那一次）：${FR}"
lite_eval main "__lite.editLine(5, 'SHIFT', 'NEW'); __lite.editLine(8, '', '    public String extra() { return \"x\"; }\n'); return true" >/dev/null
save_money
lite menu build-project main >/dev/null
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('热替换')" 240 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? ''")
ok "$(echo "${TOAST}" | grep -q '没热替换：加了方法 —— 运行中的 JVM 只能换方法体（Money）。要生效得重新调试' && echo 1)" "加了方法：换不了，说清为什么、要怎么办：${TOAST}"
for _ in 1 2 3; do lite menu debug-resume main >/dev/null; lite_wait main "${DBG} return d?.phase === 'ended'" 10 >/dev/null && break; done
ok "$(logtext OrderApplication | grep -q 'LITE-IDE-THIRD SHIFT 0.07' && ! logtext OrderApplication | grep -q 'NEW 0.07' && echo 1)" "程序照跑原来的（一个类都没换，不是换了一半）：$(logtext OrderApplication | grep LITE-IDE-THIRD | tail -1)"
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); return true" >/dev/null
mv "${ORDER}.orig" "${ORDER}"
money "CNY "

echo "== 调试单测：⌃⇧D 调试光标所在的那个方法 → JDWP 交给 surefire fork 的 JVM → 停在测试里的断点上；求值用得到兄弟模块的类；F9 之后测试照常跑完、结果树出来"
# 第 11 行是 usesSiblingModule 里那句 assertEquals
lite_eval main "await __lite.tabflow.openPath('${TEST}', { preview: false }); await __lite.debug.breakpoints('${P}', '${TEST}', [11]); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${TEST}' && document.querySelectorAll('.cm-bp').length === 1" 10 >/dev/null
lite_eval main "__lite.caret(11, 9); __lite.key('Ctrl-Shift-d'); return true" >/dev/null
TRUN="order-service [test com.demo.order.OrderMathTest#usesSiblingModule]"
TDBG="const r = __lite.runs.list.find((x) => x.name === '${TRUN}'); const d = r?.debug;"
T0=$(date +%s)
lite_wait main "${TDBG} return d?.phase === 'paused' || d?.phase === 'ended'" 300 >/dev/null
ok "$(lite_eval main "${TDBG} return r?.command ?? ''" | grep -qE -- "^export JAVA_HOME=[^;]*temurin-17[^;]*; mvn '-Dmaven.surefire.debug=-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0( -javaagent:[^ ']*)?' .*-Dtest='com.demo.order.OrderMathTest#usesSiblingModule'" && echo 1)" "⌃⇧D 起的是那一个方法、调试代理交给 surefire：$(lite_eval main "${TDBG} return (r?.command ?? '没有').slice(0, 90)")"
FR=$(lite_eval main "${TDBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.name + '@' + f.path.split('/').pop() + ':' + f.line).join('')")
ok "$(echo "${FR}" | grep -q '^paused OrderMathTest.usesSiblingModule()@OrderMathTest.java:11$' && echo 1)" "停在测试里的断点上（从按键到停下 $(( $(date +%s) - T0 )) 秒）：${FR}"
lite_wait main "${VARS} return !!document.querySelector('.dbg .expr')" 10 >/dev/null
lite_eval main "const i = document.querySelector('.dbg .expr'); i.value = 'new Money(7).cents() + 1'; i.dispatchEvent(new Event('input', { bubbles: true })); i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); return true" >/dev/null
lite_wait main "${VARS} return vs().some((v) => v.startsWith('new Money(7).cents() + 1 = '))" 20 >/dev/null
ok "$(lite_eval main "${VARS} return vs().find((v) => v.startsWith('new Money(7).cents() + 1 = ')) ?? ''" | grep -q '^new Money(7).cents() + 1 = 8' && echo 1)" "求值用得到兄弟模块的 Money（项目名用模块的 artifactId 是对的）：$(lite_eval main "${VARS} return vs().find((v) => v.startsWith('new Money')) ?? document.querySelector('.dbg .eval-err')?.textContent ?? '没有'")"
lite menu debug-resume main >/dev/null
lite_wait main "${TDBG} return r?.status !== 'running' && !!r?.tests && d?.phase === 'ended'" 120 >/dev/null
ok "$([ "$(lite_eval main "${TDBG} return r?.tests?.cases.map((c) => c.name + ':' + c.outcome).join(',') + ' ' + d?.phase + ':' + (d?.note ?? '')")" = "usesSiblingModule:passed ended:" ] && echo 1)" "F9 之后跑完：只跑了那一个、过了，会话正常结束：$(lite_eval main "${TDBG} return (r?.tests?.cases.map((c) => c.name + ':' + c.outcome).join(',') ?? '没有结果') + ' ' + d?.phase + ':' + (d?.note ?? '')")"
ok "$([ "$(lite_eval main "return [...document.querySelectorAll('.dbg .segs button')].find((b) => b.getAttribute('aria-selected') === 'true')?.textContent + ':' + !!document.querySelector('.tt')?.offsetParent")" = "控制台:true" ] && echo 1)" "结束后回到控制台那页、结果树看得见"
lite_eval main "await __lite.debug.breakpoints('${P}', '${TEST}', []); return true" >/dev/null

echo "== 构建（⌘F9，docs/IDEA.md 第 22 节）：每个 reactor 一遍 test-compile；编译错误按文件列出来（javac 的话是中文的也认）、同一条只列一次；改好了再构建说用了几秒"
cp "${MONEY}" "${P}/Money.java.bak"
python3 -c 'import sys; p=sys.argv[1]; s=open(p).read(); open(p,"w").write(s.replace("return s;", "return s + nosuch;"))' "${MONEY}"
B="const r = __lite.runs.list.find((x) => x.name === '构建项目');"
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.status !== 'running' && !!r.problems" 240 >/dev/null
ok "$([ "$(lite_eval main "${B} return r?.command.replace(/^export [^;]*; /, '') + '|' + r?.status")" = "mvn -B -ntp -P probe test-compile|failed" ] && echo 1)" "菜单「构建项目」：带上勾的 profile 跑 test-compile、失败了（JDK 那段见下一节）：$(lite_eval main "${B} return r?.command + '|' + r?.status")"
ok "$(lite_eval main "${B} const p = r?.problems?.items ?? []; return p.length + ' ' + p.map((x) => x.path.split('/').pop() + ':' + x.line + ':' + x.message.split('\\n')[0]).join(',')" | grep -q '^1 Money.java:5:' && echo 1)" "错误树里正好一条、在 Money.java 第 5 行（Maven 把它打了两遍）：$(lite_eval main "${B} const p = r?.problems?.items ?? []; return p.length + ' ' + p.map((x) => x.path.split('/').pop() + ':' + x.line + ':' + x.message.split('\\n')[0]).join(',')")"
mv "${P}/Money.java.bak" "${MONEY}"; touch "${MONEY}"
# 上一次「构建失败」那条提示没读完时，做成了的不会把它顶掉（ui.md 第三条「坏消息不被好消息顶掉」）—— 等它自己走了再构建，像人看完了那样
lite_wait main "return !document.querySelector('.toast')" 15 >/dev/null
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.status === 'done' && r.problems?.items.length === 0" 240 >/dev/null
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('构建完成')" 10 >/dev/null
ok "$([ "$(lite_eval main "${B} return r?.status + ':' + r?.problems?.items.length + ':' + (document.querySelector('.bt .sum')?.textContent?.trim() ?? '')")" = "done:0:构建成功" ] && lite_eval main "return document.querySelector('.toast')?.textContent ?? ''" | grep -q '构建完成，用了' && echo 1)" "改好了再构建：成功、没有错误，说了「$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? '没有'")」"

echo "== 按项目挑 JDK（docs/IDEA.md 第 23 节）：pom 要 21、你默认的是 17 —— 用 21 编；运行、单测、调试、构建都一样，写在命令最前面"
J21=$(ls -d /Library/Java/JavaVirtualMachines/*21*/Contents/Home 2>/dev/null | head -1)
if [ -z "${J21}" ]; then
  skip "这台机器上没有 JDK 21"
else
  cp "${P}/pom.xml" "${P}/pom.xml.bak"
  python3 -c 'import sys; p=sys.argv[1]; s=open(p).read(); open(p,"w").write(s.replace("<java.version>17</java.version>", "<java.version>21</java.version>"))' "${P}/pom.xml"
  lite_wait main "return !document.querySelector('.toast')" 15 >/dev/null
  # 重新构建（先 clean）：只改了 pom 的话 test-compile 的增量判断可能说「不用编」，输出里就没有 release 21
  RB="const r = __lite.runs.list.find((x) => x.name === '重新构建项目');"
  lite menu rebuild-project main >/dev/null
  lite_wait main "${RB} return !!r && r.status !== 'running'" 240 >/dev/null
  ok "$(lite_eval main "${RB} return r?.command ?? ''" | grep -q "^export JAVA_HOME=[^;]*21[^;]*; mvn" && echo 1)" "命令最前面换成了 JDK 21：$(lite_eval main "${RB} return (r?.command ?? '').split(';')[0]")"
  ok "$([ "$(lite_eval main "${RB} return r?.status")" = done ] && logtext 重新构建项目 | grep -q 'release 21' && echo 1)" "用 21 编过了（输出里是 release 21；用你默认的 17 编会报「不支持发行版本 21」）：$(lite_eval main "${RB} return r?.status") $(logtext 重新构建项目 | grep -o 'release [0-9]*' | head -1)"
  mv "${P}/pom.xml.bak" "${P}/pom.xml"; touch "${P}/pom.xml"
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
