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
REPO=$("${SHELL}" -ilc 'mvn -o -q help:evaluate -Dexpression=settings.localRepository -DforceStdout' </dev/null 2>/dev/null | tail -1)

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
mkdir -p "${P}/.mvn"
# 验收只用本机缓存，Maven 3.8 不认 MAVEN_ARGS，用临时项目的配置保证任务里的 mvn 也不上网下载。
printf '%s\n' '-o' > "${P}/.mvn/maven.config"
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
# run() 里那句 parseInt 抛了又接住：异常断点（docs/TAKEOVER.md G14）要有一个「被捕获的」异常停。写在同一行，别处按行号认的断言不受影响
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
    public static void main(String[] args) { System.out.println("LITE-IDE-CFG " + System.getProperty("lite.vm") + " " + System.getenv("LITE_ENV") + " " + String.join(",", args) + " " + System.getProperty("spring.profiles.active")); SpringApplication.run(OrderApplication.class, args); }
    @Override public void run(String... args) { try { Integer.parseInt("lite"); } catch (NumberFormatException e) { } System.out.println("LITE-IDE-PROBE " + greeting + " " + new Money(12345).show()); }
}
EOF
# 单测（docs/IDEA.md 第 17 节）：一个用到兄弟模块的类（不用 install 就能跑）、一个故意没过
mkdir -p "${P}/order-service/src/test/java/com/demo/order"
# 边跑边出（G15）用：两个各跑 5 秒的类 —— surefire 先跑哪个不一定（按目录顺序），各 5 秒的话不管谁先，中间都有 5 秒「一个出了、一个还在跑」
for c in LiveATest LiveBTest; do
  printf 'package com.demo.order;\n\nimport org.junit.jupiter.api.Test;\n\nclass %s {\n    @Test\n    void waits() throws Exception { Thread.sleep(5000); }\n}\n' "$c" > "${P}/order-service/src/test/java/com/demo/order/$c.java"
done
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
    public static void main(String[] args) { System.out.println("LITE-IDE-IDEA " + System.getProperty("lite.msg") + "|" + System.getProperty("lite.over") + "|" + System.getenv("LITE_IDEA_ENV") + "|" + System.getProperty("spring.profiles.active")); SpringApplication.run(UserApplication.class, args); }
}
EOF
printf 'server:\n  port: %s\n' "${UP}" > "${P}/user-service/src/main/resources/application.yml"
cat > "${P}/user-service/src/main/java/com/demo/user/TraceController.java" <<'EOF'
package com.demo.user;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.http.ResponseEntity;
@RestController
public class TraceController {
    @GetMapping("/trace") public ResponseEntity<String> trace() {
        System.out.println("INFO trace=spring-trace-7 completed");
        System.out.println("INFO trace=other-request completed");
        return ResponseEntity.ok().header("X-Trace-Id", "spring-trace-7").body("{}");
    }
}
EOF
printf '### 服务链路\nGET http://127.0.0.1:%s/trace\n' "${UP}" > "${P}/trace.http"

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
  ok "$(echo "${HEAP}" | grep -qE '^ok [0-9]+ MB \| 堆：已用' && echo 1)" "选中它，工具栏上带着它的堆和小曲线（读 JVM 自己的性能计数，不起子进程）：${HEAP}"
  echo "== HTTP 响应头跳回 Spring 服务日志（#77）"
  lite_eval main "await __lite.tabflow.openPath('${P}/trace.http', { preview: false }); return true" >/dev/null
  lite_wait main "return document.querySelectorAll('.cm-http-send').length === 1" 10 >/dev/null
  lite_eval main "__lite.caret(2, 1); __lite.key('Ctrl-Shift-r'); return true" >/dev/null
  lite_wait main "return __lite.runs.list.find((r) => r.name === '服务链路')?.status === 'done'" 15 >/dev/null
  lite_eval main "[...document.querySelectorAll('.http [role=tab]')].find((x) => x.textContent.startsWith('响应头'))?.click(); return true" >/dev/null
  lite_wait main "return !!document.querySelector('.http .hrow button')" 8 >/dev/null
  lite_eval main "document.querySelector('.http .hrow button')?.click(); return true" >/dev/null
  ok "$(lite_wait main "${SVC} return document.querySelector('.panel-head .tw-name')?.textContent === 'Services' && row('UserApplication')?.getAttribute('aria-selected') === 'true' && document.querySelector('.svc .run .search input')?.value === '\"spring-trace-7\"'" 10 && echo 1)" "跳回 Services、选中 UserApplication、带上完整链路 ID"
  ok "$(lite_wait main "const t = document.querySelector('.svc .run .body')?.textContent ?? ''; return t.includes('trace=spring-trace-7') && !t.includes('trace=other-request')" 10 && echo 1)" "服务日志只显示这条链路"
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
# 跑完才算（G15 起跑着的时候 tests 就有了、是半截的）
lite_wait main "return __lite.runs.list.some((r) => r.name === '${RUN}' && r.tests && r.status !== 'running' && r.status !== 'stopping')" 240 >/dev/null
SUM=$(lite_eval main "return document.querySelector('.tt .sum')?.textContent.trim() ?? ''")
ok "$([ "$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${RUN}'); return !!r?.tests && r.tests.cases.map((c) => c.name + ':' + c.outcome).sort().join(',') === 'failsOnPurpose:failed,usesSiblingModule:passed'")" = true ] && echo 1)" "结果树：用到兄弟模块的那个过了（shop-common 没 install）、故意的那个没过：${SUM}"
CMD=$(lite_eval main "return __lite.runs.list.find((x) => x.name === '${RUN}')?.command ?? ''")
ok "$(echo "${CMD}" | grep -q -- '-P probe' && ! echo "${CMD}" | grep -q -- 'skipTests' && echo "${CMD}" | grep -q -- '-pl order-service -am' && echo 1)" "命令带上了 profile、没带跳过测试（Maven 窗里那个开着）：${CMD}"
lite_wait main "return document.querySelectorAll('.cm-test-run.failed').length === 2 && document.querySelectorAll('.cm-test-run.passed').length === 1" 10 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-test-run.failed').length === 2 && document.querySelectorAll('.cm-test-run.passed').length === 1")" = true ] && echo 1)" "行号旁换成结果：类和没过的那个是 ✗、过了的那个是 ✓"
lite_eval main "document.querySelector('.tt .bar button[aria-label=\"重新运行失败的测试\"]').click(); return true" >/dev/null
RERUN="order-service [test com.demo.order.OrderMathTest#failsOnPurpose]"
# 跑完才算（G15 起跑着的时候 tests 就有了、是半截的）
lite_wait main "return __lite.runs.list.some((r) => r.name === '${RERUN}' && r.tests && r.status !== 'running' && r.status !== 'stopping')" 240 >/dev/null
ok "$([ "$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${RERUN}'); return !!r?.tests && r.tests.cases.map((c) => c.name + ':' + c.outcome).join(',') === 'failsOnPurpose:failed'")" = true ] && echo 1)" "重跑没过的：任务名是「${RERUN}」，只跑了那一个"

