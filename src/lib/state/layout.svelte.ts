import { DEFAULT_LAYOUT, type Layout } from "./session";

/**
 * 外壳的布局状态：侧边栏开合 / 宽度 / 视图，底部面板开合 / 高度 / 工具窗 / 标签。
 *
 * # 为什么单独一个模块
 *
 * 这几个值原来是 App.svelte 里的七个裸 `$state`，导轨、侧边栏、底部面板、
 * 菜单命令、会话快照都直接读写它们 —— 于是这几块谁也搬不出 App.svelte
 * （issue #9）。搬出来之后，每个工具窗组件自己读写这里，App 不再当中转站，
 * 也不用把七个值和七个回调一层层往下传。
 *
 * 用 class + `$state` 字段，和 `notify.svelte.ts` 同一个写法：模块导出的绑定
 * 不能被外面重新赋值，所以裸的 `export let x = $state()` 在别的文件里写不了，
 * 得是一个对象上的字段。
 *
 * # 什么进快照、什么不进
 *
 * `snapshot()` 返回的那几个字段进 `session.Layout`。`resizing` 是拖拽中的瞬态，
 * 只用来在拖的时候关掉 grid 的过渡（不关就是一路追不上手的橡皮筋），不进快照。
 */
class LayoutState {
  sidebar = $state(DEFAULT_LAYOUT.sidebar);
  sidebarWidth = $state(DEFAULT_LAYOUT.sidebarWidth);
  /** 侧边栏当前显示哪个视图。不在仓库里时渲染侧强制回文件树 */
  sideView = $state<Layout["sideView"]>(DEFAULT_LAYOUT.sideView);
  panel = $state(DEFAULT_LAYOUT.panel);
  panelHeight = $state(DEFAULT_LAYOUT.panelHeight);
  /**
   * 底部是哪个工具窗（`term` / `git`），以及 Git 窗里选中的标签（#31）。
   * 都是**存下来的偏好**：可以是 `git` 而当下并没有仓库，渲染侧另算。
   */
  panelView = $state<Layout["panelView"]>(DEFAULT_LAYOUT.panelView);
  gitTab = $state<Layout["gitTab"]>(DEFAULT_LAYOUT.gitTab);
  /** 正在拖侧边栏的宽度。瞬态，不进快照 */
  resizing = $state(false);

  /** 启动时从快照恢复。只调一次，在任何 effect 跑之前 */
  restore(l: Layout) {
    this.sidebar = l.sidebar;
    this.sidebarWidth = l.sidebarWidth;
    this.sideView = l.sideView;
    this.panel = l.panel;
    this.panelHeight = l.panelHeight;
    this.panelView = l.panelView;
    this.gitTab = l.gitTab;
  }

  /** 拿去存快照的那份。在 effect 里调一次就把七个字段全订阅上了 */
  snapshot(): Layout {
    return {
      sidebar: this.sidebar,
      sidebarWidth: this.sidebarWidth,
      sideView: this.sideView,
      panel: this.panel,
      panelHeight: this.panelHeight,
      panelView: this.panelView,
      gitTab: this.gitTab,
    };
  }

  /** 把侧边栏切到某个视图并确保它是展开的 */
  showSide(view: Layout["sideView"]) {
    this.sideView = view;
    this.sidebar = true;
  }

  /** ⇧⌘G：已经在 Git 视图上再按一次就切回文件树 —— 一个键既是去也是回 */
  toggleGitChanges() {
    this.sideView = this.sidebar && this.sideView === "git" ? "files" : "git";
    this.sidebar = true;
  }
}

export const layout = new LayoutState();
