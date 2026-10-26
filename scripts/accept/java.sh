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
# 等一个 shell 条件成立（盘上的文件）。原来没有这个函数：改名那两条写了 wait_for，bash 报「找不到命令」当场返回，检查在改名做完之前就跑了（2026-10-19）
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }
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
# 跳到实现 / 父方法（docs/TAKEOVER.md G1）用：一个接口两个实现、一个覆盖 toString 的类
C="${P}/shop-common/src/main/java/com/demo/common"
printf 'package com.demo.common;\n\npublic interface PriceRule {\n    /** 按规则算出<b>折后价</b>（分）。 */\n    long apply(long cents);\n}\n' > "${C}/PriceRule.java"
printf 'package com.demo.common;\n\npublic class VipRule implements PriceRule {\n    @Override\n    public long apply(long cents) { return cents * 9 / 10; }\n}\n' > "${C}/VipRule.java"
printf 'package com.demo.common;\n\npublic class HalfRule implements PriceRule {\n    @Override\n    public long apply(long cents) { return cents / 2; }\n}\n' > "${C}/HalfRule.java"
# 同名高亮（G11）用：字段 total 有声明、有写、有读；other() 的参数也叫 total —— 语义的高亮不该标它。twice() 传字面量：参数名提示用
printf 'package com.demo.common;\n\npublic class Tally {\n    private int total;\n    public int add(int n) {\n        total = total + n;\n        return total;\n    }\n    public int other(int total) { return total; }\n    public int twice() { return add(2) + other(3); }\n}\n' > "${C}/Tally.java"
# 移动类（G12 的 F6）用：Label 挪进已有的 com.demo.common.util，order-service 里的 LabelUse 引着它
mkdir -p "${C}/util"
printf 'package com.demo.common.util;\n\npublic class Util {}\n' > "${C}/util/Util.java"
printf 'package com.demo.common;\n\npublic class Label {\n    public String text() { return "label"; }\n}\n' > "${C}/Label.java"
printf 'package com.demo.order;\n\nimport com.demo.common.Label;\n\npublic class LabelUse {\n    String go() { return new Label().text(); }\n}\n' > "${P}/order-service/src/main/java/com/demo/order/LabelUse.java"
# 改签名 / 引入参数（G12 的 ⌘F6 / ⌥⌘P）用：Fee.calc 在 order-service 的 FeeUse 里被调用（跨模块）
printf 'package com.demo.common;\n\npublic class Fee {\n    public long calc(int cents, String code) {\n        return cents * 3 / 100;\n    }\n    private long tax(long v) {\n        return v * 6 / 100;\n    }\n    long total(long v) { return v + tax(v); }\n}\n' > "${C}/Fee.java"
printf 'package com.demo.order;\n\nimport com.demo.common.Fee;\n\npublic class FeeUse {\n    long go() { return new Fee().calc(500, "A"); }\n}\n' > "${P}/order-service/src/main/java/com/demo/order/FeeUse.java"
# 代码模板（G21）用：一个空方法体，里面敲模板
printf 'package com.demo.common;\n\npublic class Tpl {\n    void run(String name) {\n        \n    }\n}\n' > "${C}/Tpl.java"
# 生成（G5）用：一个普通字段、一个带初值的 final 字段（final 的没有 setter）
printf 'package com.demo.common;\n\npublic class Point {\n    private int x;\n    private final String name = "p";\n}\n' > "${C}/Point.java"
printf 'package com.demo.common;\n\npublic class Coupon {\n    @Override\n    public String toString() { return "coupon"; }\n}\n' > "${C}/Coupon.java"
# 代码风格（G22）用：项目自己的 IDEA 方案（「用项目的」打开 —— 也让这份验收不跟着你机器上 IDEA 的设置变）。
# 续行 4、行宽 80 都和 Eclipse 的默认（8、120）不一样：没转过去的话一眼看得出来；同一个包 3 个类就合成 .*，import 次序照 IDEA 默认（不碰前面自动 import 那几节）
mkdir -p "${P}/.idea/codeStyles"
printf '<component name="ProjectCodeStyleConfiguration">\n  <state>\n    <option name="USE_PER_PROJECT_SETTINGS" value="true" />\n  </state>\n</component>\n' > "${P}/.idea/codeStyles/codeStyleConfig.xml"
printf '<component name="ProjectCodeStyleConfiguration">\n  <code_scheme name="Project" version="173">\n    <JavaCodeStyleSettings>\n      <option name="CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" value="3" />\n    </JavaCodeStyleSettings>\n    <codeStyleSettings language="JAVA">\n      <option name="RIGHT_MARGIN" value="80" />\n      <indentOptions>\n        <option name="CONTINUATION_INDENT_SIZE" value="4" />\n      </indentOptions>\n    </codeStyleSettings>\n  </code_scheme>\n</component>\n' > "${P}/.idea/codeStyles/Project.xml"
cat > "${C}/Sty.java" <<'EOF2'
package com.demo.common;

import java.util.Map;
import com.demo.common.Money;
import static java.util.Collections.emptyList;
import java.util.HashMap;
import java.util.List;
import java.util.ArrayList;

public class Sty {
    List<String> a = new ArrayList<>();
    Map<String, String> b = new HashMap<>();
    List<String> c = emptyList();

    void say(String first, String second, String third) {
        System.out.println(first + second + third + first + second + third + first + second);
    }
}
EOF2
# 重构（G12）用：两处一样的 qty * 100（抽变量「替换所有相同的」）、两行可以抽成方法、一个字面量抽成常量
cat > "${C}/Ref.java" <<'EOF2'
package com.demo.common;

public class Ref {
    int price(int qty) {
        int total = qty * 100 + 5;
        int other = qty * 100 + 7;
        return total + other;
    }

    void show(String name) {
        System.out.println("hello " + name);
        System.out.println("bye " + name);
    }
}
EOF2
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

echo "== 补全（docs/IDEA.md 第 3 步）：要 import 的类选中后自动加 import；Lombok 生成的方法带上括号"
# 读 CM6 的补全状态（__lite.completions），不读补全框 —— 测试应用的窗口在后台时帧不走，框不一定画得出来。
# 敲字走 __lite.type（userEvent input.type，和真打字一样触发补全），不发系统按键
ORIG=$(lite_eval main "return __lite.text()")
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('c.setName(')); __lite.caret(l + 1, t[l].length + 1); __lite.type('\\n        ArrayLi'); return true" >/dev/null
T1=$(lite_now_ms)
lite_wait main "return __lite.completions().labels.includes('ArrayList')" 15 >/dev/null
TC=$(( $(lite_now_ms) - T1 ))
ok "$([ "$(lite_eval main "return __lite.completions().labels.includes('ArrayList')")" = true ] && echo 1)" "敲 ArrayLi 出了 ArrayList：${TC} ms（含 CM6 敲字后等 100ms 再问）；前几项 $(lite_eval main "return __lite.completions().labels.slice(0, 4).join(' / ')")"
# 红了把应用日志里 Java 那几行打出来：补全失败时前端是悄悄退回空列表的，原因只在日志里（2026-10-19 撞见过一次 22 秒没候选）
[ "$(lite_eval main "return __lite.completions().labels.includes('ArrayList')")" = true ] || grep '\[java\]' "${HOME}/Library/Logs/${LITE_ID}/app.log" 2>/dev/null | tail -8 | sed 's/^/    日志：/'
sleep 0.3   # CM6 刚弹出 75ms 内不认接受
lite_eval main "return __lite.acceptCompletion()" >/dev/null
lite_wait main "return __lite.text().includes('import java.util.ArrayList;')" 5 >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().includes('import java.util.ArrayList;') && /^\\s+ArrayList$/m.test(__lite.text())")" = true ] && echo 1)" "选中之后：那一行是 ArrayList、文件顶上加了 import java.util.ArrayList;（自动 import 只在 resolve 时才给）"
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => /^\\s+ArrayList$/.test(x)); __lite.caret(l + 1, t[l].length + 1); __lite.type('; c.getNa'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.includes('getName')" 15 >/dev/null
ok "$([ "$(lite_eval main "return __lite.completions().labels.includes('getName')")" = true ] && echo 1)" "c.getNa 出了 getName（Lombok 生成的，jdtls 带着 Lombok 跑才认得）"
sleep 0.3
lite_eval main "const c = __lite.completions(); if (c.labels[0] !== 'getName') return c.labels.join(','); return __lite.acceptCompletion()" >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().includes('c.getName()')")" = true ] && echo 1)" "插的是 getName()（带括号 —— jdtls 的 textEdit，不是只有名字的 insertText）：$(lite_eval main "return __lite.text().split('\\n').find((x) => x.includes('c.getNa')) ?? ''" | sed 's/^ *//')"