echo "== 单测边跑边出（docs/TAKEOVER.md G15）：两个各跑 5 秒的类，第一个跑完、第二个还在跑的时候，结果树里已经有第一个"
LIVE="order-service [test com.demo.order.LiveATest,com.demo.order.LiveBTest]"
lite_eval main "__lite.runTask('${LIVE}'); return true" >/dev/null
lite_wait main "const r = __lite.runs.list.find((x) => x.name === '${LIVE}'); return !!r && (r.tests?.cases.length ?? 0) > 0" 240 >/dev/null
MID=$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${LIVE}'); return (r?.status ?? '无') + '|' + (r?.tests?.cases.length ?? 0) + '|' + (r?.tests?.cases.map((c) => c.class.split('.').pop() + ':' + c.outcome).join(',') ?? '') + '|' + (document.querySelector('.tt .sum')?.textContent.trim() ?? '')")
ok "$([ "${MID%%|*}" = running ] && [ "$(echo "${MID}" | cut -d'|' -f2)" = 1 ] && echo "${MID}" | cut -d'|' -f3 | grep -qE '^Live[AB]Test:passed$' && echo 1)" "还在跑的时候结果树里已经有一个类的结果、是这次的（上一次跑的 OrderMathTest 不混进来）：${MID}"
lite_wait main "const r = __lite.runs.list.find((x) => x.name === '${LIVE}'); return r?.status !== 'running' && r?.tests?.cases.length === 2" 120 >/dev/null
ok "$([ "$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${LIVE}'); return r?.status !== 'running' && r?.tests?.cases.length === 2 && r.tests.cases.every((c) => c.outcome === 'passed')")" = true ] && echo 1)" "跑完：两个类都在、都过了"

echo "== 覆盖率（docs/TAKEOVER.md G15）：带覆盖率跑 OrderMathTest（JaCoCo 的代理挂在 surefire fork 的 JVM 上）→ 覆盖率工具窗列出兄弟模块的 Money.java，编辑器里跑到的行绿、没跑到的红"
COV="order-service [cover com.demo.order.OrderMathTest]"
lite_eval main "__lite.runTask('${COV}'); return true" >/dev/null
lite_wait main "const r = __lite.runs.list.find((x) => x.name === '${COV}'); return r?.status !== 'running' && !!r?.tests" 240 >/dev/null
CMD=$(lite_eval main "return __lite.runs.list.find((x) => x.name === '${COV}')?.command ?? ''")
ok "$(echo "${CMD}" | grep -q "maven.surefire.debug=-javaagent:.*jacocoagent.jar=destfile=" && [ "$(lite_eval main "const r = __lite.runs.list.find((x) => x.name === '${COV}'); return r.tests.cases.map((c) => c.name + ':' + c.outcome).sort().join(',')")" = "failsOnPurpose:failed,usesSiblingModule:passed" ] && echo 1)" "代理挂在 surefire fork 的 JVM 上（借 maven.surefire.debug、不碰 argLine），测试结果照旧 1 过 1 没过"
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('行覆盖率')" 60 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''")
lite menu tw-coverage main >/dev/null
lite_wait main "return [...document.querySelectorAll('.cov .row .name')].some((e) => e.textContent === 'Money.java')" 15 >/dev/null
ROWS=$(lite_eval main "return [...document.querySelectorAll('.cov .row')].map((r) => r.querySelector('.name').textContent + ' ' + r.querySelector('.pct').textContent).join(', ')")
ok "$(echo "${ROWS}" | grep -q 'Money.java' && echo "${TOAST}" | grep -q '行覆盖率' && echo 1)" "跑完胶囊说行覆盖率（${TOAST}）；覆盖率工具窗列出兄弟模块 shop-common 的 Money.java：${ROWS}"
lite_eval main "await __lite.tabflow.openPath('${P}/shop-common/src/main/java/com/demo/common/Money.java', { preview: false }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-lineNumbers .cm-cov').length > 0" 15 >/dev/null
GUT=$(lite_eval main "return [...document.querySelectorAll('.cm-lineNumbers .cm-cov')].map((e) => e.textContent.trim() + ':' + (e.classList.contains('cm-cov-hit') ? 'hit' : e.classList.contains('cm-cov-miss') ? 'miss' : 'part')).join(',')")
ok "$(echo ",${GUT}," | grep -q ',2:hit,' && echo ",${GUT}," | grep -q ',4:miss,' && echo 1)" "Money.java：测试用到的 record 那行（第 2 行）绿、没调到的 show() 里（第 4 行）红：${GUT}"
lite_eval main "document.querySelector('.cov .bar button')?.click(); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-lineNumbers .cm-cov').length === 0" 5 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-lineNumbers .cm-cov').length")" = 0 ] && echo 1)" "工具窗里「清掉」：编辑器里的颜色一起收了"
lite menu tw-coverage main >/dev/null

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
ok "$([ "$(lite_eval main "${SVC} return row('OrderApplication')?.querySelector('.dbg-note')?.textContent ?? ''")" = 已在断点处暂停 ] && echo 1)" "Services 那一行写着「停在断点」"
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
ok "$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('') + ' ' + d?.note" | grep -q '^paused Money.java:4 条件 nosuch == 1 无法求值，已暂停：编译错误：nosuch cannot be resolved to a variable$' && echo 1)" "条件写错了：照样停在第 4 行，停下时说清条件算不了、为什么：$(lite_eval main "${DBG} return d?.note")"
ok "$(lite_eval main "return document.querySelector('.dbg .where.warn')?.textContent ?? ''" | grep -q '无法求值' && echo 1)" "调试器那一行用琥珀色写着这句"
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); return true" >/dev/null

