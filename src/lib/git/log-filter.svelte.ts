/**
 * 提交历史只看一个文件 / 几行（docs/TAKEOVER.md G16，IDEA 的 Show History / Show History for Selection）：菜单那边放进来，`GitLog.svelte` 读。
 * 放在一个单独的模块里：菜单动作和提交历史都是懒块，这里不进入口包。`null` = 看整个仓库（或者「只看当前文件」那个勾）
 */
export const logFilter = $state<{ v: { path: string; lines: [number, number] | null } | null }>({ v: null });
