#!/bin/bash
# Java 智能（docs/IDEA.md 第 2 步）在真 .app + 真 jdtls 上验：起、Lombok 不报假错、实时报错、⌘B（Lombok 生成的、兄弟模块、jar 里的类）、
# ⌥F7、不往项目里写东西、退出不留 jdtls。走测试通道：不发全局按键、不抢焦点（scripts/lib/bridge.sh）。
#
# jdtls 和跑它的 Java 21 都内置在 .app 里（scripts/java-bundle.sh，docs/IDEA.md 第 10 节），默认验的就是内置的那份 —— 测试 .app 和正式包打的是同一份。
# 给 JDTLS_HOME=<解开的 jdtls> 就写进测试应用的 settings.json 的 java.jdtlsHome，验「用自己那份」那条路（17 条）。
# Maven 本地仓库里要有 Spring Boot 4.0.1 的依赖和 Lombok（m2e 导入时从本地仓库解析；缺的会去你 settings.xml 里的镜像下）。
#
#   scripts/accept/java.sh                    # 内置的
#   JDTLS_HOME=<jdtls 目录> scripts/accept/java.sh
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0; SKIP=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
skip() { SKIP=$((SKIP + 1)); printf '  - 未验：%s\n' "$1"; }
done_() { printf '\n通过 %s 条，失败 %s 条，未验 %s 条\n' "${PASS}" "${FAIL}" "${SKIP}"; [ "${FAIL}" = 0 ]; exit $?; }

MODE=bundled
if [ -n "${JDTLS_HOME:-}" ]; then
  [ -d "${JDTLS_HOME}/plugins" ] || { skip "JDTLS_HOME 底下没有 plugins/"; done_; }
  MODE=home
fi
[ -d "${LITE_APP}/Contents/Resources/java/jdtls/plugins" ] || { ok 0 "测试 .app 里没有内置的 jdtls（scripts/build-test-app.sh 会先跑 java-bundle.sh）"; done_; }
P=$(cd "$(mktemp -d /tmp/acJ.XXXX)" && pwd -P)
cleanup() { lite_teardown; rm -rf "${P}"; }
trap cleanup EXIT

# 项目：shopdemo（spring.sh 那份）+ shop-common 里一个 @Data 类，order-service 调它 Lombok 生成的 getter / setter（IDEA.md 第 7 节的 shoplombok）
mkdir -p "${P}/shop-common/src/main/java/com/demo/common" "${P}/order-service/src/main/java/com/demo/order"
cat > "${P}/pom.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId><version>4.0.1</version><relativePath/></parent>
  <groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version><packaging>pom</packaging>
  <properties><java.version>17</java.version></properties>
  <modules><module>shop-common</module><module>order-service</module></modules>
</project>
EOF
cat > "${P}/shop-common/pom.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version></parent>
  <artifactId>shop-common</artifactId>
  <dependencies><dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId><optional>true</optional></dependency></dependencies>
</project>
EOF
cat > "${P}/order-service/pom.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.demo</groupId><artifactId>shopdemo</artifactId><version>0.0.1-SNAPSHOT</version></parent>
  <artifactId>order-service</artifactId>
  <dependencies>
    <dependency><groupId>com.demo</groupId><artifactId>shop-common</artifactId><version>0.0.1-SNAPSHOT</version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter</artifactId></dependency>
  </dependencies>
</project>
EOF
cat > "${P}/shop-common/src/main/java/com/demo/common/Money.java" <<'EOF'
package com.demo.common;

public record Money(long cents) {
    public String show() { return String.format("¥%d.%02d", cents / 100, cents % 100); }
}
EOF
cat > "${P}/shop-common/src/main/java/com/demo/common/Customer.java" <<'EOF'
package com.demo.common;

import lombok.Data;

@Data
public class Customer {
    private long id;
    private String name;
}
EOF
APP="${P}/order-service/src/main/java/com/demo/order/OrderApplication.java"
cat > "${APP}" <<'EOF'
package com.demo.order;

import com.demo.common.Customer;
import com.demo.common.Money;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

@SpringBootApplication
public class OrderApplication {
    public static void main(String[] args) {
        SpringApplication.run(OrderApplication.class, args);
        Customer c = new Customer();
        c.setName("probe");
        System.out.println(c.getName() + new Money(12345).show());
    }
}
EOF

if [ "${MODE}" = home ]; then
  mkdir -p "${LITE_DATA}"
  python3 -c 'import json,sys; open(sys.argv[1], "w").write(json.dumps({"java.jdtlsHome": sys.argv[2]}, ensure_ascii=False))' "${LITE_DATA}/settings.json" "${JDTLS_HOME}"