echo "== 断点属性（docs/TAKEOVER.md G14）：日志断点不停、打一句进运行窗的输出；命中次数没到不停；异常断点停在被接住的那个异常上、说出抛的是什么"
lite_eval main "await __lite.debug.bp('${P}', '${MONEY}', { line: 4, log: 'cents={cents} 进来了' }); return true" >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-bp.log').length")" = 1 ] && echo 1)" "日志断点画成另一种颜色（橙点）"
debug_again
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status")" = "ended:done" ] && echo 1)" "日志断点：跑过第 4 行没停、照常跑完：$(lite_eval main "${DBG} return d?.phase + ':' + r?.status + ':' + (d?.note ?? '')")"
ok "$(logtext OrderApplication | grep -q '^\[日志断点\] cents=12345 进来了$' && echo 1)" "运行窗的输出里多了一行「[日志断点] cents=12345 进来了」（{cents} 换成了值）：$(logtext OrderApplication | grep '日志断点' | tail -1)"
ok "$(! logtext OrderApplication | grep -q '.\[日志断点\]' && echo 1)" "插在行首、自成一行（没把程序的一行劈开）"
lite_eval main "await __lite.debug.bp('${P}', '${MONEY}', { line: 4, hit: '2' }); return true" >/dev/null
debug_again
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status")" = "ended:done" ] && echo 1)" "命中次数 2：第 4 行只走到一次，没停、照常跑完（命中次数真带下去了 —— 不带的话第一次就停）"
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); await __lite.debug.exc('${P}', false, true, ['java.lang.NumberFormatException']); return true" >/dev/null
debug_again
EXC=$(lite_eval main "${DBG} return d?.phase + ' ' + d?.reason + ' ' + (d?.note ?? '')")
ok "$(echo "${EXC}" | grep -q '^paused exception 抛出 java.lang.NumberFormatException：For input string: "lite"$' && echo 1)" "异常断点（被接住的、只看 NumberFormatException）：停下了、说出抛的是什么：${EXC}"
ok "$(lite_eval main "${DBG} return (d?.frames ?? []).map((f) => f.name).join(' | ')" | grep -q 'OrderApplication.run' && echo 1)" "调用栈里有抛它的那一处（OrderApplication.run）"
lite_eval main "await __lite.debug.exc('${P}', false, false, []); return true" >/dev/null
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status")" = "ended:done" ] && echo 1)" "关掉异常断点、F9：照常跑完（调试中改了当场生效 —— 不然 Spring 启动里别的地方还会停）"

echo "== 运行到光标 ⌥F9、行内变量值、监视（docs/TAKEOVER.md G14）：停在第 4 行 → 运行到第 5 行；第 4 行行尾写着 s 的值；监视里的表达式每停一次在当前帧上算"
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', [4]); return true" >/dev/null
debug_again
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + d?.frames[0]?.line")" = "paused:4" ] && echo 1)" "停在第 4 行"
lite_eval main "await __lite.tabflow.openPath('${MONEY}', { preview: false }); __lite.caret(5, 9); return true" >/dev/null
lite menu debug-run-to main >/dev/null
lite_wait main "${DBG} return d?.phase === 'paused' && d.frames[0]?.line === 5" 20 >/dev/null
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + d?.frames[0]?.line")" = "paused:5" ] && echo 1)" "运行到光标：停在第 5 行（没在那儿下断点）"
ok "$([ "$(lite_eval main "return (await __lite.debug.lines('${P}', '${MONEY}')).join()")" = 4 ] && echo 1)" "断点表还是原来那一个（第 5 行的是临时的，不留下）"
lite_wait main "return [...document.querySelectorAll('.cm-inline-val')].some((e) => e.textContent.startsWith('s = '))" 15 >/dev/null
INL=$(lite_eval main "return [...document.querySelectorAll('.cm-line')].map((l, i) => [...l.querySelectorAll('.cm-inline-val')].map((e) => (i + 1) + ': ' + e.textContent).join('')).filter(Boolean).join(' | ')")
ok "$([ "${INL}" = '4: s = "CNY 123.45" | 5: s = "CNY 123.45"' ] && echo 1)" "行内变量值：第 4、5 行（到执行点为止）行尾写着 s 的值；cents 是 record 的字段、不是局部变量，不写：${INL}"
lite_eval main "const i = document.querySelector('.dbg .expr'); i.value = 'cents * 2'; i.dispatchEvent(new Event('input', { bubbles: true })); i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true })); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('.dbg .watch .vrow')].some((v) => v.innerText.includes('24690'))" 20 >/dev/null
ok "$(lite_eval main "return [...document.querySelectorAll('.dbg .watch .vrow')].map((v) => v.innerText.replace(/\s+/g, ' ')).join(' / ')" | grep -q '^cents \* 2 = 24690' && echo 1)" "⇧↵ 加进监视：cents * 2 = 24690：$(lite_eval main "return [...document.querySelectorAll('.dbg .watch .vrow')].map((v) => v.innerText.replace(/\s+/g, ' ')).join(' / ')")"
lite_eval main "document.querySelector('.dbg .watch .wx').click(); return true" >/dev/null
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
ok "$([ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status + ':' + document.querySelectorAll('.cm-inline-val').length")" = "ended:done:0" ] && echo 1)" "F9 跑完：行内的值跟着执行点一起清掉"
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); return true" >/dev/null