echo "== 参数提示：敲 String.format( 弹出重载、当前参数是 format；打完第一个参数和逗号挪到 args；出了括号收起"
SIG='const on = () => [...document.querySelectorAll(".cm-java-sig .row.on b")].map((b) => b.textContent).join();'
lite_eval main "const t = __lite.text().split('\n'); const l = t.findIndex((x) => x.includes('c.setName(')); __lite.caret(l + 1, t[l].length + 1); __lite.type('\n        String.format('); return true" >/dev/null
lite_wait main "${SIG} return on() === 'String format'" 15 >/dev/null
ok "$([ "$(lite_eval main "${SIG} return on()")" = "String format" ] && echo 1)" "敲 ( 弹出参数提示，当前参数 String format：$(lite_eval main "return document.querySelector('.cm-java-sig')?.innerText.split('\n').slice(0, 2).join(' / ') ?? '没弹'")"
lite_eval main "__lite.type('\"%d\", '); return true" >/dev/null
lite_wait main "${SIG} return on() === 'Object... args'" 10 >/dev/null
ok "$([ "$(lite_eval main "${SIG} return on()")" = "Object... args" ] && echo 1)" "打完第一个参数和逗号：当前参数挪到 Object... args"
lite_eval main "__lite.type('1);'); return true" >/dev/null
lite_wait main "return !document.querySelector('.cm-java-sig')" 10 >/dev/null
ok "$([ "$(lite_eval main "return !document.querySelector('.cm-java-sig')")" = true ] && echo 1)" "出了括号就收起"
# ⌘P 手动弹（照 IDEA 的 Parameter Info；2026-10-20 起找文件是 ⇧⌘O）：光标放回括号里第二个参数上
lite_eval main "const t = __lite.text().split('\n'); const l = t.findIndex((x) => x.includes('String.format(')); __lite.caret(l + 1, t[l].indexOf('1);') + 1); __lite.key('Mod-p'); return true" >/dev/null
lite_wait main "${SIG} return on() === 'Object... args'" 10 >/dev/null
ok "$([ "$(lite_eval main "${SIG} return on()")" = "Object... args" ] && echo 1)" "光标放回括号里按 ⌘P：手动弹出来，当前参数是第二个"
lite_eval main "__lite.key('Escape'); return true" >/dev/null
lite_eval main "__lite.setText($(python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' <<< "${ORIG}")); return true" >/dev/null

echo "== 改名（⇧F6，docs/IDEA.md 第 14 节）：record 跨模块改名，文件跟着改名；jdtls 不报错（它从文件监听知道了新文件）；撤销名字和内容都回来"
COMMON="${P}/shop-common/src/main/java/com/demo/common"
MONEY0=$(cat "${COMMON}/Money.java")
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('new Money')); __lite.caret(l + 1, t[l].indexOf('new Money') + 6); __lite.key('Shift-F6'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.rename input')" 15 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelector('.rename .title')?.textContent.includes('Money') && document.querySelector('.rename input')?.value === 'Money'")" = true ] && echo 1)" "⇧F6 弹出改名框，预填 Money（jdtls 的 prepareRename 说这里能改）"
T1=$(lite_now_ms)
lite_eval main "__lite.fill('Cash', '.rename input'); document.querySelector('.rename input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true" >/dev/null
wait_for 60 "[ -f '${COMMON}/Cash.java' ]"
TR=$(( $(lite_now_ms) - T1 ))
ok "$([ -f "${COMMON}/Cash.java" ] && [ ! -e "${COMMON}/Money.java" ] && grep -q 'record Cash(' "${COMMON}/Cash.java" && echo 1)" "shop-common 里 Money.java 改名成 Cash.java、里面是 record Cash：${TR} ms（第一次改名 jdtls 要搜全部索引）；目录里此刻是 $(ls "${COMMON}" | tr '\n' ' ')"
lite_wait main "return __lite.text().includes('new Cash(12345)')" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().includes('new Cash(12345)') && __lite.text().includes('import com.demo.common.Cash;') && !/\\bMoney\\b/.test(__lite.text())")" = true ] && echo 1)" "order-service 里的引用和 import 都改了，一个 Money 都不剩"
sleep 3
lite_wait main "return document.querySelectorAll('.cm-diag-err').length === 0" 20 >/dev/null
ok "$([ -z "$(errs)" ] && echo 1)" "jdtls 不报错：新文件 Cash.java 它认得（文件监听告诉了它）（有的话是：$(errs)）"
lite_eval main "return __lite.click('撤销')" >/dev/null
# 撤销是先改回名字、再写回内容：只等「Money.java 出现」会落在名字回来了、内容还没写的那几毫秒里（2026-10-19 间歇红过一次），要等两样都对
money_back() { [ -f "${COMMON}/Money.java" ] && [ ! -e "${COMMON}/Cash.java" ] && [ "$(cat "${COMMON}/Money.java")" = "${MONEY0}" ]; }
wait_for 15 money_back
ok "$(money_back && echo 1)" "点撤销：文件名改回 Money.java、内容一字不差；目录里此刻是 $(ls "${COMMON}" | tr '\n' ' ')"
lite_wait main "return __lite.text().includes('new Money(12345)')" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().includes('new Money(12345)') && !__lite.text().includes('Cash')")" = true ] && echo 1)" "撤销：order-service 里也回到 Money"
sleep 3
lite_wait main "return document.querySelectorAll('.cm-diag-err').length === 0" 20 >/dev/null
ok "$([ -z "$(errs)" ] && echo 1)" "撤销之后 jdtls 也不报错（有的话是：$(errs)）—— 这条证明不了监听：不告诉 jdtls 的错版本下它也绿，jdtls 眼里一直是改名前的样子，撤销正好回到那儿"
# 撞名：Money → Customer（同一个目录里已经有 Customer.java）：一个字都不改，说原因
CUST0=$(cat "${COMMON}/Customer.java")
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('new Money')); __lite.caret(l + 1, t[l].indexOf('new Money') + 6); __lite.key('Shift-F6'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.rename input')" 15 >/dev/null
lite_eval main "__lite.fill('Customer', '.rename input'); document.querySelector('.rename input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true" >/dev/null
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('没改名')" 30 >/dev/null
ok "$([ "$(lite_eval main "return (document.querySelector('.toast')?.textContent ?? '').includes('Customer.java 已经有了')")" = true ] && [ "$(cat "${COMMON}/Money.java")" = "${MONEY0}" ] && [ "$(cat "${COMMON}/Customer.java")" = "${CUST0}" ] && echo 1)" "撞名（改成已有的 Customer）：一个字都没改，胶囊说「Customer.java 已经有了」：$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''")"

