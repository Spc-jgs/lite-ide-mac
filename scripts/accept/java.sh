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
printf 'package com.demo.common;\n\npublic class Coupon {\n    @Override\n    public String toString() { return "coupon"; }\n}\n' > "${C}/Coupon.java"
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

echo "== 不往项目里写东西、退出不留 jdtls"
META=$(cd "${P}" && find . \( -name .project -o -name .classpath -o -name .settings -o -name .factorypath \) | head -5 | tr '\n' ' ')
ok "$([ -z "${META}" ] && echo 1)" "项目里没有 Eclipse 的元数据文件${META:+：${META}}"
JPID=$(pgrep -f "jdt.ls.core.id1.*${LITE_ID}" | head -1)
ok "$([ -n "${JPID}" ] && echo 1)" "jdtls 在跑（pid ${JPID:-无}），缓存在测试应用的数据目录里"
lite_quit
sleep 1
ok "$([ -z "$(pgrep -f "jdt.ls.core.id1.*${LITE_ID}")" ] && echo 1)" "退出之后 jdtls 也没了"
done_