echo "== 运行配置（docs/SETTINGS.md 13.6）：「运行 › 编辑配置…」里给 OrderApplication 配 profile、VM 参数、环境变量、程序参数 → 起它时全带上（下面热替换那段就带着这份配置调试）"
RC='const d = document.querySelector(".dialog.runcfg"); const put = (el, v) => { el.value = v; el.dispatchEvent(new Event("input", { bubbles: true })); }; const tick = () => new Promise((r) => setTimeout(r, 80));'
lite menu run-configs main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.runcfg #rc-profiles')" 30 >/dev/null
lite_eval main "${RC} [...d.querySelectorAll('.svc')].find((b) => b.textContent.trim() === 'OrderApplication').click(); await tick(); put(d.querySelector('#rc-profiles'), 'cfgprof'); await tick(); put(d.querySelector('#rc-vm'), '-Dlite.vm=vm1'); await tick(); put(d.querySelector('#rc-args'), '--lite.arg=a1'); await tick(); [...d.querySelectorAll('.env .btn')].find((b) => b.textContent.trim().endsWith('添加')).click(); await tick(); const kv = d.querySelectorAll('.kv input'); put(kv[0], 'LITE_ENV'); await tick(); put(kv[1], 'env1'); await tick(); d.querySelector('[data-default]').click(); return true" >/dev/null
lite_wait main "return !document.querySelector('.dialog.runcfg')" 10 >/dev/null
pick OrderApplication >/dev/null
ok "$(wait_for 120 "logtext OrderApplication | grep -q 'LITE-IDE-CFG vm1 env1 --lite.arg=a1 cfgprof'" && echo 1)" "起来的 JVM 拿到了 VM 参数、环境变量、程序参数、Spring profile：$(logtext OrderApplication | grep LITE-IDE-CFG | tail -1)"
ok "$(lite_eval main "return __lite.runs.list.find((x) => x.name === 'OrderApplication')?.command ?? ''" | grep -q -- "export LITE_ENV=env1 && exec java -Dlite.vm=vm1 -Dspring.profiles.active=cfgprof -cp .* com.demo.order.OrderApplication --lite.arg=a1$" && echo 1)" "命令照 13.6 的样子拼：环境变量在 exec 前、VM 参数和 profile 在 java 后、程序参数在主类后"
ok "$([ -z "$(ls -A "${P}/.lite-ide" 2>/dev/null)" ] && ! grep -rq 'cfgprof' "${P}" --include='*.json' --include='*.xml' && echo 1)" "运行配置没往项目里写（只有你自己的，同 IDEA 的 workspace.xml）"