echo "== 格式化（⌥⌘L，docs/IDEA.md 第 15 节）：jdtls 排，照编辑器的缩进（4 个空格，不是 Eclipse 默认的 Tab）；⌘Z 一下全回去；选中几行只排那几行"
FMT0=$(lite_eval main "return __lite.text()")
lite_eval main "__lite.setText(__lite.text().replace('public class OrderApplication {', 'public class OrderApplication {\\nint   x=1;\\n  int y=2;')); return true" >/dev/null
lite_eval main "__lite.caret(1, 1); __lite.key('Alt-Mod-l'); return true" >/dev/null
lite_wait main "return /^    int x = 1;$/m.test(__lite.text())" 15 >/dev/null
ok "$([ "$(lite_eval main "return /^    int x = 1;$/m.test(__lite.text()) && /^    int y = 2;$/m.test(__lite.text()) && !/\\t/.test(__lite.text())")" = true ] && echo 1)" "整份排好了：两行都是 4 个空格缩进、= 两边有空格、一个 Tab 都没有：$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''")"
lite_eval main "__lite.key('Mod-z'); return true" >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().includes('int   x=1;') && __lite.text().includes('  int y=2;')")" = true ] && echo 1)" "⌘Z 一下全回去（一笔事务）"
# 只选中 x 那一行：y 那行不动
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('int   x=1;')) + 1; __lite.select(l, 1, l, t[l - 1].length + 1); __lite.key('Alt-Mod-l'); return true" >/dev/null
lite_wait main "return /^    int x = 1;$/m.test(__lite.text())" 15 >/dev/null
ok "$([ "$(lite_eval main "return /^    int x = 1;$/m.test(__lite.text()) && __lite.text().includes('\\n  int y=2;')")" = true ] && echo 1)" "选中 x 那一行：只排了它，y 那行原样"
lite_eval main "__lite.setText($(python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()[:-1]))' <<< "${FMT0}")); return true" >/dev/null

echo "== 快速修复（⌥↵，docs/IDEA.md 第 16 节）：缺 import 的 List —— 第一条是 java.util（jdtls 按字母会把 com.sun 排前面）；回车补上 import、波浪线消失；⌘Z 一下撤回"
QF0=$(lite_eval main "return __lite.text()")
lite_eval main "__lite.setText(__lite.text().replace('SpringApplication.run(OrderApplication.class, args);', 'SpringApplication.run(OrderApplication.class, args);\\n        List<String> names = null;')); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('.cm-diag-err')].some((e) => e.textContent === 'List')" 20 >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('List<String> names')); __lite.caret(l + 1, t[l].indexOf('List') + 2); __lite.key('Alt-Enter'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.fixes .row').length > 0" 20 >/dev/null
ROWS=$(lite_eval main "return [...document.querySelectorAll('.fixes .row')].slice(0, 4).map((r) => r.textContent).join(' / ')")
ok "$([ "$(lite_eval main "return document.querySelector('.fixes .row')?.textContent === \"Import 'List' (java.util)\" && ![...document.querySelectorAll('.fixes .row')].some((r) => r.textContent.startsWith('Generate Getters'))")" = true ] && echo 1)" "⌥↵ 第一条是 Import 'List' (java.util)，要弹框的「生成 getter」不列：${ROWS} …"
lite_eval main "document.querySelector('.fixes').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true" >/dev/null
lite_wait main "return __lite.text().includes('import java.util.List;')" 15 >/dev/null
lite_wait main "return ![...document.querySelectorAll('.cm-diag-err')].some((e) => e.textContent === 'List')" 15 >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().includes('import java.util.List;') && ![...document.querySelectorAll('.cm-diag-err')].some((e) => e.textContent === 'List')")" = true ] && echo 1)" "回车：补上 import java.util.List;、List 底下的波浪线没了"
lite_eval main "__lite.key('Mod-z'); return true" >/dev/null
ok "$([ "$(lite_eval main "return !__lite.text().includes('import java.util.List;') && __lite.text().includes('List<String> names')")" = true ] && echo 1)" "⌘Z 一下撤回（改在编辑器里，没写盘）"
lite_eval main "__lite.setText($(python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()[:-1]))' <<< "${QF0}")); return true" >/dev/null

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

echo "== ⌥⌘B 跳到实现、⌘U 跳到父方法（docs/TAKEOVER.md G1）：接口上列出两个实现；实现里 ⌘U 回到接口；覆盖 toString 的 ⌘U 进 JDK 的 Object"
open_file "${C}/PriceRule.java" >/dev/null
caret_on 'PriceRule {' >/dev/null
lite_eval main "__lite.key('Mod-Alt-b'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.usages .row').length >= 2" 20 >/dev/null
IMPL=$(lite_eval main "return [...document.querySelectorAll('.usages .row .file')].map((r) => r.innerText).sort().join(',')")
ok "$(echo "${IMPL}" | grep -q 'HalfRule.java' && echo "${IMPL}" | grep -q 'VipRule.java' && echo 1)" "接口名上 ⌥⌘B：两个实现都列出来：${IMPL}"
lite_eval main "document.querySelector('.usages').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null
caret_on 'apply' >/dev/null
lite_eval main "__lite.key('Mod-Alt-b'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.usages .row').length >= 2" 20 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.usages .row').length")" = 2 ] && echo 1)" "接口方法上 ⌥⌘B：两个实现方法：$(lite_eval main "return [...document.querySelectorAll('.usages .row')].map((r) => r.innerText.replace(/\\s+/g, ' ')).join(' / ')")"
lite_eval main "document.querySelector('.usages').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null
open_file "${C}/VipRule.java" >/dev/null
caret_on 'apply' >/dev/null
lite_eval main "__lite.key('Mod-u'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path.endsWith('/PriceRule.java')" 20 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path.endsWith('/PriceRule.java') && __lite.where().startsWith('5:')")" = true ] && echo 1)" "实现方法上 ⌘U：一处直接跳到接口的 apply（第 5 行）：$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")"
open_file "${C}/Coupon.java" >/dev/null
caret_on 'toString' >/dev/null
lite_eval main "__lite.key('Mod-u'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.mode === 'class'" 20 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.mode === 'class' && /^Object\\.(java|class)$/.test(__lite.tabs.active.name) && __lite.text().includes('public String toString()')")" = true ] && echo 1)" "覆盖 toString 的 ⌘U：进 JDK 的 Object（只读标签）：$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")"

echo "== F1 快速文档（docs/TAKEOVER.md G3）：签名 + Javadoc；Javadoc 里的 HTML 标签不当 HTML 画（<b> 变成粗体那一段的字，不出现标签本身）"
open_file "${C}/PriceRule.java" >/dev/null
caret_on 'apply' >/dev/null
lite_eval main "__lite.key('F1'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.popup.doc')" 20 >/dev/null
DOC=$(lite_eval main "return document.querySelector('.popup.doc')?.innerText.replace(/\\s+/g, ' ') ?? ''")
ok "$(echo "${DOC}" | grep -q 'apply(long cents)' && echo "${DOC}" | grep -q '折后价' && ! echo "${DOC}" | grep -q '<b>' && echo 1)" "签名和 Javadoc 都在、没有 HTML 标签原样露出来：${DOC}"
ok "$([ "$(lite_eval main "return [...document.querySelectorAll('.popup.doc b')].some((b) => b.textContent.includes('折后价'))")" = true ] && echo 1)" "Javadoc 里 <b>折后价</b> → jdtls 转成 Markdown 的粗体 → 画成粗体（走文本插值，不插 HTML）"
lite_eval main "document.querySelector('.popup.doc').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null

