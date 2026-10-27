#!/bin/bash
# Gradle 项目（docs/TAKEOVER.md G24）在真 .app + 真 Gradle 上验：认出 Spring Boot 服务、起（兄弟模块的类、classpath 拼对）、⌘F2 停干净、
# 单测（结果树、行号旁 ✓✗、命令是 Gradle 的 --tests、没改过也真跑）、调试单测（停在测试的断点上）、⌘F9 构建（javac 的错误按文件列、Gradle 打两遍只算一条）、不往项目里写东西。
# 走测试通道：不发全局按键、不抢焦点（scripts/lib/bridge.sh）。
#
# 要一份 Gradle 9.8.1 的发行包在 ~/.gradle/wrapper/dists 里（165 MB）：没有就记「未验」，不替你下 —— 下的话：
#   在任意一个带 gradle-wrapper.properties（distributionUrl = 9.8.1、distributionSha256Sum 照官方的）的项目里跑一次 ./gradlew --version
# 有了它，脚本用它在测试项目里生成 gradlew（`gradle wrapper`，不上网），后面全走 gradlew。Spring Boot 插件和依赖第一次要从
# plugins.gradle.org / Maven Central 下（进 ~/.gradle/caches，同 Maven 的 ~/.m2）
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
# 登录 shell 读你的 .zshrc（java 在 PATH 上；同 spring.sh）
export LITE_USER_SHELL=1
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0; SKIP=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
skip() { SKIP=$((SKIP + 1)); printf '  - 未验：%s\n' "$1"; }
done_() { printf '\n通过 %s 条，失败 %s 条，未验 %s 条\n' "${PASS}" "${FAIL}" "${SKIP}"; [ "${FAIL}" = 0 ]; exit $?; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

GV=9.8.1
GSUM=dce76f55f8e251a3a1f130eb120f30b3d271de2b76c9b0729d316b5a1b6dc01f
G=$(ls -d "${HOME}"/.gradle/wrapper/dists/gradle-${GV}-bin/*/gradle-${GV}/bin/gradle 2>/dev/null | head -1)
if [ -z "${G}" ]; then
  skip "~/.gradle 里没有 Gradle ${GV} 的发行包（见脚本头上怎么下）"
  done_
fi

P=$(cd "$(mktemp -d /tmp/acG.XXXX)" && pwd -P)
cleanup() { pkill -f "com.demo.order.OrderApplication" 2>/dev/null; lite_teardown; rm -rf "${P}"; }
trap cleanup EXIT

# 项目：两个模块，order-service 用 shop-common 的类；OrderApplication 起来打一行探针、然后挂着（像个服务）
mkdir -p "${P}/shop-common/src/main/java/com/demo/common" "${P}/shop-common/src/test/java/com/demo/common" "${P}/order-service/src/main/java/com/demo/order"
cat > "${P}/settings.gradle.kts" <<'EOF'
rootProject.name = "shopdemo"
include("shop-common", "order-service")
EOF
cat > "${P}/build.gradle.kts" <<'EOF'
plugins {
    id("org.springframework.boot") version "4.0.1" apply false
    id("io.spring.dependency-management") version "1.1.7" apply false
}
subprojects {
    apply(plugin = "java")
    apply(plugin = "io.spring.dependency-management")
    repositories { mavenCentral() }
    the<io.spring.gradle.dependencymanagement.dsl.DependencyManagementExtension>().imports { mavenBom("org.springframework.boot:spring-boot-dependencies:4.0.1") }
    tasks.withType<Test> { useJUnitPlatform() }
}
EOF
cat > "${P}/shop-common/build.gradle.kts" <<'EOF'
dependencies {
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}
EOF
cat > "${P}/order-service/build.gradle.kts" <<'EOF'
apply(plugin = "org.springframework.boot")
dependencies {
    implementation(project(":shop-common"))
    implementation("org.springframework.boot:spring-boot-starter")
}
EOF
MONEY="${P}/shop-common/src/main/java/com/demo/common/Money.java"
printf 'package com.demo.common;\n\npublic class Money {\n    public static long cents(long yuan) { return yuan * 100; }\n}\n' > "${MONEY}"
printf 'package com.demo.common;\n\nimport org.junit.jupiter.api.Test;\nimport static org.junit.jupiter.api.Assertions.*;\n\nclass MoneyTest {\n    @Test void ok() { assertEquals(300, Money.cents(3)); }\n    @Test void bad() { assertEquals(1, Money.cents(3)); }\n}\n' > "${P}/shop-common/src/test/java/com/demo/common/MoneyTest.java"
printf 'package com.demo.order;\n\nimport com.demo.common.Money;\nimport org.springframework.boot.SpringApplication;\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n\n@SpringBootApplication\npublic class OrderApplication {\n    public static void main(String[] args) throws Exception {\n        SpringApplication.run(OrderApplication.class, args);\n        System.out.println("LITE-GRADLE-PROBE " + Money.cents(2));\n        Thread.sleep(600000);\n    }\n}\n' > "${P}/order-service/src/main/java/com/demo/order/OrderApplication.java"
(cd "${P}" && "${G}" -q wrapper --gradle-version "${GV}" --gradle-distribution-sha256-sum "${GSUM}" </dev/null >/dev/null 2>&1)
[ -x "${P}/gradlew" ] || { ok 0 "gradle wrapper 没生成 gradlew"; done_; }
mkdir -p "${P}/order-service/src/main/resources"; printf 'spring:\n  main:\n    banner-mode: off\n' > "${P}/order-service/src/main/resources/application.yml"
# 生成 wrapper 那一次在项目里留下的（.gradle/、build/）不算我们写的：先记下来
BEFORE=$(cd "${P}" && find . -path ./.gradle -prune -o -type f -print | sort)

lite_launch "${P}" || exit 1
logtext() { local f; f=$(ls -t "${LITE_DATA}/runs/"*/"$1"-*.log "${LITE_DATA}/runs/$1"-*.log 2>/dev/null | head -1); [ -n "${f}" ] && cat "${f}"; }
R() { echo "const r = __lite.runs.list.find((x) => x.name === '$1');"; }