echo "== 热替换（docs/IDEA.md 第 26 节）：停在断点时改方法体 → ⌘S → 自己构建、换进 JVM（TAKEOVER.md I6：调试时存盘就自动构建，不用再按 ⌘F9）；停着那一帧跑完旧的、下一次调用是新的、断点还在；行号变了的断点这次不停；加了方法的换不了"
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
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('热替换')" 240 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? ''")
ok "$(echo "${TOAST}" | grep -q '构建完成.*已热替换 Money（当前暂停的栈帧仍执行旧代码' && echo 1)" "构建完了自动换上、一句话说完（只有 Money：入口类重编出来一字不差，不算）：${TOAST}"
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
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('热替换')" 240 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? ''")
ok "$(echo "${TOAST}" | grep -q '已热替换 Money。Money.java 的行号已变化，其中的断点在本次调试中不再生效，重新调试后恢复' && echo 1)" "行号变了：换上了，说清断点这次不停、为什么：${TOAST}"
lite menu debug-resume main >/dev/null
lite_wait main "${DBG} return d?.phase === 'ended'" 60 >/dev/null
ok "$(logtext OrderApplication | grep -q 'LITE-IDE-THIRD SHIFT 0.07' && [ "$(lite_eval main "${DBG} return d?.phase + ':' + r?.status")" = "ended:done" ] && echo 1)" "第三次调用没停（断点没往错的地方下）、跑的是新代码、程序正常结束：$(logtext OrderApplication | grep LITE-IDE-THIRD | tail -1) $(lite_eval main "${DBG} return d?.phase + ':' + r?.status")"
# 换不了的：加了一个方法。JVM 一个都不换，照实说为什么、程序照跑原来的
debug_again
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")
ok "$([ "${FR}" = "paused Money.java:4" ] && echo 1)" "新的一次调试，断点照停（上一次「不再停」只管那一次）：${FR}"
lite_eval main "__lite.editLine(5, 'SHIFT', 'NEW'); __lite.editLine(8, '', '    public String extra() { return \"x\"; }\n'); return true" >/dev/null
save_money
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('热替换')" 240 >/dev/null
TOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? ''")
ok "$(echo "${TOAST}" | grep -q '热替换失败：加了方法 —— 运行中的 JVM 只能换方法体（Money）。需重新调试才能生效' && echo 1)" "加了方法：换不了，说清为什么、要怎么办：${TOAST}"
for _ in 1 2 3; do lite menu debug-resume main >/dev/null; lite_wait main "${DBG} return d?.phase === 'ended'" 10 >/dev/null && break; done
ok "$(logtext OrderApplication | grep -q 'LITE-IDE-THIRD SHIFT 0.07' && ! logtext OrderApplication | grep -q 'NEW 0.07' && echo 1)" "程序照跑原来的（一个类都没换，不是换了一半）：$(logtext OrderApplication | grep LITE-IDE-THIRD | tail -1)"
# 设置里关掉（java.hotswapOnSave）：调试着存盘也不再自动构建
lite_eval main "await window.__TAURI_INTERNALS__.invoke('settings_apply', { root: __lite.project.root, global: { 'java.hotswapOnSave': false }, project: null }); return true" >/dev/null
lite_wait main "return __lite.settings.v.javaHotswapOnSave === false" 10 >/dev/null
debug_again
B0=$(lite_eval main "return __lite.runs.list.filter((x) => x.name === '构建项目').map((x) => x.id).join()")
lite_eval main "__lite.editLine(5, 'NEW', 'OFF'); return true" >/dev/null
save_money
sleep 3
ok "$([ "$(lite_eval main "return __lite.runs.list.filter((x) => x.name === '构建项目').map((x) => x.id).join()")" = "${B0}" ] && echo 1)" "设置里关掉 java.hotswapOnSave：调试着存盘，等了 3 秒也没起构建"
lite_eval main "await window.__TAURI_INTERNALS__.invoke('settings_apply', { root: __lite.project.root, global: { 'java.hotswapOnSave': null }, project: null }); return true" >/dev/null
for _ in 1 2 3; do lite menu debug-resume main >/dev/null; lite_wait main "${DBG} return d?.phase === 'ended'" 10 >/dev/null && break; done
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
TCLICK=$(python3 -c 'import time; print(int(time.time() * 1000))')
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.status !== 'running' && !!r.problems" 240 >/dev/null
# 卡在哪一步（2026-10-28 抓到 Rust 真起进程是在等满 240 秒那一刻）：前端起任务那条路的时间线、Rust 那边命令进来 / 进阻塞池 / 起进程的时刻，都是 Unix 毫秒
TRACE=$(lite_eval main "return (await __lite.runTrace()).slice(-8).map((x) => x[0] + ' ' + x[1]).join(' / ')" 2>&1)
RUSTLOG=$(grep -E '@[0-9]+ (task_run|run_compound_items)' "${LITE_LOG}" 2>/dev/null | tail -8 | tr '\n' '/')
ok "$([ "$(lite_eval main "${B} return r?.command.replace(/^export [^;]*; /, '') + '|' + r?.status")" = "mvn -B -ntp -P probe test-compile|failed" ] && echo 1)" "菜单「构建项目」：带上勾的 profile 跑 test-compile、失败了（JDK 那段见下一节）：$(lite_eval main "${B} return r?.command + '|' + r?.status")$([ "$(lite_eval main "${B} return r?.status")" = running ] && printf ' —— 240 秒还在跑，输出最后几行：%s；那时的进程（找卡在哪）：%s' "$(logtext '构建项目' | tail -4 | tr '\n' ' ')" "$(ps -axo pid,ppid,pgid,lstart,stat,command | grep -E 'java|mvn|zsh' | grep -v grep | cut -c1-200 | tr '\n' ';')；日志 $(lite_eval main "${B} return r?.log ?? ''" | xargs -I{} sh -c 'ls -l "{}" 2>&1' | cut -c1-200)；现在 $(date '+%H:%M:%S')；点菜单 @${TCLICK}；前端：${TRACE}；Rust：${RUSTLOG}")"
ok "$(lite_eval main "${B} const p = r?.problems?.items ?? []; return p.length + ' ' + p.map((x) => x.path.split('/').pop() + ':' + x.line + ':' + x.message.split('\\n')[0]).join(',')" | grep -q '^1 Money.java:5:' && echo 1)" "错误树里正好一条、在 Money.java 第 5 行（Maven 把它打了两遍）：$(lite_eval main "${B} const p = r?.problems?.items ?? []; return p.length + ' ' + p.map((x) => x.path.split('/').pop() + ':' + x.line + ':' + x.message.split('\\n')[0]).join(',')")"
mv "${P}/Money.java.bak" "${MONEY}"; touch "${MONEY}"
# 上一次「构建失败」那条提示没读完时，做成了的不会把它顶掉（ui.md 第三条「坏消息不被好消息顶掉」）—— 等它自己走了再构建，像人看完了那样
lite_wait main "return !document.querySelector('.toast')" 15 >/dev/null
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.status === 'done' && r.problems?.items.length === 0" 240 >/dev/null
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('构建完成')" 10 >/dev/null
ok "$([ "$(lite_eval main "${B} return r?.status + ':' + r?.problems?.items.length + ':' + (document.querySelector('.bt .sum')?.textContent?.trim() ?? '')")" = "done:0:构建成功" ] && lite_eval main "return document.querySelector('.toast')?.textContent ?? ''" | grep -q '构建完成，耗时' && echo 1)" "改好了再构建：成功、没有错误，说了「$(lite_eval main "return document.querySelector('.toast')?.textContent?.trim() ?? '没有'")」"

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
  # 设置 › 此项目 里明确选一个 JDK：盖过按 pom 自动挑的（docs/SETTINGS.md 13.6 的次序）。选 17 而 pom 要 21，构建会失败 —— 这里只看命令用的是哪个
  J17=$(ls -d /Library/Java/JavaVirtualMachines/*17*/Contents/Home 2>/dev/null | head -1)
  if [ -n "${J17}" ]; then
    SD='const d = document.querySelector(".dialog.settings"); const cat = (n) => [...d.querySelectorAll(".cat")].find((c) => c.innerText.startsWith(n));'
    lite menu open-settings main >/dev/null
    lite_wait main "return !!document.querySelector('.dialog.settings .cat')" 10 >/dev/null
    lite_eval main "${SD} cat('此项目').click(); await new Promise((r) => setTimeout(r, 300)); [...d.querySelectorAll('.choice')].find((c) => c.textContent.includes('${J17}')).click(); await new Promise((r) => setTimeout(r, 100)); d.querySelector('[data-default]').click(); return true" >/dev/null
    lite_wait main "return !document.querySelector('.dialog.settings')" 10 >/dev/null
    before=$(lite_eval main "${RB} return r?.id ?? 0")
    lite menu rebuild-project main >/dev/null
    lite_wait main "${RB} return !!r && r.id !== ${before} && r.status !== 'running'" 240 >/dev/null
    ok "$(lite_eval main "${RB} return r?.command ?? ''" | grep -q "^export JAVA_HOME=${J17} " && echo 1)" "设置 › 此项目 里选了 Java 17：命令用它，不按 pom 挑 21：$(lite_eval main "${RB} return (r?.command ?? '').split(';')[0]")"
    lite menu open-settings main >/dev/null
    lite_wait main "return !!document.querySelector('.dialog.settings .cat')" 10 >/dev/null
    lite_eval main "${SD} cat('此项目').click(); await new Promise((r) => setTimeout(r, 300)); [...d.querySelectorAll('.choice')].find((c) => c.textContent.includes('根据 pom 自动选择')).click(); await new Promise((r) => setTimeout(r, 100)); d.querySelector('[data-default]').click(); return true" >/dev/null
    lite_wait main "return !document.querySelector('.dialog.settings')" 10 >/dev/null
  else
    skip "这台机器上没有 JDK 17，「项目里明确选一个」那条没验"
  fi
  mv "${P}/pom.xml.bak" "${P}/pom.xml"; touch "${P}/pom.xml"
