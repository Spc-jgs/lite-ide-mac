/**
 * 菜单项按下去做什么。
 *
 * 从 App.svelte 搬出来（入口包瘦身，2026-09-21）。判据还是那一条 —— **入口包是首屏之前
 * 必须解析执行完的那一段**：菜单在窗口出现之前一次都点不到，而它的处理是一张 70 个
 * case 的表，加上「帮助」那四个动作，合起来 4 KB 出头。它走的路本来就是异步的
 * （AppKit 吃掉快捷键 → 发 `menu` 事件 → 前端收到），所以晚一拍拉 chunk 察觉不到；
 * App 那边首屏后 300ms 预拉，第一次按菜单时多半已经在了。
 *
 * id 与 `keymap.ts`、`menu.rs` 三处同一套 —— 那两处由 `tests/menu_sync.rs` 卡着，
 * 这里是第三处，漏一个 case 的表现是「点了没反应」，所以末尾留了一条 diag。
 *
 * 几个只属于 App 组件的状态（缩略图开关、字号、分支挂件的锚点元素）不搬 ——
 * 它们是 `$state`，搬进普通模块就得改成另一套；通过 `MenuCtx` 回调过去。
 */
import { probePath, diag } from "../ipc/commands";
import { installCli, appLogPath, clearAppLog, openExternal } from "../ipc/app";
import { notify } from "../state/notify.svelte";
import { layout } from "../state/layout.svelte";
import { tabs } from "../state/tabs.svelte";
import { tabflow } from "../state/tabflow.svelte";
import { project } from "../state/project.svelte";
import { worktree } from "../state/worktree.svelte";
import { git } from "../state/git.svelte";
import { remote } from "../state/remote.svelte";
import { nav } from "../state/nav.svelte";
import { overlay } from "../state/overlay.svelte";
import { terms } from "../state/terms.svelte";
import { docs } from "../state/docs.svelte";
import { wrapsByDefault } from "../state/tab";

/** App 组件里才有的那几样，菜单动作要碰它们时回调回去 */
export interface MenuCtx {
  toggleMinimap(): void;
  /** `delta` 为 null = 复位到默认字号 */
  zoom(delta: number | null): void;
  openBranchPicker(): void;
}

/**
 * 「安装命令行工具…」（issue #40）。装上了说一句；软链没装上（/usr/local/bin 要 sudo）
 * 就把那一句命令摆在横幅里 —— 它不会自动消失，人要把它抄进终端。
 */
async function installCliTool() {
  notify.clear();
  try {
    const r = await installCli();
    if (r.linked) {
      notify.ok(`${r.replaced ? "已重新安装" : "已安装"} lite 命令 —— 终端里 lite <路径> 就能开`, 5000);
    } else {
      notify.block(
        "脚本已写好，但 /usr/local/bin 写不进去 —— 在终端里跑这一句补上软链：",
        r.linkCmd,
      );
    }
  } catch (e) {
    notify.fail(String(e));
  }
}

/**
 * 打开应用自己的运行日志。
 *
 * 走的是普通的 `openPath` —— 那个文件多半会被判成日志模式（体积/行数），
 * 于是级别过滤、tail、跳到下一处错误全都现成。**这就是这个功能的全部实现**：
 * 一个日志查看器不需要另外做一个「日志窗口」。
 *
 * 正常情况下这个文件**一定在** —— 启动时 `applog::install` 就把它建出来了，
 * 并且写了一行「启动 vX.Y.Z」。所以探不到它意味着日志根本没装上
 * （目录建不了、权限不对），那条消息要这么说，不能只说「打不开」。
 */
async function openAppLog() {
  let path: string;
  try {
    path = await appLogPath();
    await probePath(path);
  } catch (e) {
    notify.fail(`应用日志没装上：${e}`);
    return;
  }
  await tabflow.openPath(path);
}

/**
 * 清空应用日志。
 *
 * 清完必须走一次 `worktree.changed()` —— 否则开着那份日志的标签上
 * 还摊着刚被清掉的几百行，人会以为没生效，然后再点一次。
 * 这正是那条老规矩的又一例（**盘上的东西被外部改了，两件事要一起做**），
 * 只不过这次「外部」是我们自己。
 */
async function clearLog() {
  try {
    await clearAppLog();
    await worktree.changed();
    notify.ok("应用日志已清空");
  } catch (e) {
    notify.fail(`清不掉应用日志：${e}`);
  }
}

/** 项目主页。交给系统默认浏览器 —— 这个应用自己不开网页 */
async function openRepoPage() {
  await openExternal("https://github.com/Spc-jgs/lite-ide-mac").catch(() => {
    notify.fail("打不开项目主页", 2600);
  });
}

