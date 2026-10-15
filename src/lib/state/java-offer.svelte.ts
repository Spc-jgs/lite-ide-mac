/**
 * 「要下载 jdtls 吗」那张卡片的状态（docs/IDEA.md 第 10 节）。Rust 说 `missing` 时 java-lsp.ts 放进来，Confirms.svelte 画成卡片。
 * 单独一个模块：Confirms 和 java-lsp 都是懒的，各自引它，它就落在它俩共用的块里，不进入口包。
 */
export const javaOffer = $state<{ card: { root: string; text: string } | null }>({ card: null });

/** 这次运行里点过「以后再说」的项目：不再问（重启应用再问一次 —— 记一辈子的话，人忘了自己点过，Java 智能就永远不来了） */
export const declined = new Set<string>();