fi

echo "== 用哪个 Maven（docs/SETTINGS.md 13.4）：项目里有 mvnw 就用它；设置里关掉就用 PATH 里的"
# 假的 mvnw：说一句再交给真 mvn。Spring Initializr 生成的真 mvnw 要下载 Maven，这里只验「用不用它」
printf '#!/bin/sh\necho LITE-MVNW-USED\nexec mvn "$@"\n' > "${P}/mvnw"; chmod +x "${P}/mvnw"
B="const r = __lite.runs.list.find((x) => x.name === '构建项目');"
before=$(lite_eval main "${B} return r?.id ?? 0")
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.id !== ${before} && r.status !== 'running'" 240 >/dev/null
ok "$(lite_eval main "${B} return r?.command ?? ''" | grep -q "./mvnw -B -ntp" && logtext 构建项目 | grep -q LITE-MVNW-USED && echo 1)" "项目里有 mvnw：用它（命令和输出里都看得到）：$(lite_eval main "${B} return (r?.command ?? '').replace(/^export [^;]*; /, '').slice(0, 40)")"
lite menu open-settings main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.settings .cat')" 10 >/dev/null
lite_eval main "const d = document.querySelector('.dialog.settings'); [...d.querySelectorAll('.cat')].find((c) => c.innerText.startsWith('Maven')).click(); await new Promise((r) => setTimeout(r, 150)); [...d.querySelectorAll('.row')].find((r) => r.querySelector('.key')?.textContent === 'maven.useWrapper').querySelector('input').click(); await new Promise((r) => setTimeout(r, 100)); d.querySelector('[data-default]').click(); return true" >/dev/null
lite_wait main "return !document.querySelector('.dialog.settings')" 10 >/dev/null
sleep 0.5
before=$(lite_eval main "${B} return r?.id ?? 0")
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.id !== ${before} && r.status !== 'running'" 240 >/dev/null
ok "$(lite_eval main "${B} return r?.command ?? ''" | grep -qv "mvnw" && lite_eval main "${B} return r?.command ?? ''" | grep -q "mvn -B -ntp" && grep -q '"maven.useWrapper": false,' "${LITE_DATA}/settings.json" && echo 1)" "设置里关掉「用 mvnw」（写进了 settings.json）：用 PATH 里的 mvn：$(lite_eval main "${B} return (r?.command ?? '').replace(/^export [^;]*; /, '').slice(0, 40)")"
rm -f "${P}/mvnw"

echo "== 从 IDEA 带运行配置（docs/SETTINGS.md 13.8）：项目里有 IDEA 存的 .idea/workspace.xml → 「编辑配置…」里 UserApplication 多一个「从 IDEA 导入」→ 填进表单、确定 → 起的 JVM 拿到；IDEA 自动建的那份（只有主类和模块）不列"
# 照 IDEA 2026.1 存出来的样子写（字段名、默认值不写、路径换成 $PROJECT_DIR$、VM 参数里的双引号）
mkdir -p "${P}/.idea"
cat > "${P}/.idea/workspace.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectId" id="lite-ide-accept" />
  <component name="RunManager" selected="Spring Boot.UserApplication (idea)">
    <configuration name="OrderApplication" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot" nameIsGenerated="true">
      <module name="order-service" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="com.demo.order.OrderApplication" />
      <method v="2">
        <option name="Make" enabled="true" />
      </method>
    </configuration>
    <configuration name="UserApplication (idea)" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot">
      <option name="ACTIVE_PROFILES" value="ideaprof" />
      <additionalParameters>
        <param>
          <option name="enabled" value="true" />
          <option name="name" value="lite.over" />
          <option name="value" value="o1" />
        </param>
      </additionalParameters>
      <envs>
        <env name="LITE_IDEA_ENV" value="e1" />
      </envs>
      <module name="user-service" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="com.demo.user.UserApplication" />
      <option name="VM_PARAMETERS" value="-Dlite.msg=&quot;a b $PROJECT_DIR$&quot;" />
      <method v="2">
        <option name="Make" enabled="true" />
      </method>
    </configuration>
  </component>