echo "== 认出来：任务列表（⌃⌥R）里 Spring Boot 一组只有 OrderApplication（公共模块不算）"
lite menu run-pick main >/dev/null
lite_wait main "return document.querySelectorAll('.picker .row').length >= 1" 15 >/dev/null
L=$(lite_eval main "return [...document.querySelectorAll('.picker .group')].map((g) => g.innerText).join('|') + ' :: ' + [...document.querySelectorAll('.picker .row .name:not(.action)')].map((n) => n.innerText).join(',')")
ok "$([ "${L}" = "Spring Boot :: OrderApplication" ] && echo 1)" "${L}"

echo "== 起 OrderApplication：Gradle 算 classpath（兄弟模块的 jar 顺带编好）、exec java 起；输出里有用到兄弟模块的那行"
lite_eval main "const r = [...document.querySelectorAll('.picker .row')].find((x) => x.querySelector('.name')?.innerText === 'OrderApplication'); r?.click(); return !!r" >/dev/null
ok "$(wait_for 240 "logtext OrderApplication | grep -q 'LITE-GRADLE-PROBE 200'" && echo 1)" "输出里有探针那行：$(logtext OrderApplication | grep -E 'LITE-GRADLE-PROBE|FAIL|rror' | head -3 | tr '\n' ' ')"
CMD=$(lite_eval main "$(R OrderApplication) return r?.command ?? ''")
ok "$(echo "${CMD}" | grep -q -- './gradlew --console=plain -q --init-script ' && echo "${CMD}" | grep -q ':order-service:liteIdeClasspath && cd order-service && exec java ' && echo 1)" "命令是 gradlew + init 脚本算 classpath + exec java（调试、热替换认的形状）：${CMD}"
APP=$(pgrep -f com.demo.order.OrderApplication | head -1)
ok "$([ -n "${APP}" ] && [ "$(ps -o ppid= -p "${APP}" | tr -d ' ')" != "$(pgrep -f GradleDaemon | head -1)" ] && echo 1)" "应用的 JVM 是我们起的（不是 Gradle daemon 的子进程 —— ⌘F2 的整组停够得着它）：pid ${APP:-无}"