echo "== 同名高亮（docs/TAKEOVER.md G11）：光标停在字段上，这个字段出现的地方都标出来、写的那处换颜色；另一个方法里同名的参数不标（语义的，不是按字找）"
open_file "${C}/Tally.java" >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('int total;')); __lite.caret(l + 1, t[l].indexOf('total') + 2); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-usage, .cm-usage-w').length >= 3" 20 >/dev/null
HL=$(lite_eval main "return [...document.querySelectorAll('.cm-line')].map((l, i) => [...l.querySelectorAll('.cm-usage, .cm-usage-w')].map((m) => (i + 1) + (m.classList.contains('cm-usage-w') ? '写' : '读')).join(' ')).filter(Boolean).join(' ')")
ok "$(echo "${HL}" | grep -q '6写' && echo "${HL}" | grep -q '6读' && echo "${HL}" | grep -q '7读' && ! echo "${HL}" | grep -qE '(^| )9[读写]' && echo 1)" "字段 total：第 6 行一写一读、第 7 行读，第 9 行同名的参数不标：${HL}"
lite_eval main "__lite.caret(5, 1); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-usage, .cm-usage-w').length === 0" 5 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-usage, .cm-usage-w').length")" = 0 ] && echo 1)" "光标挪到空白处：标记收掉"

echo "== 参数名提示（docs/TAKEOVER.md G11）：传字面量的地方前面一小块灰字写参数名（add(n: 2)、other(total: 3)），不是正文"
lite_wait main "return document.querySelectorAll('.cm-inlay').length >= 2" 20 >/dev/null
INL=$(lite_eval main "return [...document.querySelectorAll('.cm-line')].map((l, i) => [...l.querySelectorAll('.cm-inlay')].map((m) => (i + 1) + ':' + m.textContent).join(' ')).filter(Boolean).join(' ')")
ok "$([ "${INL}" = '10:n: 10:total:' ] && echo 1)" "第 10 行 add(2)、other(3) 前面各一个，别处没有：${INL}"
ok "$([ "$(lite_eval main "return !__lite.text().includes('n: 2') && __lite.text().includes('add(2)')")" = true ] && echo 1)" "提示不进正文（存盘、复制拿到的还是 add(2)）"
lite_eval main "await window.__TAURI_INTERNALS__.invoke('settings_apply', { root: __lite.project.root, global: { 'editor.inlayHints': false }, project: null }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-inlay').length === 0" 10 >/dev/null
OFF=$(lite_eval main "return document.querySelectorAll('.cm-inlay').length")
lite_eval main "await window.__TAURI_INTERNALS__.invoke('settings_apply', { root: __lite.project.root, global: { 'editor.inlayHints': null }, project: null }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-inlay').length === 2" 10 >/dev/null
ON=$(lite_eval main "return document.querySelectorAll('.cm-inlay').length")
ok "$([ "${OFF}" = 0 ] && [ "${ON}" = 2 ] && echo 1)" "设置里关掉 editor.inlayHints：当场收（${OFF}）；恢复默认：回来（${ON}）—— 走的是写 settings.json → 文件监听 → 广播"

echo "== 生成 ⌃↵（docs/TAKEOVER.md G5）：Getter 和 Setter（final 的字段只有 getter）、toString()、构造器；字段默认全勾，生成的落进编辑器"
G='.popup.gen'
gk() { lite_eval main "document.querySelector('${G}').dispatchEvent(new KeyboardEvent('keydown', { key: '$1', bubbles: true, cancelable: true })); return true" >/dev/null; }
gen_pick() {
  lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('private int x')); __lite.caret(l + 1, 5); __lite.key('Ctrl-Enter'); return true" >/dev/null
  lite_wait main "return !!document.querySelector('${G} .mi')" 10 >/dev/null
  # 不用 seq：macOS 的 `seq 1 0` 是倒着数的（输出 1、0 两行），「选第 0 项」会多按两下 ↓（2026-10-22 撞的）
  local n=0; while [ "${n}" -lt "$1" ]; do gk ArrowDown; n=$((n + 1)); done
  gk Enter
  lite_wait main "return !!document.querySelector('${G} .foot') || !!document.querySelector('${G} .err')" 20 >/dev/null
}
open_file "${C}/Point.java" >/dev/null
gen_pick 3
ITEMS=$(lite_eval main "return [...document.querySelectorAll('${G} .list .nm')].map((e) => e.textContent).join(',') + (document.querySelector('${G} .err')?.textContent ?? '')")
gk Enter
lite_wait main "return !document.querySelector('${G}') && __lite.text().includes('getX()')" 20 >/dev/null
TXT=$(lite_eval main "return ['int getX()', 'void setX(int x)', 'String getName()', 'setName'].map((s) => __lite.text().includes(s) ? 1 : 0).join('')")
ok "$([ "${TXT}" = 1110 ] && echo 1)" "Getter 和 Setter：getX / setX / getName 都有、final 的 name 没有 setName（${TXT}）；列出的字段：${ITEMS}"
gen_pick 5
gk Enter
lite_wait main "return !document.querySelector('${G}') && __lite.text().includes('toString()')" 20 >/dev/null
ok "$([ "$(lite_eval main "return /public String toString\\(\\)/.test(__lite.text()) && /x=/.test(__lite.text()) && /name=/.test(__lite.text())")" = true ] && echo 1)" "toString()：两个字段都在里面"
gen_pick 0
CITEMS=$(lite_eval main "return [...document.querySelectorAll('${G} .list .nm')].map((e) => e.textContent).join(',') + (document.querySelector('${G} .err')?.textContent ?? '') + (document.querySelector('${G} .empty-msg')?.textContent ?? '')")
gk Enter
lite_wait main "return !document.querySelector('${G}') && /public Point\\(/.test(__lite.text())" 20 >/dev/null
ok "$([ "$(lite_eval main "return /public Point\\(int x\\)/.test(__lite.text())")" = true ] && echo 1)" "构造器：带 x（带初值的 final 字段 jdtls 不列：${CITEMS}）：$(lite_eval main "return __lite.text().split('\\n').find((l) => l.includes('public Point(')) ?? '没有'")"
ok "$([ "$(lite_eval main "return __lite.tabs.active?.dirty === true")" = true ] && echo 1)" "改动在编辑器里（没存盘，⌘Z 撤得回）"
gen_pick 4
gk Enter
lite_wait main "return !document.querySelector('${G}') && __lite.text().includes('hashCode()')" 20 >/dev/null
ok "$([ "$(lite_eval main "return /public boolean equals\\(Object/.test(__lite.text()) && /public int hashCode\\(\\)/.test(__lite.text())")" = true ] && echo 1)" "equals() 和 hashCode()：两个都生成了"
gen_pick 6
OV=$(lite_eval main "return [...document.querySelectorAll('${G} .list .mi')].map((e) => (e.querySelector('input').checked ? '✓' : '') + e.querySelector('.nm').textContent).join(',') + (document.querySelector('${G} .err')?.textContent ?? '')")
ok "$(echo "${OV}" | grep -q 'clone()' && ! echo "${OV}" | grep -q '✓' && echo 1)" "覆盖方法：列出 Object 的（clone 这些），没有抽象方法时一项都不默认勾：${OV}"
gk Escape

