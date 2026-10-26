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
  import { sendToTerminal } from "../terminal/send";
  // 菜单上印的快捷键从键位表取（唯一出处）。这个组件是懒加载的，引它不进入口包（frontend.md「一个 import { byId }」那条）
  import { byId } from "../state/keymap";

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
      { label: "跳到声明", keys: byId("jump-decl")?.accel, disabled: !api.hit, run: () => api.hit && void nav.jumpTo(api.hit) },
      { label: "在项目里找这个名字", disabled: !project.root, run: () => onMenuAction("find-word") },
      // 发的是**右键的这个文件**（分屏时它不一定是活动的那组），所以直接调，不经 onMenuAction
      { label: "发送到终端", keys: byId("send-to-terminal")?.accel, run: () => void sendToTerminal(path) },
      // 选中的那段（没选就是光标那行）带着位置挂到 AI 助手的输入框上（docs/TAKEOVER.md MVP-6）
      { label: "发给 AI 助手", disabled: !inProject, run: () => void import("../agent/agent.svelte").then((m) => m.attachSelection(path)) },
      // 剪切 / 复制 / 粘贴走系统的「编辑」菜单（predefined，不在键位表里），键位是 macOS 定死的
      { label: "剪切", keys: "⌘X", sep: true, disabled: !api.hasSelection, run: () => void api.cut() },
      { label: "复制", keys: "⌘C", disabled: !api.hasSelection, run: () => void api.copy() },
      {
        label: "粘贴",
        keys: "⌘V",
        // 读不到（超 8 MB、pbpaste 失败）就把原因说出来，不静默失败 —— ⌘V 那条路不受影响
        // 做完焦点回编辑器靠 ContextMenu 的 giveBack（右键编辑器时焦点就在编辑器上）
        run: () =>
          void api
            .paste()
            .then((pasted) => {
              if (!pasted) notify.hint("剪贴板里没有文字");
            })
            .catch((e) => notify.fail(`粘贴失败：${String(e)}`, 3600)),
      },
      { label: "复制路径", sep: true, run: () => void copyText(path, "路径") },
      { label: "复制相对路径", disabled: !inProject, run: () => void copyText(relTo(project.root!, path), "相对路径") },
      { label: "在文件树中定位", disabled: !inProject, run: () => onReveal(path) },
      { label: "在 Finder 中显示", run: () => void showInFinder(path) },
      // 本地历史（docs/IDEA.md 第 21 节）：IDEA 也是右键 → Local History → Show History
      { label: "本地历史…", sep: true, run: () => void import("../history/history-ops").then((m) => m.openHistory(path)) },
    ];
    if (git.repo && inProject) {
      out.push(
        { label: "查看这个文件的改动", sep: true, run: () => onMenuAction("git-file-diff") },
        { label: "显示 / 隐藏注解（blame）", run: () => onMenuAction("git-blame") },
        // IDEA 右键 › Git › Show History / Show History for Selection（docs/TAKEOVER.md G16）
        { label: "这个文件的历史", run: () => onMenuAction("git-file-history") },
        { label: api.hasSelection ? "选中几行的历史" : "这一行的历史", run: () => onMenuAction("git-selection-history") },
      );
    }
    return out;
  });

  function close(refocus: boolean) {
    onclose();
    // 键盘 / Esc 关的：焦点还给编辑器。鼠标点了某一项的，由 ContextMenu 还给右键之前的焦点
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
