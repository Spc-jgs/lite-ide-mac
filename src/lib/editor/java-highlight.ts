/**
 * Java 的语法层补色：注解。
 *
 * # 为什么要有
 *
 * 高亮是两段：语法包给节点打标签（`styleTags`），主题给标签配颜色（`HIGHLIGHT_SPEC`）。
 * `@lezer/java` 从来不产出 `annotation` 标签 —— `@Service` 是 `variableName`，白的；
 * 主题里那条注解色是一条死规则（2026-09-23 拿一份 Spring 代码跑出来的）。
 *
 * # 为什么只有注解在这儿，声明（方法蓝、字段紫）不在
 *
 * 本来打算全用路径规则补（`MethodDeclaration/Definition` 等），实测**一条都不生效**。
 * 是 `@lezer/highlight` 1.2.3 的 bug（上游 main 同样）：每条规则链内部按路径深度
 * 从深到浅排（`Rule.sort`），但两个来源合并时（`ruleNodeProp.combine`）条件写成
 * `a.depth >= b.depth` 取 b —— 实际先取**浅**的。语法包自带一条不看上下文的裸
 * `Definition`（深度 0），合并后排在最前，而 `getStyleTags` 碰到不看上下文的规则
 * 就直接返回，后面带路径的永远轮不到。
 *
 * 注解能生效是因为：`MarkerAnnotation` 原来没有规则，谈不上合并；`Identifier` 那条
 * 两边头一条都是深度 1，打平时取后加的（我们的）。
 *
 * 所以声明挪到了 `java-semantic.ts`（ViewPlugin 画 mark）—— 那本来就是字段使用处、
 * static 斜体要落的地方。代价：差异视图和缩略图走 `highlightTree`，拿不到那一层，
 * 只有注解。
 */
import { styleTags, tags as t } from "@lezer/highlight";

export const javaStyleTags = styleTags({
  // 注解连 `@` 一起上色：`@` 是匿名 token，不在树上，它落在父节点的区间里，
  // 所以要给父节点本身也打标签。
  "MarkerAnnotation Annotation": t.annotation,
  "MarkerAnnotation/Identifier Annotation/Identifier": t.annotation,
  // 带包名的注解（`@org.springframework.Foo`）是嵌套的 ScopedIdentifier。`!` = 不往下走，
  // 整段（连中间的点）按注解画 —— 否则里头每个 Identifier 各自回到 variableName。
  "MarkerAnnotation/ScopedIdentifier! Annotation/ScopedIdentifier!": t.annotation,
});
