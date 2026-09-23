/*
 * ⌘Click 跳转的零层：Java 单文件符号表（局部变量、字段、`q.成员`）。
 *
 * 和 jump.test.ts 同一个原则：测的是「**该不该给下划线**」，负例和正例一样重要。
 * 这里多一类负例 —— 「这层说推不出，就不许往下落到按名字找的那几层」。
 */
import { EditorState } from "@codemirror/state";
import { javaLanguage } from "@codemirror/lang-java";
import { syntaxTree } from "@codemirror/language";
import { resolveJump, type JumpCtx, type JavaTools } from "../src/lib/editor/jump.ts";
import { outlineOf } from "../src/lib/editor/outline.ts";
import { javaRefAt, javaMembersOf, pickMember, membersFromSource } from "../src/lib/editor/java-scope.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

// 别的文件：相对项目根的路径 → 源码
const DISK: Record<string, string> = {
  "core/src/main/java/com/demo/repo/OrderRepository.java": `package com.demo.repo;

public interface OrderRepository extends CrudRepository<Order, Long> {
    Order findByNo(String no);

    List<Order> findByUser(long uid);
    List<Order> findByUser(long uid, int page);

    void touch(Order a);
    void touch(String no);

    void log(String fmt, Object... args);
}
`,
  "core/src/main/java/com/demo/model/Order.java": `package com.demo.model;

public class Order {
    public long id;
    private Customer customer;
    public Customer getCustomer() { return customer; }
}
`,
  "core/src/main/java/com/demo/util/OrderUtil.java": `package com.demo.util;

public final class OrderUtil {
    public static String format(Order o) { return ""; }
}
`,
  "api/src/main/java/com/demo/api/SamePkg.java": `package com.demo.api;

public class SamePkg {
    public void ping() {}
}
`,
};

const SRC = `package com.demo.api;

import com.demo.repo.OrderRepository;
import com.demo.model.Order;
import com.demo.util.OrderUtil;

public class OrderController {
    private OrderRepository repo;
    private SamePkg same;
    private int count;

    public Order show(String no) {
        Order order = repo.findByNo(no);
        repo.findByUser(1L);
        repo.findByUser(1L, 2);
        repo.touch(order);
        repo.findById(1L);
        repo.log("a", 1, 2, 3);
        same.ping();
        use(order.id);
        OrderUtil.format(order);
        new Order().getCustomer();
        log.info("x");
        log.helper();
        order.getItems();
        this.count++;
        use(count);
        helper();
        return order;
    }

    public void shadow() {
        String repo = "";
        repo.length();
    }

    public void getItems() {}
    private void helper() {}

    static class Inner { void hi() {} }
    void inner() { new Inner().hi(); }
}
`;

const REL = "api/src/main/java/com/demo/api/OrderController.java";
const FILES = [REL, ...Object.keys(DISK)];

// 缓存里的成员表：模拟 java-peek.ts 已经读过这些文件。`pending` 里的当成「还在读」
const pending = new Set<string>();
const rechecks: string[] = [];
const tools: JavaTools = {
  refAt: (st, pos) => javaRefAt(syntaxTree(st), (a, z) => st.sliceDoc(a, z), pos),
  membersHere: (st, cls) => javaMembersOf(syntaxTree(st), (a, z) => st.sliceDoc(a, z), cls),
  peek: (rel, onReady) => {
    if (pending.has(rel)) {
      if (onReady) rechecks.push(rel);
      return undefined;
    }
    const src = DISK[rel];
    return src ? membersFromSource(src, rel.slice(rel.lastIndexOf("/") + 1, -5)) : null;
  },
  pick: pickMember,
  settle: () => null,
};

const state = EditorState.create({ doc: SRC, extensions: [javaLanguage, javaLanguage.data.of({ javaTools: tools })] });
const ctx: JumpCtx = { symbols: outlineOf(state), files: FILES, rel: REL, lang: "java", recheck: () => {} };

/** 第 n 次出现的 word（整词）的中间 */
function at(word: string, n = 1): number {
  const re = new RegExp(`(?<![\\w$])${word}(?![\\w$])`, "g");
  let m: RegExpExecArray | null = null;
  for (let k = 0; k < n; k++) m = re.exec(SRC);
  if (!m) throw new Error(`没有第 ${n} 个 ${word}`);
  return m.index + 1;
}
const jump = (word: string, n = 1) => resolveJump(state, at(word, n), ctx);
const lineOf = (src: string, needle: string) => src.slice(0, src.indexOf(needle)).split("\n").length;
const REPO = "core/src/main/java/com/demo/repo/OrderRepository.java";
const ORDER = "core/src/main/java/com/demo/model/Order.java";