echo "== ⌘F2 停：应用没了"
lite menu run-stop main >/dev/null
wait_for 10 "! pgrep -f com.demo.order.OrderApplication"
ok "$(! pgrep -f com.demo.order.OrderApplication >/dev/null && echo 1)" "⌘F2 之后 OrderApplication 的 JVM 没了"

echo "== jdtls 导入 Gradle 项目：⌘B 进得了 SpringApplication（依赖 jar 认得）、Money（兄弟模块的源码）；入口类没有报错"
OA="${P}/order-service/src/main/java/com/demo/order/OrderApplication.java"
lite_eval main "await __lite.tabflow.openPath('${OA}', { preview: false }); return true" >/dev/null
DEF() { lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('$1')); const c = t[l].indexOf('$1') + 2; const r = await window.__TAURI_INTERNALS__.invoke('java_definition', { root: '${P}', path: '${OA}', line: l, character: c }); return r.map((x) => (x.uri ?? x.path ?? '').split('/').pop()).join(',')"; }
T0=$(date +%s)
wait_for 240 "DEF 'SpringApplication.run' | grep -q 'SpringApplication'"
ok "$(DEF 'SpringApplication.run' | grep -q 'SpringApplication' && echo 1)" "⌘B SpringApplication：$(DEF 'SpringApplication.run')（导入用了 $(( $(date +%s) - T0 )) 秒；Gradle 用的是机器上的 JDK，不是 .app 里那个没有 javac 的）"
ok "$(DEF 'Money.cents' | grep -q 'Money.java' && echo 1)" "⌘B Money.cents 进兄弟模块的源码：$(DEF 'Money.cents')"
lite_wait main "return document.querySelectorAll('.cm-diag-err').length === 0" 30 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-diag-err').length")" = 0 ] && echo 1)" "入口类里 jdtls 不报错"

echo "== 调试：exec java 这个形状让 JDWP 插得进 —— 停在兄弟模块（在 classpath 上是 jar）的断点上；F9 继续照常跑"
lite_eval main "await __lite.tabflow.openPath('${MONEY}', { preview: false }); await __lite.debug.breakpoints('${P}', '${MONEY}', [4]); return true" >/dev/null
DS=$(lite_eval main "await __lite.debugTask('OrderApplication'); return 'started'")
DBG="const r = __lite.runs.list.find((x) => x.name === 'OrderApplication'); const d = r?.debug;"
T0=$(date +%s)
lite_wait main "${DBG} return d?.phase === 'paused' || d?.phase === 'ended'" 300 >/dev/null
FR=$(lite_eval main "${DBG} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 2).map((f) => f.name + '@' + (f.path ?? '').split('/').pop() + ':' + f.line).join(' | ') + ' ' + (d?.note ?? '')")
ok "$(echo "${FR}" | grep -q '^paused Money.cents(long)@Money.java:4 | OrderApplication.main(String\[\])@OrderApplication.java:' && echo 1)" "停在 shop-common 的 Money.java:4、调用者是入口类（点了调试 $(( $(date +%s) - T0 )) 秒）：${DS} ${FR}"
lite menu debug-resume main >/dev/null
ok "$(wait_for 60 "logtext OrderApplication | tail -20 | grep -q 'LITE-GRADLE-PROBE 200'" && echo 1)" "F9 继续：照常跑，输出里有探针那行"
lite_eval main "await __lite.debug.breakpoints('${P}', '${MONEY}', []); return true" >/dev/null
lite menu run-stop main >/dev/null
wait_for 10 "! pgrep -f com.demo.order.OrderApplication"