echo "== 代码模板（docs/TAKEOVER.md G21）：敲 sout 按 Tab 展开（补全框开着也展开）；jdtls 自带的后缀 .var 出得来；IDEA 的 .sout（jdtls 没有）也有"
open_file "${C}/Tpl.java" >/dev/null
lite_eval main "__lite.caret(5, 9); __lite.type('sout'); return true" >/dev/null
sleep 0.6
lite_eval main "__lite.key('Tab'); return true" >/dev/null
lite_wait main "return __lite.text().includes('System.out.println();')" 5 >/dev/null
lite_eval main "__lite.type('name'); return true" >/dev/null
ok "$([ "$(lite_eval main "return __lite.text().split('\\n')[4].trim()")" = 'System.out.println(name);' ] && echo 1)" "sout + Tab：展开、光标停在括号里（接着打的 name 进了括号）：$(lite_eval main "return __lite.text().split('\\n')[4].trim()")"
lite_eval main "const t = __lite.text().split('\\n'); __lite.caret(5, t[4].length + 1); __lite.type('\\n        name.length().va'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.some((l) => /^var\b/.test(l))" 20 >/dev/null
VAR=$(lite_eval main "return __lite.completions().labels.filter((l) => /^var\b/.test(l)).join(' / ')")
ok "$([ -n "${VAR}" ] && echo 1)" "jdtls 自带的后缀 .var 在补全框里：${VAR}"
sleep 0.3
lite_eval main "const c = __lite.completions(); const i = c.labels.findIndex((l) => /^var\b/.test(l)); if (i !== 0) return c.labels.join(','); return __lite.acceptCompletion()" >/dev/null
# 整行比：第一版用「含有」判，`name.length().int length = name.length();`（前面的表达式没删）照样算过
lite_wait main "return __lite.text().split('\\n').some((l) => /^int \\w+ = name\\.length\\(\\);$/.test(l.trim()))" 10 >/dev/null
ok "$(lite_eval main "return __lite.text().split('\\n').some((l) => /^int \\w+ = name\\.length\\(\\);$/.test(l.trim()))" | grep -q true && echo 1)" ".var：整行换成 int 变量 = name.length();（类型是 jdtls 推的、前面的表达式删掉）：$(lite_eval main "return __lite.text().split('\\n').find((l) => l.includes('name.length()'))?.trim() ?? ''")"
lite_eval main "__lite.key('Escape'); const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('name.length()')); __lite.caret(l + 1, t[l].length + 1); __lite.type('\\n        name.sou'); return true" >/dev/null
lite_wait main "return __lite.completions().labels.includes('.sout')" 15 >/dev/null
sleep 0.3
lite_eval main "const c = __lite.completions(); if (c.labels[0] !== '.sout') return c.labels.join(','); return __lite.acceptCompletion()" >/dev/null
ok "$(lite_eval main "return __lite.text().split('\\n').filter((l) => l.trim() === 'System.out.println(name);').length" | grep -q '^2$' && echo 1)" "name.sou → .sout：整段换成 System.out.println(name);（第二处）：$(lite_eval main "return __lite.completions().labels.slice(0, 4).join(' / ')")"

echo "== 调用层级 ⌃⌥H、类型层级 ⌃H（docs/TAKEOVER.md G10）：底部「层级」工具窗；调用方带调用的那一行、↵ 跳过去；换成「被调用」；接口上 ⌃H 列出两个实现"
H='.hier [role="tree"]'
hrows() { lite_eval main "return [...document.querySelectorAll('${H} .hrow')].map((r) => r.querySelector('.name').textContent + '@' + r.querySelector('.file').textContent).join(' ')"; }
open_file "${C}/Tally.java" >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('int add(')); __lite.caret(l + 1, t[l].indexOf('add') + 2); __lite.key('Ctrl-Alt-h'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('${H} .hrow').length >= 2" 20 >/dev/null
ROWS=$(hrows)
ok "$([ "${ROWS}" = 'add(int) : int@Tally.java twice() : int@Tally.java:10' ] && echo 1)" "add 的调用方：twice，带调用那一行（第 10 行）；名字带签名（jdtls 给的，同 IDEA）：${ROWS}"
lite_eval main "const t = document.querySelector('${H}'); t.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); t.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); return true" >/dev/null
lite_wait main "return __lite.where().startsWith('10:')" 10 >/dev/null
ok "$([ "$(lite_eval main "const [l, c] = __lite.where().split(':').map(Number); return __lite.text().split('\\n')[l - 1].slice(c - 1).startsWith('add(2)')")" = true ] && echo 1)" "↵：跳到调用 add 的那一处（不是 twice 的开头）：$(lite_eval main "return __lite.where()")"
lite_eval main "__lite.click('被调用', 'any'); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('${H} .hrow')].length >= 1 && !document.querySelector('${H} .spinner')" 20 >/dev/null
ok "$([ "$(hrows)" = 'add(int) : int@Tally.java' ] && echo 1)" "换成「被调用」：add 自己不调项目里的方法，只剩根：$(hrows) / $(lite_eval main "return document.querySelector('${H} .sub')?.textContent ?? ''")"
open_file "${C}/PriceRule.java" >/dev/null
caret_on 'PriceRule {' >/dev/null
lite_eval main "__lite.key('Ctrl-h'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('${H} .hrow').length >= 3" 20 >/dev/null
TROWS=$(lite_eval main "return [...document.querySelectorAll('${H} .hrow .name')].map((r) => r.textContent).slice(1).sort().join(',')")
ok "$([ "${TROWS}" = 'HalfRule,VipRule' ] && [ "$(lite_eval main "return document.querySelector('${H} .hrow .name')?.textContent + '/' + document.querySelector('.hier .segs .on')?.textContent")" = 'PriceRule/子类型' ] && echo 1)" "接口上 ⌃H：默认看子类型，两个实现：${TROWS}"
ok "$([ "$(lite_eval main "return __lite.text().includes('public interface PriceRule {')")" = true ] && echo 1)" "⌃H 在 Java 文件里不再是「往回删一个字」"

echo "== ⇧⇧ 找类（docs/TAKEOVER.md G2）：问 jdtls（驼峰缩写也认，接口标「接口」）；默认只找项目的，「含 jar 里的」找得到依赖里的、↵ 开成只读标签"
Q='[aria-label="随处搜索"]'
lite_eval main "__lite.overlay.openQuick('class', 'PrRu'); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('${Q} .row')].some((r) => r.innerText.includes('PriceRule'))" 20 >/dev/null
ROWS=$(lite_eval main "return [...document.querySelectorAll('${Q} .row')].map((r) => r.innerText.replace(/\\s+/g, ' ')).join(' / ')")
ok "$(echo "${ROWS}" | grep -q '接口 PriceRule com.demo.common' && echo 1)" "驼峰缩写 PrRu → PriceRule，种类是接口、右边是包名：${ROWS}"
lite_eval main "__lite.fill('SpringApplication', '${Q} input'); return true" >/dev/null
lite_wait main "return !!document.querySelector('${Q} .empty-msg')?.innerText.includes('含 jar 里的')" 20 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('${Q} .row').length")" = 0 ] && echo 1)" "默认只找项目自己的：SpringApplication 不在项目里，空着、说可以连 jar 一起找"
lite_eval main "__lite.click('含 jar 里的'); return true" >/dev/null
lite_wait main "return [...document.querySelectorAll('${Q} .row')].some((r) => r.innerText.includes('spring-boot'))" 20 >/dev/null
JAR=$(lite_eval main "return document.querySelector('${Q} .row')?.innerText.replace(/\\s+/g, ' ') ?? ''")
ok "$(echo "${JAR}" | grep -q 'SpringApplication org.springframework.boot · spring-boot-4.0.1.jar' && echo 1)" "点「含 jar 里的」：依赖里的找到了、标出是哪个 jar：${JAR}"
# 点名字对得上的那一行，不按 ↵：↵ 选的是「当前项」，2026-10-22 有一次检查时第一行是 SpringApplication、↵ 开的却是 SpringApplicationBannerPrinter ——
# 当前项被挪走了。打一行出来：当前项是哪个
echo "    （当前项：$(lite_eval main "return document.querySelector('${Q} .row.sel .main')?.textContent ?? '无'")）"
lite_eval main "[...document.querySelectorAll('${Q} .row')].find((r) => r.querySelector('.main')?.textContent === 'SpringApplication')?.click(); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.mode === 'class'" 20 >/dev/null
lite_wait main "return /class SpringApplication\\b/.test(__lite.text().split('\\n')[parseInt(__lite.where()) - 1] ?? '')" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.mode === 'class' && /^SpringApplication\\.(java|class)$/.test(__lite.tabs.active.name) && /class SpringApplication\\b/.test(__lite.text().split('\\n')[parseInt(__lite.where()) - 1] ?? '')")" = true ] && echo 1)" "↵：开成只读标签、停在类声明那行（jdtls 给 jar 里的类的位置是文件开头，同 ⌘B 停的地方）：$(lite_eval main "return __lite.tabs.active?.name + ' ' + __lite.where()")"
# 延迟：每个词问一次（项目里的、连 jar 一起的各算一次），热的 jdtls
LAT=$(lite_eval main "const qs = ['Order', 'Cust', 'PrRu', 'Vip', 'Half', 'Coup', 'Str', 'List', 'Map', 'Spring', 'Appl', 'Obj']; const ms = []; for (const q of qs) for (const libs of [false, true]) { const t = performance.now(); await window.__TAURI_INTERNALS__.invoke('java_types', { root: __lite.project.root, query: q, libs }); ms.push(performance.now() - t); } ms.sort((a, b) => a - b); return Math.round(ms[Math.floor(ms.length / 2)]) + ' ' + Math.round(ms[Math.floor(ms.length * 0.95)]) + ' ' + Math.round(ms[ms.length - 1])")
read -r P50 P95 PMAX <<< "${LAT}"
ok "$([ "${P95:-9999}" -lt 500 ] && echo 1)" "一次找类（24 次，热的 jdtls）：p50 ${P50} ms、p95 ${P95} ms、最慢 ${PMAX} ms（p95 要在 500 以内）"

