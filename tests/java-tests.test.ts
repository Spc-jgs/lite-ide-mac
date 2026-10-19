/**
 * 跑单测里不碰编辑器的那一半（src/lib/editor/java-tests.ts）：认出测试类和方法、模块、任务名、重跑失败的选择器。
 */
import { failedSelector, moduleOf, parseTestTask, testTargets, testTaskName } from "../src/lib/editor/java-tests.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const SRC = `package com.demo.order;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;

/** 说明里提到 @Test 不算 */
class OrderTest {
    @Test
    void adds() {
        assertEquals(2, 1 + 1);
    }

    @Test @DisplayName("合计")
    public void totals() { }

    @ParameterizedTest
    @ValueSource(ints = {1, 2})
    void many(int x) { }

    // @Test
    void notATest() { }

    String helper() { return "@Test"; }

    @org.junit.jupiter.api.Test
    void qualified() { }
}
`;
eq(testTargets(SRC), [
  { line: 7, cls: "com.demo.order.OrderTest", method: null },
  { line: 9, cls: "com.demo.order.OrderTest", method: "adds" },
  { line: 14, cls: "com.demo.order.OrderTest", method: "totals" },
  { line: 18, cls: "com.demo.order.OrderTest", method: "many" },
  { line: 26, cls: "com.demo.order.OrderTest", method: "qualified" },
], "类 + 四个测试方法：注解和方法名不同行、同一行、带参数的注解、全名的注解；注释和字符串里的 @Test 不算");
eq(testTargets("class Util {\n  void a() {}\n}\n"), [], "没有测试方法的类不给 ▶");
eq(testTargets("class T {\n  @Test void x() {}\n}\n"), [{ line: 1, cls: "T", method: null }, { line: 2, cls: "T", method: "x" }], "没有 package 的类名不带点");

eq(moduleOf("/p", "/p/order/src/test/java/com/demo/OrderTest.java"), "order", "模块是 src/test/java 前面那段");
eq(moduleOf("/p", "/p/backend/order/src/test/java/A.java"), "backend/order", "嵌套的模块");
eq(moduleOf("/p", "/p/src/test/java/A.java"), ".", "根模块是 .");
eq(moduleOf("/p", "/p/order/src/main/java/A.java"), null, "不是测试源码");
eq(moduleOf("/p", "/p/xsrc/test/java/A.java"), null, "目录名只是碰巧以 src 结尾的不算");
eq(moduleOf("/p", "/other/src/test/java/A.java"), null, "不在项目里");

eq(testTaskName("order", { cls: "com.demo.OrderTest", method: "adds" }), "order [test com.demo.OrderTest#adds]", "任务名");
eq(testTaskName(".", { cls: "T", method: null }), ". [test T]", "整个类");
eq(parseTestTask("order [test com.demo.OrderTest#adds]"), { module: "order", selector: "com.demo.OrderTest#adds" }, "拆回来");
eq(parseTestTask("order-service [package]"), null, "跑目标的不是测试");

eq(failedSelector([
  { class: "A", name: "x", outcome: "failed" },
  { class: "A", name: "ok", outcome: "passed" },
  { class: "A", name: "y", outcome: "error" },
  { class: "B", name: "many(int)[1]", outcome: "failed" },
  { class: "B", name: "many(int)[2]", outcome: "failed" },
  { class: "B", name: "skipped", outcome: "skipped" },
]), "A#x+y,B#many", "同类的方法用 +、不同类用 ,；参数化的去掉括号、只出现一次；通过和跳过的不算");
eq(failedSelector([{ class: "A", name: "x", outcome: "passed" }]), null, "没有失败的");

console.log(`${fail === 0 ? "✅" : "❌"} 跑单测的识别：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