echo "== 单测：⌃⇧R 跑整个类 → Gradle 的 --tests；结果树 1 过 1 没过、行号旁 ✓ / ✗"
TEST="${P}/shop-common/src/test/java/com/demo/common/MoneyTest.java"
lite_eval main "await __lite.tabflow.openPath('${TEST}', { preview: false }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-test-run').length === 3" 15 >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); __lite.caret(t.findIndex((x) => x.includes('class MoneyTest')) + 1, 1); __lite.key('Ctrl-Shift-r'); return true" >/dev/null
RUN="shop-common [test com.demo.common.MoneyTest]"
lite_wait main "return __lite.runs.list.some((r) => r.name === '${RUN}' && r.tests && r.status !== 'running' && r.status !== 'stopping')" 240 >/dev/null
ok "$([ "$(lite_eval main "$(R "${RUN}") return !!r?.tests && r.tests.cases.map((c) => c.name + ':' + c.outcome).sort().join(',') === 'bad():failed,ok():passed' || r?.tests?.cases.map((c) => c.name + ':' + c.outcome).sort().join(',') === 'bad:failed,ok:passed'")" = true ] && echo 1)" "结果树（读的是 build/test-results/test）：$(lite_eval main "$(R "${RUN}") return JSON.stringify(r?.tests?.cases.map((c) => c.name + ':' + c.outcome))")"
TC=$(lite_eval main "$(R "${RUN}") return r?.command ?? ''")
ok "$(echo "${TC}" | grep -q '^\./gradlew --console=plain --init-script .* :shop-common:test --rerun --tests com\.demo\.common\.MoneyTest$' && echo 1)" "命令（带 --rerun：上次过了、没改过，Gradle 会说 UP-TO-DATE 不跑）：${TC}"
lite_wait main "return document.querySelectorAll('.cm-test-run.failed').length === 2 && document.querySelectorAll('.cm-test-run.passed').length === 1" 10 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-test-run.failed').length === 2 && document.querySelectorAll('.cm-test-run.passed').length === 1")" = true ] && echo 1)" "行号旁：类和没过的是 ✗、过了的是 ✓"
# 只跑过得了的那个方法、跑两遍：第二遍什么都没改，没有 --rerun 的话 Gradle 说 UP-TO-DATE 不跑（没过的测试不会 UP-TO-DATE，所以挑过得了的验）
OKRUN="shop-common [test com.demo.common.MoneyTest#ok]"
OKLOG='shop-common__test_com.demo.common.MoneyTest_ok_'
for _ in 1 2; do
  lite_eval main "__lite.runTask('${OKRUN}'); return true" >/dev/null
  sleep 1
  lite_wait main "return __lite.runs.list.some((r) => r.name === '${OKRUN}' && r.status !== 'running' && r.status !== 'stopping')" 120 >/dev/null
done
ok "$(logtext "${OKLOG}" | grep -q '> Task :shop-common:test$' && echo 1)" "过得了的方法、什么都没改再跑一遍：test 任务真跑了（不是 UP-TO-DATE）：$(logtext "${OKLOG}" | grep ':shop-common:test' | tail -1)"

