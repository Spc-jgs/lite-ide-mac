/**
 * 按需加载组件的一点点脚手架。
 *
 * # 为什么值得抽出来
 *
 * 「重的东西不进入口包」是这个项目的一条红线（ARCHITECTURE.md）：CM6 约 340KB、
 * xterm 约 250KB、Git 那一套约 60KB，只看日志的人一个都用不上。于是每一样都得
 * 写一遍「要用了才 import」。写到第九遍时 App.svelte 里堆了九组几乎一模一样的
 * 十行样板 —— 每组都有自己的 `XxxComp`、`xxxLoading`、一个 `$effect`、
 * 一样的 then/catch/finally。
 *
 * 样板多了不只是啰嗦，是**容易写错而看不出来**：漏掉 `loading` 判重就会在
 * 加载期间重复 import，漏掉 catch 就在加载失败时静默留一片空白。
 *
 * # 用法
 *
 * ```ts
 * const editor = lazy(() => import("./Editor.svelte"), "编辑器");
 * // 模板里：{#if editor.comp}<editor.comp … />{/if}
 * // 需要它时：editor.load()
 * ```
 *
 * `load()` 可以随便调，重复调用会被自身状态挡掉。
 */

type Loader<T> = () => Promise<{ default: T }>;

export interface Lazy<T> {
  /** 加载完成前是 null */
  readonly comp: T | null;
  readonly loading: boolean;
  /** 加载失败时的消息，成功或未开始是空串 */
  readonly error: string;
  /** 幂等：已加载或正在加载时什么都不做 */
  load(): void;
}

export function lazy<T>(loader: Loader<T>, label: string): Lazy<T> {
  let comp = $state<T | null>(null);
  let loading = $state(false);
  let error = $state("");

  return {
    get comp() {
      return comp;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    load() {
      if (comp || loading) return;
      loading = true;
      error = "";
      loader()
        .then((m) => (comp = m.default))
        .catch((e) => (error = `${label}加载失败：${e}`))
        .finally(() => (loading = false));
    },
  };
}

/**
 * 一组一起加载的组件。
 *
 * Git 那几块就属于这种：进了 Git 视图基本都会点开差异、翻历史、切分支，
 * 分五次 import 只是多四次往返。一次拉齐更快，也省掉五份加载状态。
 */
export function lazyGroup<T extends Record<string, unknown>>(
  loaders: { [K in keyof T]: Loader<T[K]> },
  label: string,
): { readonly comps: Partial<T>; readonly loading: boolean; readonly error: string; load(): void } {
  let comps = $state<Partial<T>>({});
  let loading = $state(false);
  let error = $state("");
  let done = false;

  return {
    get comps() {
      return comps;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    load() {
      if (done || loading) return;
      loading = true;
      error = "";
      const keys = Object.keys(loaders) as (keyof T)[];
      Promise.all(keys.map((k) => loaders[k]()))
        .then((mods) => {
          const next: Partial<T> = {};
          keys.forEach((k, i) => (next[k] = mods[i].default as T[keyof T]));
          comps = next;
          done = true;
        })
        .catch((e) => (error = `${label}加载失败：${e}`))
        .finally(() => (loading = false));
    },
  };
}