echo "== 重构（docs/TAKEOVER.md G12）：⌥⌘V 抽变量（几种做法弹出来挑）、抽完接着改名；⌥⌘M 抽方法、⌥⌘C 抽常量，同样接着改名；⌥⌘N 内联回去"
open_file "${C}/Ref.java" >/dev/null
SEL='const sel = (needle, part) => { const t = __lite.text().split("\n"); const l = t.findIndex((x) => x.includes(needle)); const c = t[l].indexOf(part); __lite.select(l + 1, c + 1, l + 1, c + 1 + part.length); };'
RENAME() { lite_wait main "return !!document.querySelector('.rename input')" 20 >/dev/null; [ "$(lite_eval main "return !!document.querySelector('.rename input')")" = true ] || echo "    （诊断：没开改名框；胶囊「$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''")」；Ref.java 此刻：$(lite_eval main "return __lite.text().split('\\n').slice(3, 16).join(' ⏎ ')")）"; N0=$(lite_eval main "return document.querySelector('.rename input')?.value ?? ''"); lite_eval main "__lite.fill('$1', '.rename input'); document.querySelector('.rename input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true" >/dev/null; }
lite_eval main "${SEL} sel('qty * 100 + 5', 'qty * 100'); __lite.key('Alt-Mod-v'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('[role=option]').length >= 2" 20 >/dev/null
OPTS=$(lite_eval main "return [...document.querySelectorAll('[role=option]')].map((o) => o.textContent.trim()).join(' / ')")
ok "$(echo "${OPTS}" | grep -q '替换所有相同的' && echo 1)" "⌥⌘V：几种做法弹出来挑、标题是中文：${OPTS}"
lite_eval main "const o = [...document.querySelectorAll('[role=option]')].find((x) => x.textContent.includes('替换所有相同的')); o.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); o.click(); return true" >/dev/null
RENAME base
lite_wait main "return /int base = qty \\* 100;/.test(__lite.text())" 20 >/dev/null
ok "$([ "$(lite_eval main "const t = __lite.text(); return /int base = qty \\* 100;/.test(t) && t.includes('int total = base + 5;') && t.includes('int other = base + 7;') && !/\\t/.test(t)")" = true ] && echo 1)" "抽完光标停在新名字上、改名框直接开着（jdtls 起的名字是「${N0}」），打 base 回车：声明一行、两处都换成 base；抽出来的代码是空格缩进（Eclipse 默认是 Tab）$([ "$(lite_eval main "return /\\t/.test(__lite.text())")" = true ] && echo "；此刻：$(lite_eval main "return JSON.stringify(__lite.text().split('\\n').slice(3, 9))")")"
lite_wait main "return !document.querySelector('.rename')" 10 >/dev/null; sleep 1
lite_eval main "const t = __lite.text().split('\\n'); const a = t.findIndex((x) => x.includes('hello ')); const b = t.findIndex((x) => x.includes('bye ')); __lite.select(a + 1, t[a].indexOf('System') + 1, b + 1, t[b].length + 1); __lite.key('Alt-Mod-m'); return true" >/dev/null
RENAME greet
lite_wait main "return __lite.text().includes('greet(name);')" 20 >/dev/null
ok "$([ "$(lite_eval main "const t = __lite.text(); return t.includes('greet(name);') && /^    private void greet\\(String name\\)/m.test(t) && !/extracted/.test(t) && !/\\t/.test(t)")" = true ] && echo 1)" "⌥⌘M：两行抽成方法、接着改名（jdtls 起的是「${N0}」）：调用处 greet(name);、声明 private void greet(String name)$([ "$(lite_eval main "return /\\t/.test(__lite.text())")" = true ] && echo "；此刻：$(lite_eval main "return JSON.stringify(__lite.text().split('\\n').slice(9, 20))")")"
# 上一步的改名是异步落进编辑器的：落下来那一下会把选区换掉。等改名框关了、正文不再变再选（一次间歇红：选中之后被换掉，⌥⌘C 没有选区什么都没做）
lite_wait main "return !document.querySelector('.rename')" 10 >/dev/null; sleep 1
lite_eval main "${SEL} sel('\"bye \"', '\"bye \"'); __lite.key('Alt-Mod-c'); return true" >/dev/null
RENAME BYE
lite_wait main "return __lite.text().includes('BYE + name')" 20 >/dev/null
ok "$([ "$(lite_eval main "const t = __lite.text(); return /static final String BYE = \"bye \";/.test(t) && t.includes('BYE + name')")" = true ] && echo 1)" "⌥⌘C：字面量抽成常量、接着改名（jdtls 起的是「${N0}」）：static final String BYE = \"bye \";"
lite_wait main "return !document.querySelector('.rename')" 10 >/dev/null; sleep 1
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('int total = base')); __lite.caret(l + 1, t[l].indexOf('base') + 2); __lite.key('Alt-Mod-n'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('[role=option]').length > 0 || !__lite.text().includes('int base')" 20 >/dev/null
lite_eval main "const o = document.querySelector('[role=option]'); if (o) { o.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); o.click(); } return true" >/dev/null
lite_wait main "return !__lite.text().includes('int base')" 20 >/dev/null
ok "$([ "$(lite_eval main "const t = __lite.text(); return !t.includes('int base') && t.includes('int total = qty * 100 + 5;') && t.includes('int other = qty * 100 + 7;')")" = true ] && echo 1)" "⌥⌘N：base 内联回去，声明没了、两处换回 qty * 100（没接着开改名框：$(lite_eval main "return document.querySelector('.rename') ? '开了' : '没开'")）"

echo "== 代码风格照 IDEA（docs/TAKEOVER.md G22）：项目的 .idea/codeStyles 写着续行 4、行宽 80、3 个类合成 .*；⌃⌥O 删没用的、合成 java.util.*、static 放最后；⌥⌘L 照行宽折行、续行缩 4"
open_file "${C}/Sty.java" >/dev/null
lite_eval main "__lite.caret(1, 1); __lite.key('Ctrl-Alt-o'); return true" >/dev/null
lite_wait main "return __lite.text().includes('import java.util.*;')" 20 >/dev/null
IMPS=$(lite_eval main "return __lite.text().split('\\n').filter((l) => l.startsWith('import') || l === '').slice(0, 6).join(' | ')")
ok "$([ "$(lite_eval main "const t = __lite.text(); return t.includes('import java.util.*;') && !t.includes('import java.util.List;') && !t.includes('import com.demo.common.Money;') && t.indexOf('import java.util.*;') < t.indexOf('import static java.util.Collections.emptyList;')")" = true ] && echo 1)" "⌃⌥O：4 个 java.util 的类合成一行 .*（阈值 3）、同包的 Money 删掉、static 排在最后：${IMPS}"
lite_eval main "__lite.key('Ctrl-Alt-o'); return true" >/dev/null; sleep 1
ok "$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''" | grep -q '已经是这个样子' && echo 1)" "再按一次：没什么可改的，说一句（不是「已优化」）"
lite_eval main "__lite.caret(1, 1); __lite.key('Alt-Mod-l'); return true" >/dev/null
lite_wait main "return !__lite.text().split('\\n').some((l) => l.length > 80)" 15 >/dev/null
WRAP=$(lite_eval main "const t = __lite.text().split('\\n'); const i = t.findIndex((l) => l.includes('System.out.println(')); return t.slice(i, i + 2).map((l) => l.length + ':' + (l.match(/^ */)[0].length)).join(' ')")
ok "$([ "$(lite_eval main "const t = __lite.text().split('\\n'); const i = t.findIndex((l) => l.includes('System.out.println(')); return t.every((l) => l.length <= 80) && /^ {12}\\S/.test(t[i + 1] ?? '')")" = true ] && echo 1)" "⌥⌘L：没有一行超过 80、折下来那行缩进 8 + 4 = 12（Eclipse 默认是 120 不折、续行 16）（长度:缩进）：${WRAP}"
lite_eval main "__lite.key('Mod-z'); __lite.key('Mod-z'); return true" >/dev/null