fi

lite_launch "${P}" || exit 1
ROOT="${P}"
status() { lite_eval main "return (await window.__TAURI_INTERNALS__.invoke('java_status', { root: '${ROOT}' }))?.phase ?? 'none'"; }
errs() { lite_eval main "return [...document.querySelectorAll('.cm-diag-err')].map((e) => e.textContent).join(',')"; }
open_file() { lite_eval main "await __lite.tabflow.openPath('$1'); return true" >/dev/null; lite_wait main "return __lite.tabs.active?.path === '$1' && !!document.querySelector('.cm-content')" 10; }
# 光标放到第 $1 行那个词（第一次出现）的第 2 个字上
caret_on() { lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('$1')); __lite.caret(l + 1, t[l].indexOf('$1') + 2); return l + 1"; }

echo "== 打开一个 Java 文件：jdtls 在后台起，不挡编辑器"
T0=$(lite_now_ms)
open_file "${APP}"
ok "$([ -n "$(lite_eval main "return __lite.text()?.length > 0")" ] && echo 1)" "编辑器先开出来（不等语言服务器）：$(( $(lite_now_ms) - T0 )) ms"
lite_wait main "return ['starting','importing','ready'].includes((await window.__TAURI_INTERNALS__.invoke('java_status', { root: '${ROOT}' }))?.phase)" 15
ok "$([ "$(status)" != none ] && echo 1)" "jdtls 起了：$(status)"
if lite_wait main "return (await window.__TAURI_INTERNALS__.invoke('java_status', { root: '${ROOT}' }))?.phase === 'ready'" 180; then
  ok 1 "导入完、能答了：$(( $(lite_now_ms) - T0 )) ms"
else
  ok 0 "180 秒没到 ready（现在 $(status)；日志 ${LITE_LOG}）"; lite_quit; done_
fi
DETAIL=$(lite_eval main "return (await window.__TAURI_INTERNALS__.invoke('java_status', { root: '${ROOT}' }))?.detail")
ok "$(echo "${DETAIL}" | grep -q 'Lombok lombok-' && echo 1)" "带上了 Lombok：${DETAIL/#$HOME/~}"
JCMD=$(ps -axo command | grep 'jdt.ls.core.id1' | grep "${LITE_ID}" | grep -v grep | head -1)
ok "$(echo "${JCMD}" | grep -q "lite-ide-mwtest.app/Contents/Resources/java/runtime-" && echo 1)" "跑 jdtls 的是 .app 里内置的 Java（不是机器上的 JDK）：$(echo "${JCMD}" | awk '{print $1}' | sed "s|${LITE_ROOT}/||")"
if [ "${MODE}" = bundled ]; then
  ok "$(echo "${JCMD}" | grep -q "Resources/java/jdtls/plugins/org.eclipse.equinox.launcher" && echo 1)" "jdtls 也是内置的那份"
else
  ok "$(echo "${JCMD}" | grep -qF "${JDTLS_HOME}/plugins/org.eclipse.equinox.launcher" && echo 1)" "jdtls 是 settings.json 里指的那份"
fi

echo "== Lombok：生成的 getter / setter 不报假错"
sleep 4
ok "$([ -z "$(errs)" ] && echo 1)" "没有波浪线（有的话是：$(errs)）"

echo "== 实时报错：改错一个方法名，波浪线出来；改回去，没了"
ORIG=$(lite_eval main "return __lite.text()")
lite_eval main "__lite.setText(__lite.text().replace('c.getName()', 'c.getNamee()')); return true" >/dev/null
T1=$(lite_now_ms)
lite_wait main "return [...document.querySelectorAll('.cm-diag-err')].some((e) => e.textContent === 'getNamee')" 15
ok "$([ "$(errs)" = getNamee ] && echo 1)" "getNamee 底下出了红波浪线：$(( $(lite_now_ms) - T1 )) ms（含 250ms 去抖）"
lite_eval main "__lite.setText(__lite.text().replace('c.getNamee()', 'c.getName()')); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-diag-err').length === 0" 15
ok "$([ -z "$(errs)" ] && echo 1)" "改回去，波浪线没了"

