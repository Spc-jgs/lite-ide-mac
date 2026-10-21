#!/usr/bin/env bash
# MyBatis 的 Mapper 接口 ↔ XML 跳转（docs/IDEA.md 第 24 节）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。不要 mvn、不要 jdtls：两边都是读文本，打开就有 —— 这正是要验的一条。
# 验的是只有真实现才有的：Rust 的解析（注释、注解括号、default 方法里的匿名类）、在项目里找 XML（跳过 target 里 Maven 拷过去的那份）、找接口（main 优先于 test）、没保存的改动也算
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
P=$(cd "$(mktemp -d /tmp/acMb.XXXX)" && pwd -P)
trap 'lite_teardown; rm -rf "${P}"' EXIT

M="${P}/order-service"
mkdir -p "${M}/src/main/java/com/demo/order/mapper" "${M}/src/main/resources/mapper" "${M}/src/test/java/com/demo/order/mapper" "${M}/target/classes/mapper"
printf '<project><modelVersion>4.0.0</modelVersion><groupId>d</groupId><artifactId>order-service</artifactId><version>1</version></project>\n' > "${M}/pom.xml"
JAVA="${M}/src/main/java/com/demo/order/mapper/OrderMapper.java"
cat > "${JAVA}" <<'J'
package com.demo.order.mapper;

import org.apache.ibatis.annotations.*;

/** 订单：findById(Long) 写在注释里不算 */
@Mapper
public interface OrderMapper {
    String SQL = "select count(*) from t where id in (select 1)";

    Order findById(@Param("id") Long id);

    @Update("UPDATE t_order SET status = #{s} WHERE id = (#{id})")
    int updateStatus(@Param("id") Long id,
                     @Param("s") String s);

    long count();

    default int deleteById(Long id) {
        Runnable r = new Runnable() {
            public void run() { System.out.println(id); }
        };
        return 0;
    }
}
J
XML="${M}/src/main/resources/mapper/OrderMapper.xml"
cat > "${XML}" <<'X'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE mapper PUBLIC "-//mybatis.org//DTD Mapper 3.0//EN" "http://mybatis.org/dtd/mybatis-3-mapper.dtd">
<mapper namespace="com.demo.order.mapper.OrderMapper">
    <!-- <select id="count">SELECT 1</select> 注释掉的不算 -->
    <select id="findById" resultType="Order">
        SELECT * FROM t_order WHERE id = #{id}
    </select>
    <update
        id="updateStatus">UPDATE t_order SET status = #{s} WHERE id = #{id}</update>
    <delete id="deleteById">DELETE FROM t_order WHERE id = #{id}</delete>
</mapper>
X
# Maven 打包会把资源拷进 target/classes：跳转要跳到 src 里那份，不能跳进构建产物
cp "${XML}" "${M}/target/classes/mapper/OrderMapper.xml"
# 测试目录里同名的接口：从 XML 跳回去要去 main 的那份
printf 'package com.demo.order.mapper;\npublic interface OrderMapper { Order findById(Long id); }\n' > "${M}/src/test/java/com/demo/order/mapper/OrderMapper.java"
# 另一个 namespace 的 XML：不该配上
printf '<mapper namespace="com.demo.user.UserMapper"><select id="findById">SELECT 1</select></mapper>\n' > "${M}/src/main/resources/mapper/UserMapper.xml"
BEFORE=$(cd "${P}" && find . -type f | sort | md5)

lite_clean_data
lite_launch "${P}" || exit 1
MK='const mk = () => [...document.querySelectorAll(".cm-mb")].map((m) => m.className.replace("cm-mb ", "") + " " + m.title);'

echo "== 接口：XML 里有的方法旁边一个图标（count 没有、注释里的不算、default 里匿名类的 run 不算）；打开就有，不等 jdtls"
lite_eval main "await __lite.tabflow.openPath('${JAVA}', { preview: false }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-mb').length >= 3" 10 >/dev/null
ok "$([ "$(lite_eval main "${MK} return mk().join(' / ')")" = 'to-xml 跳到 XML：<select id="findById">（OrderMapper.xml:5） / to-xml 跳到 XML：<update id="updateStatus">（OrderMapper.xml:8） / to-xml 跳到 XML：<delete id="deleteById">（OrderMapper.xml:10）' ] && echo 1)" "三个图标、指向的语句和行号对：$(lite_eval main "${MK} return mk().join(' / ')")"
ok "$([ "$(lite_eval main "return [...document.querySelectorAll('.cm-mb')].map((m) => m.closest('.cm-gutterElement') ? __lite.lineOfGutter(m) : 0).join(',')")" = "10,13,18" ] && echo 1)" "画在接口的第 10、13、18 行（findById、跨两行的 updateStatus、default 的 deleteById）"

echo "== 点一下：跳进 src 里的 XML（不是 target 里 Maven 拷过去的那份），停在那段语句"
lite_eval main "const m = document.querySelectorAll('.cm-mb')[0]; const r = m.getBoundingClientRect(); m.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, clientX: r.left + 3, clientY: r.top + 3, button: 0 })); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${XML}'" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path + ':' + __lite.caretLine()")" = "${XML}:5" ] && echo 1)" "到了 ${XML#"${P}"/}:5（<select id=\"findById\">）"

echo "== XML：每段语句旁边一个往回跳的；点了回到 main 里的接口（不是 test 里同名的）"
lite_wait main "return document.querySelectorAll('.cm-mb.to-java').length === 3" 10 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-mb.to-java').length")" = 3 ] && echo 1)" "三段语句各一个"
lite_eval main "const m = document.querySelectorAll('.cm-mb')[1]; const r = m.getBoundingClientRect(); m.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, clientX: r.left + 3, clientY: r.top + 3, button: 0 })); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${JAVA}'" 10 >/dev/null
ok "$([ "$(lite_eval main "return __lite.tabs.active?.path + ':' + __lite.caretLine()")" = "${JAVA}:13" ] && echo 1)" "回到 main 里的接口第 13 行（updateStatus）：$(lite_eval main "return (__lite.tabs.active?.path ?? '').replace('${P}/', '') + ':' + __lite.caretLine()")"

echo "== 没保存的改动也算：XML 里加一段 count（接口里有 count），停一秒，这段旁边也有了往回跳的图标"
lite_eval main "await __lite.tabflow.openPath('${XML}', { preview: false }); return true" >/dev/null
lite_wait main "return __lite.tabs.active?.path === '${XML}' && document.querySelectorAll('.cm-mb').length === 3" 10 >/dev/null
lite_eval main "__lite.caret(10, 1); __lite.type('    <select id=\"count\">SELECT count(*) FROM t_order</select>\\n'); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-mb').length === 4" 10 >/dev/null
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-mb').length + ':' + __lite.tabs.active?.dirty")" = "4:true" ] && echo 1)" "XML 这边多了一个（还没保存）：$(lite_eval main "return document.querySelectorAll('.cm-mb').length + ':' + __lite.tabs.active?.dirty")"

echo "== 没有副作用"
ok "$([ "$(cd "${P}" && find . -type f | sort | md5)" = "${BEFORE}" ] && echo 1)" "项目里一个文件都没多（不建索引文件）"

lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