// ── 正例：接收者的类型写得出来，成员在那个类里、唯一 ──
{
  const h = jump("findByNo");
  ok(h?.target.rel === REPO && h.target.line === lineOf(DISK[REPO], "findByNo"), `字段接收者 → 接口里的方法：${JSON.stringify(h?.target)}`);
  ok(h?.target.why === "成员", "依据写的是「成员」");

  const one = jump("findByUser", 1);
  const two = jump("findByUser", 2);
  ok(one?.target.line === lineOf(DISK[REPO], "findByUser(long uid)"), `一个实参挑一个参数的重载：${one?.target.line}`);
  ok(two?.target.line === lineOf(DISK[REPO], "findByUser(long uid, int page)"), `两个实参挑两个参数的：${two?.target.line}`);

  const va = jump("log", 1);
  ok(va?.target.line === lineOf(DISK[REPO], "void log("), `可变参数吃掉多出来的实参：${JSON.stringify(va?.target)}`);

  const sp = jump("ping");
  ok(sp?.target.rel === "api/src/main/java/com/demo/api/SamePkg.java", `同包的类（不写 import）：${sp?.target.rel}`);

  const fld = jump("id");
  ok(fld?.target.rel === ORDER && fld.target.line === lineOf(DISK[ORDER], "long id"), `局部变量.字段 → 那个类的字段：${JSON.stringify(fld?.target)}`);

  const st = jump("format");
  ok(st?.target.rel === "core/src/main/java/com/demo/util/OrderUtil.java", `类型名.static 方法：${st?.target.rel}`);

  const nw = jump("getCustomer");
  ok(nw?.target.rel === ORDER, `new Foo().方法：${nw?.target.rel}`);

  const inner = jump("hi", 2);
  ok(inner?.target.rel === "" && inner.target.line === lineOf(SRC, "void hi()"), `本文件的内部类：${JSON.stringify(inner?.target)}`);
}

// ── 负例：这层管了，答案是不画 ──
{
  ok(jump("touch") === null, "同个数的重载靠类型区分 → 不猜");
  ok(jump("findById") === null, "继承来的方法（CrudRepository 在 jar 里）→ 不画");
  ok(jump("info") === null, "接收者是认不出的 log（Lombok 生成）→ 不画");
  // 本文件恰好有个 helper()：接收者推不出类型时，按名字那层会命中它 —— 零层要拦的第一种
  ok(jump("helper", 1) === null, "接收者推不出类型 → 不落到按名字找（本文件的同名方法）");
  // 本文件恰好有个 getItems()：按名字那层会命中它 —— 零层要拦的第二种
  ok(jump("getItems", 1) === null, "接收者推得出类型、但那个类里没这个成员 → 不落到按名字找");
  ok(jump("length") === null, "局部 String 遮住字段 repo：String 不在项目里 → 不画（没有落回字段类型）");
}

// ── 本文件的变量：跳到声明 ──
{
  const c = jump("count", 3);
  ok(c?.target.rel === "" && c.target.line === lineOf(SRC, "private int count"), `裸名字是字段 → 字段声明：${JSON.stringify(c?.target)}`);
  const tc = jump("count", 2);
  ok(tc?.target.line === lineOf(SRC, "private int count"), `this.字段 → 字段声明：${tc?.target.line}`);
  const o = jump("order", 3);
  ok(o?.target.line === lineOf(SRC, "Order order = "), `局部变量 → 它的声明行：${o?.target.line}`);
  const sh = jump("repo", 10); // 第 1 个是 import 里包名那一段
  ok(sh?.target.line === lineOf(SRC, 'String repo = ""'), `同名局部遮住字段 → 跳局部，不跳字段：${sh?.target.line}`);
  // 没限定的方法调用不归零层管，照旧按名字那层（本文件的声明）
  const hp = jump("helper", 2);
  ok(hp?.target.rel === "" && hp.target.line === lineOf(SRC, "private void helper"), `没限定的方法调用照旧：${JSON.stringify(hp?.target)}`);
  // 类型名照旧走 import
  const ty = jump("OrderUtil", 2);
  ok(ty?.target.rel === "core/src/main/java/com/demo/util/OrderUtil.java" && ty.target.why === "import", `类型名照旧走 import：${JSON.stringify(ty?.target)}`);
}

// ── 目标文件还没读：先不画，读回来叫 recheck ──
{
  pending.add(REPO);
  const h = resolveJump(state, at("findByNo"), { ...ctx, recheck: () => {} });
  ok(h === null, "还在读 → 这一下不画");
  ok(rechecks.includes(REPO), "登记了读完之后的回调");
  pending.delete(REPO);
  ok(resolveJump(state, at("findByNo"), ctx)?.target.rel === REPO, "读回来之后再问 → 画");
}

console.log(`Java 成员跳转：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
