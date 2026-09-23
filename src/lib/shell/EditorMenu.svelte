<script lang="ts">
  /**
   * 编辑器的右键菜单（2026-09-23 交互习惯那轮）。
   *
   * 原来右键出来的是 WebView 自带的那个（查询、翻译、服务…）；IDEA 里右键就能跳转、看注解、
   * 复制路径 —— 手是这么长的。剪切 / 复制 / 粘贴接管之后得自己给：要碰编辑器内部的选区，
   * 由编辑器交出来（`EditorMenuApi`）。其余的项和菜单栏 / 标签右键是同一个动作。
   *
   * 懒加载（Content.svelte）：右键才出现的东西不进入口包。
   */
  import { cmenu } from "./context-menu.svelte";
  import type { MenuItem } from "./ContextMenu.svelte";
  import type { EditorMenuApi } from "../editor/menu-api";
  import { copyText, showInFinder, relTo } from "./pathactions";
  import { project } from "../state/project.svelte";
  import { git } from "../state/git.svelte";
  import { nav } from "../state/nav.svelte";
  import { notify } from "../state/notify.svelte";

  let {
    x,
    y,
    path,
    api,
    onMenuAction,
    onReveal,
    onclose,
  }: {
    x: number;
    y: number;
    path: string;
    api: EditorMenuApi;
    onMenuAction: (id: string) => void;
    onReveal: (path: string) => void;
    onclose: () => void;
  } = $props();

  cmenu.load();

  let items = $derived.by((): MenuItem[] => {
    const inProject = !!project.root && path.startsWith(`${project.root}/`);
    const out: MenuItem[] = [
      { label: "跳到声明", disabled: !api.hit, run: () => api.hit && void nav.jumpTo(api.hit) },
      { label: "在项目里找这个名字", disabled: !project.root, run: () => onMenuAction("find-word") },
      { label: "剪切", sep: true, disabled: !api.hasSelection, run: () => void api.cut() },
      { label: "复制", disabled: !api.hasSelection, run: () => void api.copy() },
      {
        label: "粘贴",
        // 读剪贴板在 WKWebView 里可能被拦：读不到就说一声，不静默失败
        run: () => void api.paste().catch(() => notify.ok("这里读不到剪贴板，用 ⌘V 粘贴", 2600)),
      },
      { label: "复制路径", sep: true, run: () => void copyText(path, "路径") },
      { label: "复制相对路径", disabled: !inProject, run: () => void copyText(relTo(project.root!, path), "相对路径") },
      { label: "在文件树中定位", disabled: !inProject, run: () => onReveal(path) },
      { label: "在 Finder 中显示", run: () => void showInFinder(path) },
    ];
    if (git.repo && inProject) {
      out.push(
        { label: "查看这个文件的改动", sep: true, run: () => onMenuAction("git-file-diff") },
        { label: "显示 / 隐藏注解（blame）", run: () => onMenuAction("git-blame") },
      );
    }
    return out;
  });

  function close(refocus: boolean) {
    onclose();
    // 键盘 / Esc 关的：焦点还给编辑器（鼠标点了某一项的，那一项自己决定焦点去哪）
    if (refocus) api.focus();
  }
</script>

{#if cmenu.comp}
  <cmenu.comp
    {x}
    {y}
    title={path.slice(path.lastIndexOf("/") + 1)}
    titleTip={path}
    label="编辑器的操作"
    {items}
    onclose={close}
  />
{/if}