echo "== 移动类（F6，docs/TAKEOVER.md G12）：挑包、先看要改哪些、点「移动」—— 文件挪进 util、包声明和 order-service 里的 import 跟着改、jdtls 不报错；撤销挪回来"
LUSE="${P}/order-service/src/main/java/com/demo/order/LabelUse.java"
LABEL0=$(cat "${C}/Label.java"); LUSE0=$(cat "${LUSE}")
open_file "${C}/Label.java" >/dev/null
lite_eval main "__lite.caret(3, 2); __lite.key('F6'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.dialog.move .row').length > 0" 30 >/dev/null
lite_eval main "__lite.fill('common.util', '.dialog.move .field'); return true" >/dev/null
sleep 0.3
ok "$([ "$(lite_eval main "return [...document.querySelectorAll('.dialog.move .row .pkg')].map((e) => e.textContent).join(',')")" = "com.demo.common.util" ] && echo 1)" "F6 列出项目里的包（jdtls 的 java/getMoveDestinations），筛「common.util」剩一个；现在在的 com.demo.common 不在可选里"
lite_eval main "document.querySelector('.dialog.move .field').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true" >/dev/null
lite_wait main "return !!document.querySelector('.dialog.move .files')" 60 >/dev/null
PREV=$(lite_eval main "return [...document.querySelectorAll('.dialog.move .files li')].map((e) => e.textContent).join(',')")
ok "$([ "${PREV}" = "order-service/src/main/java/com/demo/order/LabelUse.java,shop-common/src/main/java/com/demo/common/Label.java" ] && [ "$(cat "${C}/Label.java")" = "${LABEL0}" ] && echo 1)" "预览：要改 Label.java（包声明）和 LabelUse.java（import），盘上一个字还没动：${PREV}"
lite_eval main "document.querySelector('.dialog.move .btn.primary').click(); return true" >/dev/null
wait_for 30 "[ -f '${C}/util/Label.java' ]"
lite_wait main "return __lite.tabs.active?.path === '${C}/util/Label.java'" 10 >/dev/null
ok "$([ -f "${C}/util/Label.java" ] && [ ! -e "${C}/Label.java" ] && grep -q '^package com.demo.common.util;' "${C}/util/Label.java" && grep -q '^import com.demo.common.util.Label;' "${LUSE}" && echo 1)" "点「移动」：Label.java 挪进 util/、包声明改了、LabelUse 的 import 改成 com.demo.common.util.Label；标签跟着到新路径"
open_file "${LUSE}" >/dev/null
sleep 3
lite_wait main "return document.querySelectorAll('.cm-diag-err').length === 0" 30 >/dev/null
ok "$([ -z "$(errs)" ] && echo 1)" "LabelUse 里 jdtls 不报错（挪过去的类它认得）（有的话是：$(errs)）"
# 走菜单那条常驻的路：胶囊上的「撤销」停 10 秒，上面等 jdtls 报错（最多 30 秒）的工夫它早走了 —— 第一版点胶囊，点了个空
lite menu undo-replace main >/dev/null
label_back() { [ -f "${C}/Label.java" ] && [ ! -e "${C}/util/Label.java" ] && [ "$(cat "${C}/Label.java")" = "${LABEL0}" ] && [ "$(cat "${LUSE}")" = "${LUSE0}" ]; }
wait_for 15 label_back
ok "$(label_back && echo 1)" "「编辑 › 撤销上次的替换 / 改名」：Label.java 挪回 common/、两个文件的内容一字不差$(label_back || echo "；胶囊此刻：$(lite_eval main "return document.querySelector('.toast')?.textContent ?? '（没有）'")；目录：$(ls "${C}" | tr '\n' ' ')")"