export async function runMenu(id: string, ctx: MenuCtx) {
  if (id.startsWith("recent:")) {
    await tabflow.openRecent(id.slice("recent:".length));
    return;
  }
  switch (id) {
    case "open-folder": return void tabflow.openFolder();
    case "close-project": return tabflow.closeProject();
    case "new-scratch": return void tabflow.newScratch();
    case "open-scratch-dir": return void tabflow.openScratchDir();
    case "install-cli": return void installCliTool();
    case "recent-clear": project.recent = []; return;
    case "save": return docs.saveActive();
    case "save-as": return void worktree.saveAs();
    case "encoding":
      if (tabs.active) overlay.openEncoding();
      return;
    case "close-tab":
      if (tabs.active) tabflow.requestClose(tabs.active.id);
      return;
    case "close-all-tabs": return tabflow.closeMany(tabs.list.map((t) => t.id));
    case "toggle-mode":
      if (tabs.active) tabflow.requestSwitchMode(tabs.active);
      return;
    case "quick-all": overlay.openQuick("all"); return;
    case "quick-file": overlay.openQuick("file"); return;
    case "recent-files": overlay.openQuick("file"); return;
    case "quick-content": overlay.openQuick("content"); return;
    case "find-word": return overlay.findWordAtCursor();
    case "goto-line":
      // 编辑器跳行:列；日志视图跳行或时间（2026-09-21）。差异 / 合并没有「行」
      if (tabs.active?.mode === "edit" || tabs.active?.mode === "log") overlay.openGoto();
      return;
    case "nav-back": return void nav.go("back");
    case "nav-fwd": return void nav.go("fwd");
    case "outline": return overlay.openOutline();
    case "toggle-sidebar": layout.toggleSidebar(project.root !== null); return;
    case "toggle-panel": layout.panel = !layout.panel; return;
    case "toggle-scratch":
      // 已经在草稿视图上再点一次就收起侧边栏，和导轨上那个按钮同一个手势
      if (layout.sidebar && layout.sideView === "scratch") layout.sidebar = false;
      else layout.showSide("scratch");
      return;
    case "toggle-minimap": return ctx.toggleMinimap();
    case "zoom-in": return ctx.zoom(1);
    case "zoom-out": return ctx.zoom(-1);
    case "zoom-reset": return ctx.zoom(null);
    // 分屏（issue #35）。都作用在活动标签上；不成立的情况菜单项已经灰了，这里再兜一次
    case "split-right":
      if (tabs.activeId !== null) tabs.moveToGroup(tabs.activeId, 1);
      return tabs.audit("分屏");
    case "move-to-other-group":
      if (tabs.activeId !== null) tabs.moveToOther(tabs.activeId);
      return tabs.audit("挪组");
    case "focus-other-group":
      if (tabs.split) {
        tabs.focusGroup(tabs.activeGroup === 0 ? 1 : 0);
        docs.focusEditor();
      }
      return;
    case "unsplit":
      tabs.unsplit();
      return tabs.audit("合并分屏");
    case "toggle-wrap": {
      const t = tabs.active;
      if (t?.mode === "edit") t.wrap = !(t.wrap ?? wrapsByDefault(t.path));
      return;
    }
    case "new-terminal": terms.open(project.root ?? "~"); return;
    case "close-terminal":
      if (terms.activeId !== null) terms.close(terms.activeId);
      return;
    case "git-changes":
      // 已经在 Git 视图上再点一次就切回去，和 ⇧⌘G 是同一个手势
      layout.toggleGitChanges();
      return;
    case "git-file-diff": {
      const en = git.activeEntry;
      if (en) void git.openDiff(en, false);
      else notify.fail("当前文件没有未提交的改动", 2600);
      return;
    }
    case "git-blame":
      git.blameOn = !git.blameOn;
      if (git.blameOn && tabs.active?.mode !== "edit") notify.ok("注解已打开，打开一个仓库里的文件就能看到", 2600);
      return;
    case "git-log": layout.openGitTab("log"); return;
    case "git-console": layout.openGitTab("console"); return;
    case "git-branches": return ctx.openBranchPicker();
    case "git-refresh": return void git.refresh();
    case "git-pull": return void remote.pull();
    case "git-push": return void remote.askPush();
    case "git-fetch": return void remote.fetch("fetch");
    case "help-keys": overlay.keysOpen = true; return;
    case "help-repo": return void openRepoPage();
    case "help-log": return void openAppLog();
    case "help-log-clear": return void clearLog();
    default:
      diag(`菜单项 ${id} 没有对应的处理`);
  }
}
