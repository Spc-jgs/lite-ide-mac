#!/usr/bin/env bash
# Spring 的两样导航（docs/IDEA.md 第 25 节）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。不要 mvn、不要 jdtls：读文本。
# - 接口：随处搜索的「接口」范围；粘一个日志里的真 URL 找到处理它的方法；写死的路径排在 {id} 前面（同 Spring 自己挑处理者）；target 里的拷贝不算
# - 配置：@Value("${…}") 上 ⌘B → application*.yml / properties；宽松绑定（驼峰对 kebab）、每个 profile 的都列、一处直接跳、一处都没有说一句
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
P=$(cd "$(mktemp -d /tmp/acSn.XXXX)" && pwd -P)
trap 'lite_teardown; rm -rf "${P}"' EXIT

M="${P}/order-service"
mkdir -p "${M}/src/main/java/com/demo/order/web" "${M}/src/main/resources" "${M}/target/generated-sources/com/demo/order/web"
printf '<project><modelVersion>4.0.0</modelVersion><groupId>d</groupId><artifactId>order-service</artifactId><version>1</version></project>\n' > "${M}/pom.xml"
CTL="${M}/src/main/java/com/demo/order/web/OrderController.java"
cat > "${CTL}" <<'J'
package com.demo.order.web;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/orders")
public class OrderController {
    private static final String V2 = "/v2";

    @Value("${demo.greetingText:hi}")
    private String greeting;

    @Value("${server.port}")
    private int port;

    @Value("${demo.nowhere}")
    private String nowhere;

    @GetMapping("/{id}")
    public String get(@PathVariable Long id) { return greeting; }

    @GetMapping("/search")
    public String search(@RequestParam("q") String q) { return q; }

    @RequestMapping(value = {"/export", "/download"}, method = RequestMethod.GET)
    public String export() { return ""; }

    @DeleteMapping(path = V2 + "/{id}")
    public void delete(@PathVariable Long id) {}
}
J
# 构建产物里的一份（注解处理器 / 拷贝生成的）：不该进接口列表
cp "${CTL}" "${M}/target/generated-sources/com/demo/order/web/OrderController.java"
printf 'server:\n  port: 8080\ndemo:\n  greeting-text: hello\n' > "${M}/src/main/resources/application.yml"
printf 'demo:\n  greeting_text: 你好（dev）\n' > "${M}/src/main/resources/application-dev.yml"
printf 'demo.greeting-text=prod 的\n' > "${M}/src/main/resources/application-prod.properties"
# Bean 导航（docs/TAKEOVER.md G30）用的几个类：接口 + 实现、测试里的假实现（不该列）、一个接口两个实现、@Bean 方法、@Mapper 接口，和注入它们的 OrderFacade
J="${M}/src/main/java/com/demo/order"
mkdir -p "${J}/service" "${J}/pay" "${J}/cfg" "${J}/mapper" "${M}/src/test/java/com/demo/order/service"
printf 'package com.demo.order.service;\n\npublic interface OrderService {\n    String find(long id);\n}\n' > "${J}/service/OrderService.java"
printf 'package com.demo.order.service;\n\nimport org.springframework.stereotype.Service;\n\n@Service\npublic class OrderServiceImpl implements OrderService {\n    public String find(long id) { return ""; }\n}\n' > "${J}/service/OrderServiceImpl.java"
printf 'package com.demo.order.service;\n\n@Service\nclass FakeOrderService implements OrderService {\n    public String find(long id) { return "fake"; }\n}\n' > "${M}/src/test/java/com/demo/order/service/FakeOrderService.java"
printf 'package com.demo.order.pay;\n\npublic interface PayGateway {}\n' > "${J}/pay/PayGateway.java"
printf 'package com.demo.order.pay;\n\n@Component\npublic class AliPay implements PayGateway {}\n' > "${J}/pay/AliPay.java"
printf 'package com.demo.order.pay;\n\n@Component\npublic class WxPay implements PayGateway {}\n' > "${J}/pay/WxPay.java"
printf 'package com.demo.order.cfg;\n\n@Configuration\npublic class AppConfig {\n    @Bean\n    public RestTemplate restTemplate() { return new RestTemplate(); }\n}\n' > "${J}/cfg/AppConfig.java"
printf 'package com.demo.order.mapper;\n\n@Mapper\npublic interface OrderMapper {}\n' > "${J}/mapper/OrderMapper.java"
FAC="${J}/OrderFacade.java"
cat > "${FAC}" <<'F'
package com.demo.order;