</project>
EOF
WS0=$(shasum "${P}/.idea/workspace.xml")
lite menu run-configs main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.runcfg #rc-profiles')" 30 >/dev/null
# 带花括号的 JS 放进单引号变量：bash 3.2 在 $(…) 里照样做花括号展开（AGENTS.md「验证纪律」）
BTNS='const btns = async (n) => { [...d.querySelectorAll(".svc")].find((b) => b.textContent.trim() === n).click(); await tick(); return [...d.querySelectorAll(".idea .btn")].map((b) => b.textContent.trim()).join(","); };'
IDEA=$(lite_eval main "${RC} ${BTNS} return 'Order=' + (await btns('OrderApplication')) + ' User=' + (await btns('UserApplication'))")
ok "$([ "${IDEA}" = "Order= User=从「UserApplication (idea)」导入" ] && echo 1)" "只有配过东西的那份有「导入」，自动建的不列：${IDEA}"
lite_eval main "${RC} d.querySelector('.idea .btn').click(); await tick(); d.querySelector('[data-default]').click(); return true" >/dev/null
lite_wait main "return !document.querySelector('.dialog.runcfg')" 10 >/dev/null
pick UserApplication >/dev/null
if wait_for 120 "listening ${UP}"; then
  G=$(pgid_of "${UP}"); GROUPS_SEEN+=("${G}")
  ok "$(logtext UserApplication | grep -qF "LITE-IDE-IDEA a b ${P}|o1|e1|ideaprof" && echo 1)" "起来的 JVM 拿到了：带空格的 VM 参数是一个参数、\$PROJECT_DIR\$ 换回了路径、勾上的覆盖属性、环境变量、profile：$(logtext UserApplication | grep LITE-IDE-IDEA | tail -1 | sed "s|${P}|<项目>|")"
  lite menu run-stop main >/dev/null
  wait_for 15 "! listening ${UP}" >/dev/null
else
  ok "" "UserApplication 带着导入的配置没起来：$(logtext UserApplication | tail -5)"
fi
ok "$([ "$(shasum "${P}/.idea/workspace.xml")" = "${WS0}" ] && [ "$(ls "${P}/.idea")" = workspace.xml ] && echo 1)" "IDEA 的文件只读：workspace.xml 一个字节没变、.idea 里没多东西"
rm -rf "${P}/.idea"

echo "== 你建的运行配置（docs/TAKEOVER.md R1）：「编辑配置…」里 ＋ 应用 → 主类、模块、VM 参数、程序参数 → ⌃⌥R 里有它、起来的 JVM 拿到；和认出来的服务同一种命令（在 reactor 根上跑）"
lite menu run-configs main >/dev/null
lite_wait main "return !!document.querySelector('.dialog.runcfg [aria-label=\"新建\"]')" 30 >/dev/null
# 一步一个 lite_eval：每步之后 Svelte 要重画（表单换了一份），连着写在一个里会拿到旧的元素
lite_eval main "${RC} d.querySelector('[aria-label=\"新建\"]').click(); await tick(); [...d.querySelectorAll('.addmenu .svc')].find((b) => b.textContent.trim() === '应用').click(); await tick(); return true" >/dev/null
lite_eval main "${RC} put(d.querySelector('#rc-name'), '订单手建'); await tick(); put(d.querySelector('#rc-main'), 'com.demo.order.OrderApplication'); await tick(); [...d.querySelectorAll('[aria-label=\"Maven 模块\"] .choice')].find((c) => c.textContent.trim() === 'order-service').click(); await tick(); put(d.querySelector('#rc-uvm'), '-Dlite.vm=hand1'); await tick(); put(d.querySelector('#rc-uargs'), '--lite.arg=h1'); await tick(); d.querySelector('[data-default]').click(); return true" >/dev/null
if lite_wait main "return !document.querySelector('.dialog.runcfg')" 10 >/dev/null; then
  pick '订单手建' >/dev/null
  ok "$(wait_for 120 "logtext '订单手建' | grep -q 'LITE-IDE-CFG hand1 null --lite.arg=h1'" && echo 1)" "起来的 JVM 拿到了 VM 参数和程序参数：$(logtext '订单手建' | grep LITE-IDE-CFG | tail -1)"
  ok "$(lite_eval main "return __lite.runs.list.find((x) => x.name === '订单手建')?.command ?? ''" | grep -q -- "-pl order-service -am .* exec java -Dlite.vm=hand1 -cp .* com.demo.order.OrderApplication --lite.arg=h1$" && echo 1)" "命令和认出来的服务同一种（-pl 模块 -am、exec java、主类在最后）"
else
  ok "" "运行配置对话框没关上（存的时候报错了）：$(lite_eval main "return document.querySelector('.dialog.runcfg .actions .err')?.textContent ?? ''")"
fi
ok "$([ -z "$(ls -A "${P}/.lite-ide" 2>/dev/null)" ] && echo 1)" "你建的运行配置也不往项目里写"