echo "== 带覆盖率跑单测（docs/TAKEOVER.md G15）：JaCoCo 的代理经 init 脚本交给 Gradle daemon 起的测试 JVM（-PliteIdeJacoco）→ 覆盖率工具窗列出 Money.java、行号旁跑到的行绿"
COV="shop-common [cover com.demo.common.MoneyTest]"
lite_eval main "__lite.runTask('${COV}'); return true" >/dev/null
sleep 1
lite_wait main "const r = __lite.runs.list.find((x) => x.name === '${COV}'); return !!r && r.status !== 'running' && r.status !== 'stopping'" 240 >/dev/null
CCMD=$(lite_eval main "return __lite.runs.list.find((x) => x.name === '${COV}')?.command ?? ''")
ok "$(echo "${CCMD}" | grep -q -- "--rerun --tests com.demo.common.MoneyTest -PliteIdeJacoco=-javaagent:.*jacocoagent.jar=destfile=.*,append=false" && echo 1)" "命令同单测、末尾多一个 -PliteIdeJacoco（init 脚本交给测试 JVM）：$(echo "${CCMD}" | sed 's/.*--init-script [^ ]* //')"
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('行覆盖率')" 60 >/dev/null
CTOAST=$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''")
lite menu tw-coverage main >/dev/null
lite_wait main "return [...document.querySelectorAll('.cov .row .name')].some((e) => e.textContent === 'Money.java')" 15 >/dev/null
CROWS=$(lite_eval main "return [...document.querySelectorAll('.cov .row')].map((r) => r.querySelector('.name').textContent + ' ' + r.querySelector('.pct').textContent).join(', ')")
# Money.java 是 50%：第 3 行是隐式的构造器，测试没 new 过它（JaCoCo 照实算，同 IDEA）
ok "$(echo "${CROWS}" | grep -q 'Money.java [0-9]*%' && echo "${CTOAST}" | grep -q '行覆盖率' && echo 1)" "跑完说行覆盖率（${CTOAST}）；覆盖率工具窗列出 Money.java（读 build/classes/java/main）：${CROWS}"
lite_eval main "await __lite.tabflow.openPath('${MONEY}', { preview: false }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-lineNumbers .cm-cov').length > 0" 15 >/dev/null
CGUT=$(lite_eval main "return [...document.querySelectorAll('.cm-lineNumbers .cm-cov')].map((e) => e.textContent.trim() + ':' + (e.classList.contains('cm-cov-hit') ? 'hit' : e.classList.contains('cm-cov-miss') ? 'miss' : 'part')).join(',')")
ok "$(echo "${CGUT}" | grep -q '4:hit' && echo 1)" "Money.java 第 4 行（cents）行号旁是绿的：${CGUT}"
lite menu tw-coverage main >/dev/null

echo "== 调试单测：测试的 JVM 是 Gradle daemon 起的（不在我们的进程组里）—— 端口我们定、init 脚本交给它；停在测试里的断点上，F9 之后照常跑完、结果树出来"
lite_eval main "await __lite.debug.breakpoints('${P}', '${TEST}', [7]); return true" >/dev/null
lite_eval main "await __lite.debugTask('${RUN}'); return true" >/dev/null
TD="const r = __lite.runs.list.find((x) => x.name === '${RUN}'); const d = r?.debug;"
T0=$(date +%s)
lite_wait main "${TD} return d?.phase === 'paused' || d?.phase === 'ended'" 240 >/dev/null
TF=$(lite_eval main "${TD} return d?.phase + ' ' + (d?.frames ?? []).slice(0, 1).map((f) => f.name + '@' + (f.path ?? '').split('/').pop() + ':' + f.line).join('') + ' ' + (d?.note ?? '')")
ok "$(echo "${TF}" | grep -q '^paused MoneyTest.ok()@MoneyTest.java:7' && echo 1)" "停在 MoneyTest.java:7（$(( $(date +%s) - T0 )) 秒）：${TF}"
ok "$(lite_eval main "${TD} return r?.command ?? ''" | grep -q -- '--rerun .*-PliteIdeJdwp=-agentlib:jdwp=.*address=127.0.0.1:[0-9]' && echo 1)" "命令带 --rerun（没改过也要真跑）和我们定的调试端口"
lite_eval main "await __lite.debug.breakpoints('${P}', '${TEST}', []); return true" >/dev/null
lite menu debug-resume main >/dev/null
lite_wait main "${TD} return r?.status !== 'running' && r?.status !== 'stopping' && !!r?.tests && d?.phase === 'ended'" 120 >/dev/null
ok "$([ "$(lite_eval main "${TD} return d?.phase + ':' + r?.tests?.cases.map((c) => c.name + ':' + c.outcome).sort().join(',')")" = "ended:bad():failed,ok():passed" ] && echo 1)" "F9 之后跑完：调试会话结束、结果树照常出来：$(lite_eval main "${TD} return d?.phase + ':' + r?.tests?.cases.map((c) => c.name + ':' + c.outcome).sort().join(',')")"