@Service
@RequiredArgsConstructor
public class OrderFacade {
    private static final String NAME = "facade";
    private final OrderService orders;
    private final PayGateway pay;
    private final OrderMapper mapper;
    private String note;

    @Autowired
    private RestTemplate rest;
}
F
BEFORE=$(cd "${P}" && find . -type f | sort | md5)

lite_clean_data
lite_launch "${P}" || exit 1
ROWS='const rows = () => [...document.querySelectorAll(".palette .results .row")].map((r) => r.innerText.replace(/\s+/g, " ").trim());'

echo "== 接口：随处搜索的「接口」范围列全；target 里那份不算；常量照原名显示"
lite menu quick-file main >/dev/null
lite_wait main "return !!document.querySelector('.palette')" 10 >/dev/null
lite_eval main "[...document.querySelectorAll('.scopes .tab')].find((b) => b.textContent.trim() === '接口').click(); return true" >/dev/null
lite_wait main "${ROWS} return rows().length >= 5" 10 >/dev/null
ok "$([ "$(lite_eval main "${ROWS} return rows().join(' / ')")" = "GET /api/orders/download OrderController.export / GET /api/orders/export OrderController.export / GET /api/orders/search OrderController.search / DELETE /api/orders/{V2}/{id} OrderController.delete / GET /api/orders/{id} OrderController.get" ] && echo 1)" "五个、只有 src 里的：$(lite_eval main "${ROWS} return rows().join(' / ')")"

echo "== 粘一个日志里的真 URL：对上 {id}；写死的 search 排在 {id} 前面"
lite_eval main "__lite.fill('http://localhost:8080/api/orders/42?expand=true', '.palette .q input'); return true" >/dev/null
lite_wait main "${ROWS} return rows()[0]?.startsWith('GET /api/orders/{id}')" 5 >/dev/null
ok "$([ "$(lite_eval main "${ROWS} return rows().map((r) => r.split(' ')[1]).join(' / ')")" = "/api/orders/{id} / /api/orders/{V2}/{id}" ] && echo 1)" "/api/orders/42 → 整个对上的 {id} 在前，开头对上的 {V2}/{id} 在后：$(lite_eval main "${ROWS} return rows().join(' / ')")"
lite_eval main "__lite.fill('/api/orders/search', '.palette .q input'); return true" >/dev/null
lite_wait main "${ROWS} return rows()[0]?.startsWith('GET /api/orders/search')" 5 >/dev/null
ok "$([ "$(lite_eval main "${ROWS} return rows().map((r) => r.split(' ')[1]).join(' / ')")" = "/api/orders/search / /api/orders/{id} / /api/orders/{V2}/{id}" ] && echo 1)" "/api/orders/search：写死的在 {id} 前面（同 Spring 挑处理者的规矩），开头对上的最后：$(lite_eval main "${ROWS} return rows().join(' / ')")"
lite_eval main "__lite.key('Enter'); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${CTL}'" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path + ':' + __lite.caretLine()")" = "${CTL}:24" ] && echo 1)" "回车跳到 search 那一行（第 24 行）"

echo "== 配置：@Value 上 ⌘B"
cfg() { lite_eval main "__lite.caret($1, $2); __lite.key('Meta-b'); return true" >/dev/null; }
# 第 11 行 demo.greetingText：三份配置里写法各不同（kebab、下划线、properties），宽松绑定都算 —— 好几处：列出来挑
cfg 11 18
lite_wait main "return !!document.querySelector('.usages')" 5 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelector('.usages .title')?.textContent + ' | ' + [...document.querySelectorAll('.usages [role=option]')].map((o) => o.innerText.replace(/\s+/g, ' ').trim().split(' ').slice(0, 1).join()).join(',')")" = "demo.greetingText 的配置位置 | application.yml:4,application-dev.yml:2,application-prod.properties:1" ] && echo 1)" "驼峰对得上 kebab / 下划线 / properties，三处都列、不带 profile 的在前：$(lite_eval main "return [...document.querySelectorAll('.usages [role=option]')].map((o) => o.innerText.replace(/\s+/g, ' ').trim()).join(' | ')")"
lite_eval main "__lite.key('Escape'); return true" >/dev/null
# 第 14 行 server.port：只有一处，直接跳
lite_wait main "return !document.querySelector('.usages')" 5 >/dev/null
lite_eval main "await __lite.tabflow.openPath('${CTL}', { preview: false }); return true" >/dev/null
cfg 14 18
lite_wait main "return __lite.tabs.active?.path?.endsWith('application.yml')" 5 >/dev/null
ok "$([ "$(lite_eval main "return (__lite.tabs.active?.path ?? '').split('/').pop() + ':' + __lite.caretLine()")" = "application.yml:2" ] && echo 1)" "只有一处：直接跳到 application.yml 第 2 行（port）"
# 第 17 行 demo.nowhere：没有，说一句，不跳
lite_eval main "await __lite.tabflow.openPath('${CTL}', { preview: false }); return true" >/dev/null
cfg 17 18
lite_wait main "return (document.querySelector('.toast')?.textContent ?? '').includes('demo.nowhere')" 5 >/dev/null
ok "$(lite_eval main "return __lite.tabs.active?.path === '${CTL}' && (document.querySelector('.toast')?.textContent ?? '').includes('未在配置文件中找到 demo.nowhere')" | grep -q true && echo 1)" "没有：留在原地，说「配置文件里没有 demo.nowhere」"