echo "== ⌘B：Lombok 生成的方法、兄弟模块的类、jar 里的类"
caret_on 'getName()' >/dev/null
lite_eval main "__lite.key('Mod-b'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/Customer.java')" 10
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path.endsWith('/Customer.java') && __lite.where().startsWith('8:')")" = true ] && echo 1)" "getName → Customer.java 第 8 行（那个字段）：$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")"
open_file "${APP}" >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('new Money')); __lite.caret(l + 1, t[l].indexOf('Money') + 2); __lite.key('Mod-b'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/Money.java')" 10
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path === '${P}/shop-common/src/main/java/com/demo/common/Money.java'")" = true ] && echo 1)" "Money → 兄弟模块 shop-common 的源码（不是 jar）：$(lite_eval main "return __lite.tabs.active?.path.slice(${#P} + 1)")"
open_file "${APP}" >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('SpringApplication.run')); __lite.caret(l + 1, t[l].indexOf('SpringApplication') + 2); __lite.key('Mod-b'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.mode === 'class'" 20
ok "$([ "$(lite_eval main "return __lite.tabs.active?.mode === 'class' && /^SpringApplication\\.(java|class)$/.test(__lite.tabs.active.name) && __lite.text().includes('class SpringApplication')")" = true ] && echo 1)" "SpringApplication → jar 里的类，只读标签：$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where() + ' ' + __lite.text().length + ' 字'")"
# 两条路各验一次：程序发的事务（右键菜单的粘贴、剪切就是这条）、输入框的那条（打字、输入法）
lite_eval main "__lite.type('xyz'); return true" >/dev/null 2>&1
ok "$([ "$(lite_eval main "return !__lite.text().includes('xyz') && !__lite.tabs.active?.dirty")" = true ] && echo 1)" "jar 里的类：程序发的改动进不去（右键粘贴那条路）"
lite_eval main "__lite.caret(1, 1); document.execCommand('insertText', false, 'qqq'); return true" >/dev/null 2>&1
sleep 0.3
ok "$([ "$(lite_eval main "return !__lite.text().includes('qqq') && !__lite.tabs.active?.dirty")" = true ] && echo 1)" "jar 里的类：打字进不去（输入框那条路）"

echo "== JDK 里的类：项目声明 Java 17，⌘B 进 String 是机器上 JDK 17 的源码（jdtls 自己用 java_home -X 认的），不是内置那份裁过的、没源码的"
lite_eval main "await __lite.tabflow.openPath('${P}/shop-common/src/main/java/com/demo/common/Customer.java'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/Customer.java') && !!document.querySelector('.cm-content')" 10
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('private String')); __lite.caret(l + 1, t[l].indexOf('String') + 2); __lite.key('Mod-b'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.mode === 'class'" 20
STR=$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.tabs.active?.path.split('?')[0].split('/').slice(-3, -1).join('/')")
if [ -d /Library/Java/JavaVirtualMachines/temurin-17.jdk ] || ls -d "${HOME}"/Library/Java/JavaVirtualMachines/*17* >/dev/null 2>&1; then
  ok "$([ "$(lite_eval main "return __lite.tabs.active?.name === 'String.java' && __lite.text().includes('public final class String')")" = true ] && echo 1)" "String → String.java（有源码）：${STR}"
else
  skip "机器上没有 JDK 17，String 会是内置运行时里反编译的：${STR}"
fi

echo "== ⌥F7：查找用法"
lite_eval main "await __lite.tabflow.openPath('${P}/shop-common/src/main/java/com/demo/common/Customer.java'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/Customer.java') && !!document.querySelector('.cm-content')" 10
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('class Customer')); __lite.caret(l + 1, t[l].indexOf('Customer') + 2); __lite.key('Alt-F7'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.usages .row')" 20
ROWS=$(lite_eval main "return [...document.querySelectorAll('.usages .row .file')].map((r) => r.innerText).join(',')")
ok "$(echo "${ROWS}" | grep -q 'OrderApplication.java' && echo 1)" "列表里有 order-service 里的用法：${ROWS}"
lite_eval main "document.querySelector('.usages').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/OrderApplication.java')" 10
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path.endsWith('/OrderApplication.java') && !document.querySelector('.usages')")" = true ] && echo 1)" "↵ 跳过去、列表关掉：$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")"

echo "== 不往项目里写东西、退出不留 jdtls"
META=$(cd "${P}" && find . \( -name .project -o -name .classpath -o -name .settings -o -name .factorypath \) | head -5 | tr '\n' ' ')
ok "$([ -z "${META}" ] && echo 1)" "项目里没有 Eclipse 的元数据文件${META:+：${META}}"
JPID=$(pgrep -f "jdt.ls.core.id1.*${LITE_ID}" | head -1)
ok "$([ -n "${JPID}" ] && echo 1)" "jdtls 在跑（pid ${JPID:-无}），缓存在测试应用的数据目录里"
lite_quit
sleep 1
ok "$([ -z "$(pgrep -f "jdt.ls.core.id1.*${LITE_ID}")" ] && echo 1)" "退出之后 jdtls 也没了"
done_