echo "== Gradle 工具窗（照 IDEA）：导轨上有它；构建根 → 子项目 → Tasks → 组 → 任务（gradlew tasks --all 列的）；双击 shop-common 的 jar → 运行窗里跑、出 jar"
lite_eval main "document.querySelector('nav.rail button[aria-label=\"Gradle\"]').click(); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('.gradle .row')].some((r) => r.innerText.trim() === 'shop-common')" 20 >/dev/null
lite_eval main "const r = [...document.querySelectorAll('.gradle .row')].find((x) => x.innerText.trim() === 'shop-common'); r.click(); return true" >/dev/null
lite_eval main "const r = [...document.querySelectorAll('.gradle .row')].filter((x) => x.innerText.trim() === 'Tasks').pop(); r.click(); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('.gradle .row')].some((r) => r.innerText.trim() === 'build')" 60 >/dev/null
GROWS=$(lite_eval main "return [...document.querySelectorAll('.gradle .row')].map((r) => r.innerText.split('\\n')[0].trim()).join(',')")
ok "$(echo "${GROWS}" | grep -q 'shopdemo' && echo "${GROWS}" | grep -q 'order-service' && echo "${GROWS}" | grep -q 'shop-common,Tasks' && echo "${GROWS}" | grep -q 'build' && echo "${GROWS}" | grep -q 'verification' && echo 1)" "树：${GROWS}"
lite_eval main "const r = [...document.querySelectorAll('.gradle .row')].filter((x) => x.innerText.trim() === 'build').pop(); r.click(); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('.gradle .row')].some((r) => r.innerText.split('\\n')[0].trim() === 'jar')" 10 >/dev/null
lite_eval main "const r = [...document.querySelectorAll('.gradle .row')].find((x) => x.innerText.split('\\n')[0].trim() === 'jar'); r.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); return true" >/dev/null
JR=$(R ':shop-common [jar]')
lite_wait main "${JR} return !!r && r.status !== 'running' && r.status !== 'stopping'" 120 >/dev/null
ok "$([ "$(lite_eval main "${JR} return r?.status + '|' + r?.command")" = "done|./gradlew --console=plain :shop-common:jar" ] && [ -f "${P}/shop-common/build/libs/shop-common.jar" ] && echo 1)" "双击 jar：运行窗里跑完、出了 build/libs/shop-common.jar：$(lite_eval main "${JR} return r?.status + '|' + r?.command")"

echo "== ⌘F9 构建：Gradle 的 classes testClasses；javac 的错误（中文、没有列、打两遍）按文件列、只算一条；改好了再构建成功"
cp "${MONEY}" "${P}/Money.java.bak"
python3 -c 'import sys; p=sys.argv[1]; s=open(p).read(); open(p,"w").write(s.replace("yuan * 100", "yuan * nosuch"))' "${MONEY}"
B=$(R 构建项目)
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.status !== 'running' && !!r.problems" 240 >/dev/null
ok "$([ "$(lite_eval main "${B} return r?.command + '|' + r?.status")" = "./gradlew --console=plain --continue classes testClasses|failed" ] && echo 1)" "菜单「构建项目」：$(lite_eval main "${B} return r?.command + '|' + r?.status")"
PR=$(lite_eval main "${B} const p = r?.problems?.items ?? []; return p.length + ' ' + p.map((x) => x.path.split('/').pop() + ':' + x.line + ':' + x.col + ':' + x.message.split('\\n')[0]).join(',')")
ok "$(echo "${PR}" | grep -q '^1 Money.java:4:' && echo 1)" "错误树里正好一条、在 Money.java 第 4 行（Gradle 把它打了两遍；列从 ^ 算）：${PR}"
mv "${P}/Money.java.bak" "${MONEY}"; touch "${MONEY}"
lite_wait main "return !document.querySelector('.toast')" 15 >/dev/null
lite menu build-project main >/dev/null
lite_wait main "${B} return !!r && r.status === 'done' && r.problems?.items.length === 0" 240 >/dev/null
ok "$([ "$(lite_eval main "${B} return r?.status + ':' + r?.problems?.items.length")" = "done:0" ] && echo 1)" "改好了再构建：成功、没有错误"