echo "== Bean 导航（docs/TAKEOVER.md G30）：注入点旁边一个图标，点了跳到被注入的 Bean；配上两个的弹列表挑；测试里的假实现不列"
lite_eval main "await __lite.tabflow.openPath('${FAC}', { preview: false }); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${FAC}' && document.querySelectorAll('.cm-sb').length >= 4" 15 >/dev/null
SB=$(lite_eval main "return [...document.querySelectorAll('.cm-sb')].map((m) => __lite.lineOfGutter(m)).join(',')")
ok "$([ "${SB}" = "7,8,9,13" ] && echo 1)" "画在第 7、8、9（Lombok 的 final 字段）和 13 行（@Autowired），static final 和普通字段不画：${SB}"
ok "$(lite_eval main "return document.querySelectorAll('.cm-sb')[0].title" | grep -q '转到注入的 Bean：OrderServiceImpl' && echo 1)" "OrderService 只配上 OrderServiceImpl（src/test 里的 FakeOrderService 不列）：$(lite_eval main "return document.querySelectorAll('.cm-sb')[0].title")"
# 点之前先让 CM6 量一次（lineOfGutter 里做）：窗口在后台时没有帧，行号旁那一列按估的行高摆，按屏幕坐标点会点到别的行
CLICK='const click = (i) => { const m = document.querySelectorAll(".cm-sb")[i]; __lite.lineOfGutter(m); const r = m.getBoundingClientRect(); m.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, clientX: r.left + 3, clientY: r.top + 3, button: 0 })); };'
lite_eval main "${CLICK} click(0); return true" >/dev/null
lite_wait main "return (__lite.tabs.active?.path ?? '').endsWith('/OrderServiceImpl.java')" 5 >/dev/null
ok "$(lite_eval main "return (__lite.tabs.active?.path ?? '').endsWith('/service/OrderServiceImpl.java')" | grep -q true && echo 1)" "点 OrderService 那个：直接跳到 OrderServiceImpl.java"
lite_eval main "await __lite.tabflow.openPath('${FAC}', { preview: false }); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${FAC}' && document.querySelectorAll('.cm-sb').length >= 4" 10 >/dev/null
lite_eval main "${CLICK} click(1); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.usages [role=option]').length > 0" 5 >/dev/null
PG=$(lite_eval main "return [...document.querySelectorAll('.usages [role=option]')].map((o) => o.innerText.replace(/\\s+/g, ' ').trim().split(/[ :]/)[0]).sort().join(',')")
ok "$([ "${PG}" = "AliPay,WxPay" ] && echo 1)" "PayGateway 两个实现：弹列表挑：${PG}"
lite_eval main "document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true" >/dev/null
ok "$(lite_eval main "return document.querySelectorAll('.cm-sb')[3].title" | grep -q 'AppConfig.restTemplate()' && echo 1)" "@Autowired 的 RestTemplate 配上 @Bean 方法 AppConfig.restTemplate()"
ok "$(lite_eval main "return document.querySelectorAll('.cm-sb')[2].title" | grep -q 'OrderMapper' && echo 1)" "OrderMapper 配上 @Mapper 接口"

echo "== 没有副作用"
ok "$([ "$(cd "${P}" && find . -type f | sort | md5)" = "${BEFORE}" ] && echo 1)" "项目里一个文件都没多（不建索引文件）"

lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