echo "== 远程 JVM 调试（docs/TAKEOVER.md G23）：自己起一个带调试端口的 JVM（每 0.3 秒打一行）→ 建一份「远程 JVM 调试」配置 → ▶ 就是挂上去、停在断点上、能看变量；⌘F2 断开之后那个 JVM 照常在跑"
JDK=${J17:-$(/usr/libexec/java_home 2>/dev/null)}
if [ -x "${JDK}/bin/javac" ]; then
  LOOPSRC="${P}/order-service/src/main/java/com/demo/order/Loop.java"
  printf 'package com.demo.order;\n\npublic class Loop {\n    static int tick(int i) {\n        int twice = i * 2;\n        return twice;\n    }\n    public static void main(String[] a) throws Exception {\n        for (int i = 0; ; i++) { System.out.println("LOOP " + tick(i)); Thread.sleep(300); }\n    }\n}\n' > "${LOOPSRC}"
  LOOPOUT="$(dirname "${P}")/lite-loop-$$"
  mkdir -p "${LOOPOUT}"
  # -g：带上局部变量表（Maven 编的默认带）。不带的话调试器只认得 arg0 —— 远程调试别人编的 JVM 时也是这样，说明里写了
  "${JDK}/bin/javac" -g -d "${LOOPOUT}" "${LOOPSRC}"
  LPORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  "${JDK}/bin/java" "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=127.0.0.1:${LPORT}" -cp "${LOOPOUT}" com.demo.order.Loop > "${LOOPOUT}/out.log" 2>&1 &
  LPID=$!
  wait_for 10 "grep -q LOOP '${LOOPOUT}/out.log'"
  lite_eval main "await window.__TAURI_INTERNALS__.invoke('run_config_user_set', { root: __lite.project.root, old: '', cfg: { name: '远程循环', kind: 'remote', mainClass: '', module: '', goals: '', command: '', cwd: '', items: [], profiles: '', vmOptions: '', programArgs: '', env: [], jdk: '', host: '127.0.0.1', port: ${LPORT} } }); return true" >/dev/null
  lite_eval main "await __lite.debug.breakpoints('${P}', '${LOOPSRC}', [5]); return true" >/dev/null
  RDBG="const r = __lite.runs.list.find((x) => x.name === '远程循环'); const d = r?.debug;"
  pick '远程循环' >/dev/null
  lite_wait main "${RDBG} return d?.phase === 'paused' || d?.phase === 'ended'" 300 >/dev/null
  ok "$([ "$(lite_eval main "${RDBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.path.split('/').pop() + ':' + f.line).join('')")" = "paused Loop.java:5" ] && echo 1)" "▶ 就是挂上去（远程那种只能调试）：停在别人起的 JVM 里、Loop.java:5：$(lite_eval main "${RDBG} return d?.phase + ' ' + (d?.note ?? '')")"
  ok "$(logtext '远程循环' | grep -q "^正在调试 127.0.0.1:${LPORT} 上的 JVM" && echo 1)" "运行窗那一格说清挂在哪、⌘F2 只断开"
  lite_wait main "return [...document.querySelectorAll('.dbg .vrow')].some((v) => v.innerText.startsWith('i = '))" 20 >/dev/null
  ok "$(lite_eval main "return [...document.querySelectorAll('.dbg .vrow')].map((v) => v.innerText.replace(/\s+/g, ' ')).join(' / ')" | grep -qE '(^| )i = [0-9]+' && echo 1)" "变量看得到：$(lite_eval main "return [...document.querySelectorAll('.dbg .vrow')].map((v) => v.innerText.replace(/\s+/g, ' ')).join(' / ')")"
  lite_eval main "await __lite.debug.breakpoints('${P}', '${LOOPSRC}', []); return true" >/dev/null
  lite menu debug-resume main >/dev/null
  lite_wait main "${RDBG} return d?.phase === 'running'" 10 >/dev/null
  lite menu run-stop main >/dev/null
  lite_wait main "${RDBG} return d?.phase === 'ended'" 15 >/dev/null
  N1=$(grep -c LOOP "${LOOPOUT}/out.log"); sleep 1.5; N2=$(grep -c LOOP "${LOOPOUT}/out.log")
  ok "$(kill -0 "${LPID}" 2>/dev/null && [ "${N2}" -gt "${N1}" ] && echo 1)" "⌘F2 断开：那个 JVM 还活着、接着在打（${N1} → ${N2} 行）—— 只断开，没停它"
  ok "$([ "$(lite_eval main "${RDBG} return d?.phase + ':' + r?.status")" = "ended:stopped" ] && echo 1)" "这边的会话结束、那一格是「已停止」：$(lite_eval main "${RDBG} return d?.phase + ':' + r?.status + ':' + (d?.note ?? '')")"
  kill "${LPID}" 2>/dev/null
  # 对面先退：会话自己结束、那一格自己停，说清为什么
  "${JDK}/bin/java" "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=127.0.0.1:${LPORT}" -cp "${LOOPOUT}" com.demo.order.Loop > "${LOOPOUT}/out2.log" 2>&1 &
  LPID=$!
  wait_for 10 "grep -q LOOP '${LOOPOUT}/out2.log'"
  pick '远程循环' >/dev/null
  lite_wait main "${RDBG} return d?.phase === 'running'" 120 >/dev/null
  kill "${LPID}" 2>/dev/null
  lite_wait main "${RDBG} return d?.phase === 'ended'" 15 >/dev/null
  ok "$(lite_eval main "${RDBG} return d?.phase + ':' + r?.status + ':' + (d?.note ?? '')" | grep -q "^ended:.*:127.0.0.1:${LPORT} 上的 JVM 已断开" && echo 1)" "对面的 JVM 退了：会话结束、说清是对面断了：$(lite_eval main "${RDBG} return d?.phase + ':' + r?.status + ':' + (d?.note ?? '')")"
  lite_eval main "await window.__TAURI_INTERNALS__.invoke('run_config_user_delete', { root: __lite.project.root, name: '远程循环' }); return true" >/dev/null
  rm -f "${LOOPSRC}"
else
  skip "机器上找不到 javac（远程调试要自己起一个带调试端口的 JVM）"
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