echo "== Spring Boot 配置文件（docs/TAKEOVER.md G30）：application.yml 里的补全来自 jar 里的配置元数据（问 jdtls 要 classpath）—— 父键底下只给它的子键、接受插相对键；布尔的值给 true / false"
Y="${P}/order-service/src/main/resources/application.yml"
mkdir -p "$(dirname "${Y}")"; printf 'spring:\n  main:\n    banner-mode: off\n' > "${Y}"
lite_eval main "await __lite.tabflow.openPath('${Y}', { preview: false }); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${Y}' && !!document.querySelector('.cm-content')" 10 >/dev/null
lite_eval main "__lite.caret(3, 21); __lite.type('\\n    lazy'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.includes('lazy-initialization')" 60 >/dev/null
CL=$(lite_eval main "return __lite.completions().labels.slice(0, 5).join(' / ')")
ok "$([ "$(lite_eval main "return __lite.completions().labels.includes('lazy-initialization') && !__lite.completions().labels.some((l) => l.startsWith('spring.'))")" = true ] && echo 1)" "spring.main 底下敲 lazy：候选是相对键（不带 spring.main. 前缀）：${CL}"
sleep 0.2
lite_eval main "__lite.acceptCompletion(); return true" >/dev/null
lite_wait main "return __lite.text().includes('    lazy-initialization: ')" 5 >/dev/null
ok "$(lite_eval main "return __lite.text()" | grep -q '^    lazy-initialization: $' && echo 1)" "接受：插的是「lazy-initialization: 」"
lite_eval main "__lite.type('t'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.includes('true')" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.completions().labels.join(',')")" = "true" ] && echo 1)" "布尔的值：敲 t 给 true：$(lite_eval main "return __lite.completions().labels.join(',')")"
lite_eval main "__lite.setText('logging.char'); return true" >/dev/null
lite_eval main "__lite.type('s'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.includes('logging.charset.console')" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.completions().labels.includes('logging.charset.console')")" = true ] && echo 1)" "顶层敲 logging.chars：给整个键名：$(lite_eval main "return __lite.completions().labels.slice(0, 3).join(' / ')")"
# 值是枚举、元数据没给可选值（Banner$Mode）：读 classpath 上 spring-boot.jar 里那个类文件拿常量（spring_meta::enum_values）
lite_eval main "__lite.setText('spring:\\n  main:\\n    banner-mode: '); __lite.caret(3, 18); __lite.type('C'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.includes('CONSOLE')" 20 >/dev/null
EN=$(lite_eval main "return __lite.completions().labels.join(',')")
ok "$([ "${EN}" = "CONSOLE" ] && echo 1)" "值是枚举（spring.main.banner-mode → Banner\$Mode，元数据里没写可选值）：敲 C 给 CONSOLE：${EN}"
lite_eval main "__lite.setText('spring:\\n  main:\\n    banner-mode: off\\n'); return true" >/dev/null
lite menu save main >/dev/null
lite_wait main "return __lite.tabs.byPath('${Y}')?.dirty === false" 10 >/dev/null
sleep 1

echo "== 不往项目里写东西（build/ 底下的产物除外）：init 脚本在应用数据目录里"
AFTER=$(cd "${P}" && find . -path ./.gradle -prune -o -path '*/build' -prune -o -type f -print | sort)
NEW=$(comm -13 <(echo "${BEFORE}" | grep -v '/build/') <(echo "${AFTER}"))
ok "$([ -z "${NEW}" ] && [ -f "${LITE_DATA}/lite-ide-init.gradle" ] && echo 1)" "项目里多出来的文件：${NEW:-没有}；init 脚本：$([ -f "${LITE_DATA}/lite-ide-init.gradle" ] && echo 在应用数据目录 || echo 没找到)"

lite_quit
done_