echo "== 改签名（⌘F6，docs/TAKEOVER.md G12）：改名、对调两个参数、加一个带默认值的 —— 先列要改哪些、盘上不动；点了之后声明和另一个模块里的调用一起改、jdtls 不报错；撤销回来"
FUSE="${P}/order-service/src/main/java/com/demo/order/FeeUse.java"
FEE0=$(cat "${C}/Fee.java"); FUSE0=$(cat "${FUSE}")
open_file "${C}/Fee.java" >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('calc(')); __lite.caret(l + 1, t[l].indexOf('calc') + 2); __lite.key('Mod-F6'); return true" >/dev/null
lite_wait main "return !!document.querySelector('.csig #csig-name')" 30 >/dev/null
ok "$([ "$(lite_eval main "const d = document.querySelector('.csig'); return d.querySelector('#csig-name').value + '|' + [...d.querySelectorAll('.prow')].map((r) => r.querySelector('.ty').value + ' ' + r.querySelector('.nm').value).join(',')")" = "calc|int cents,String code" ] && echo 1)" "⌘F6 弹出改签名，读进来的是 jdtls 给的（java/getChangeSignatureInfo）：calc(int cents, String code)"
lite_eval main "__lite.fill('fee', '.csig #csig-name'); [...document.querySelectorAll('.csig .prow')][0].querySelector('[aria-label^=\"把 cents 往后\"]').click(); return true" >/dev/null
sleep 0.2
lite_eval main "[...document.querySelectorAll('.csig button')].find((b) => b.textContent.includes('加一个')).click(); return true" >/dev/null
sleep 0.2
lite_eval main "__lite.fill('boolean', '.csig .prow:nth-child(3) .ty'); return true" >/dev/null
lite_eval main "__lite.fill('vip', '.csig .prow:nth-child(3) .nm'); return true" >/dev/null
lite_eval main "__lite.fill('false', '.csig .prow:nth-child(3) .dv'); return true" >/dev/null
sleep 0.2
SIGL=$(lite_eval main "return document.querySelector('.csig .sig code')?.textContent ?? ''")
ok "$([ "${SIGL}" = "public long fee(String code, int cents, boolean vip)" ] && echo 1)" "表里改完，底下那行实时拼出新签名：${SIGL}"
lite_eval main "[...document.querySelectorAll('.csig button')].find((b) => b.textContent === '下一步').click(); return true" >/dev/null
lite_wait main "return !!document.querySelector('.csig .files') || !!document.querySelector('.csig .empty-msg.bad')" 60 >/dev/null
PREV=$(lite_eval main "return [...document.querySelectorAll('.csig .files li')].map((e) => e.textContent).join(',') || document.querySelector('.csig .empty-msg.bad')?.textContent")
ok "$([ "${PREV}" = "order-service/src/main/java/com/demo/order/FeeUse.java,shop-common/src/main/java/com/demo/common/Fee.java" ] && [ "$(cat "${C}/Fee.java")" = "${FEE0}" ] && echo 1)" "下一步：列出要改 Fee.java（声明）和另一个模块的 FeeUse.java（调用），盘上一个字还没动：${PREV}"
lite_eval main "[...document.querySelectorAll('.csig button')].find((b) => b.textContent === '改签名').click(); return true" >/dev/null
sig_done() { grep -q 'public long fee(String code, int cents, boolean vip)' "${C}/Fee.java" && grep -q 'new Fee().fee("A", 500, false)' "${FUSE}"; }
wait_for 30 sig_done
ok "$(sig_done && echo 1)" "点「改签名」：声明换了、FeeUse 里的调用跟着换成 fee(\"A\", 500, false)$(sig_done || echo "；此刻 Fee：$(grep -n 'long' "${C}/Fee.java" | tr '\n' ' ')；FeeUse：$(grep -n 'Fee()' "${FUSE}")；胶囊：$(lite_eval main "return document.querySelector('.toast')?.textContent ?? ''")")"
open_file "${FUSE}" >/dev/null
sleep 3
lite_wait main "return document.querySelectorAll('.cm-diag-err').length === 0" 30 >/dev/null
ok "$([ -z "$(errs)" ] && echo 1)" "FeeUse 里 jdtls 不报错（有的话是：$(errs)）"
lite menu undo-replace main >/dev/null
fee_back() { [ "$(cat "${C}/Fee.java")" = "${FEE0}" ] && [ "$(cat "${FUSE}")" = "${FUSE0}" ]; }
wait_for 15 fee_back
ok "$(fee_back && echo 1)" "「编辑 › 撤销上次的替换 / 改名」：两个文件一字不差地回来"

echo "== 引入参数（⌥⌘P，docs/TAKEOVER.md G12）：私有方法 tax 里选中 100 → 多一个参数、本文件的调用传 100、接着改名成 rate；public 的 calc 调用在别的模块：说清做不了、给出路、盘上不动"
sleep 2
open_file "${C}/Fee.java" >/dev/null
lite_wait main "return __lite.text().includes('calc(int cents, String code)')" 10 >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('v * 6 / 100')); const c = t[l].indexOf('100'); __lite.select(l + 1, c + 1, l + 1, c + 4); __lite.key('Alt-Mod-p'); return true" >/dev/null
RENAME rate
lite_wait main "return __lite.text().includes('tax(v, 100)') && __lite.text().includes('/ rate')" 20 >/dev/null
ok "$([ "$(lite_eval main "const t = __lite.text(); return /private long tax\\(long v, int rate\\)/.test(t) && t.includes('v * 6 / rate') && t.includes('tax(v, 100)')")" = true ] && echo 1)" "⌥⌘P：tax(long v, int rate)、方法体里用 rate、total 里的调用传 100；做完改名框直接开着（jdtls 起的是「${N0}」）$([ "$(lite_eval main "return __lite.text().includes('tax(v, 100)')")" = true ] || echo "；此刻：$(lite_eval main "return JSON.stringify(__lite.text().split('\\n').slice(6, 10))")")"
lite_wait main "return !document.querySelector('.rename')" 10 >/dev/null; sleep 1
# 放回原样：存一下（引入参数那一笔在编辑器里没存），再把原文写回盘上、等编辑器跟着重读（改名那一笔是落盘的，⌘Z 撤不干净）
lite menu save main >/dev/null; sleep 1
printf '%s\n' "${FEE0}" > "${C}/Fee.java"
lite_wait main "return __lite.text().includes('v * 6 / 100') && !__lite.text().includes('rate')" 15 >/dev/null
lite_eval main "const t = __lite.text().split('\\n'); const l = t.findIndex((x) => x.includes('* 3 / 100')); const c = t[l].indexOf('100'); __lite.select(l + 1, c + 1, l + 1, c + 4); __lite.key('Alt-Mod-p'); return true" >/dev/null
lite_wait main "return __lite.notify.log.some((x) => x.text.includes('别的文件里也有调用'))" 30 >/dev/null
ok "$([ "$(lite_eval main "return __lite.notify.log.some((x) => x.kind === 'fail' && x.text.includes('⌥⌘V') && x.text.includes('⌘F6'))")" = true ] && [ "$(cat "${FUSE}")" = "${FUSE0}" ] && echo 1)" "public 的 calc（调用在 order-service）：说清这时做不了、给出路（先 ⌥⌘V 再 ⌘F6），FeeUse 一字没动：$(lite_eval main "return __lite.notify.log.at(-1)?.text ?? ''")"

echo "== 不往项目里写东西、退出不留 jdtls"
META=$(cd "${P}" && find . \( -name .project -o -name .classpath -o -name .settings -o -name .factorypath \) | head -5 | tr '\n' ' ')
ok "$([ -z "${META}" ] && echo 1)" "项目里没有 Eclipse 的元数据文件${META:+：${META}}"
JPID=$(pgrep -f "jdt.ls.core.id1.*${LITE_ID}" | head -1)
ok "$([ -n "${JPID}" ] && echo 1)" "jdtls 在跑（pid ${JPID:-无}），缓存在测试应用的数据目录里"
lite_quit
sleep 1
ok "$([ -z "$(pgrep -f "jdt.ls.core.id1.*${LITE_ID}")" ] && echo 1)" "退出之后 jdtls 也没了"
done_
