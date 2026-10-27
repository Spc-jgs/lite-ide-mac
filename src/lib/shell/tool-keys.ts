/**
 * 工具窗的编号键（⌘4 运行、⌘5 调试、⌘6 问题、⌘7 结构、⌘8 Services、⌘9 Git、⌥F12 终端，照 IDEA；docs/TAKEOVER.md G7）。
 *
 * 懒的：菜单那边（menu-actions.ts，在入口包里）只有一行把 `tw-*` 交过来。
 * 指名道姓地要一个工具窗、它此刻又没有内容可显示时，说一句为什么 —— 不开一个空壳（ui.md 第十条「工具窗有内容才出现」）
 */
import { layout } from "../state/layout.svelte";
import { runs } from "../state/runs.svelte";
import { git } from "../state/git.svelte";
import { project } from "../state/project.svelte";
import { notify } from "../state/notify.svelte";

export function toggleToolKey(id: string) {
  switch (id) {
    case "tw-terminal":
      return layout.toggleTool("term");
    case "tw-problems":
      return layout.toggleTool("problems");
    case "tw-notifications":
      return layout.toggleTool("notifications");
    case "tw-coverage":
      return layout.toggleTool("coverage");
    case "tw-bookmarks":
      return layout.toggleTool("bookmarks");
    case "tw-todo":
      if (!project.root) return void notify.hint("请先打开项目：TODO 工具窗列出项目注释中的 TODO / FIXME");
      return layout.toggleTool("todo");
    case "tw-structure":
      // 侧边栏里的（IDEA 也在左边）：开着就收起，否则切过去
      if (!project.root) return void notify.hint("请先打开项目：结构工具窗列出当前文件的类、方法和字段");
      if (layout.sidebar && layout.sideView === "structure") layout.sidebar = false;
      else layout.showSide("structure");
      return;
    case "tw-git":
      if (!git.repo) return void notify.hint("此项目不是 Git 仓库");
      return layout.toggleTool("git");
    case "tw-services":
      if (!project.root) return void notify.hint("请先打开项目：Services 工具窗列出项目中的 Spring Boot 服务");
      return layout.toggleTool("services");
    case "tw-run":
      if (!runs.plain().length) return void notify.hint("尚未运行任务（按 ⌃⌥R 选择任务）");
      return layout.toggleTool("run");
    case "tw-debug": {
      // 调试没有单独的工具窗：调试着起的那一次就是一格运行（服务在 Services 里），里面「调试器 / 控制台」两页（docs/IDEA.md 第 18 节）
      const r = [...runs.list].reverse().find((x) => x.debug && (x.status === "running" || x.status === "stopping"));
      if (!r) return void notify.hint("没有正在调试的程序（⌃D 调试上一个任务，⌃⇧D 调试光标处的测试）");
      if (r.service) {
        layout.toggleTool("services");
        if (layout.panel) void import("../state/runs-ops").then((m) => m.focusService(r.id));
      } else {
        runs.activeId = r.id;
        layout.toggleTool("run");
      }
      return;
    }
  }
}
