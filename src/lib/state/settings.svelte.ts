import { invoke } from "@tauri-apps/api/core";
import { adoptUiState, getSettings, listenHere, setUiState, stepFont, type Settings } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { DEFAULT_SETTINGS, LEGACY_MOVED_KEY, cssVars, readLegacyPrefs, type UiKey } from "./settings-view";

/**
 * 设置（issue #44，docs/SETTINGS.md 第 7 节）。**主人是 Rust**：`settings.json`（你写的）和 `ui-state.json`（按钮和 ⌘= 改的）
 * 都在那边读、合并、存；这里只拿合好的结果用，变了跟着换。
 *
 * 原来 5 个偏好在 localStorage 里各窗口各读各的（App 的缩略图和字号、FileTree 的紧凑和跟随、GitPane 的分组）：
 * A 里关了缩略图，B 要重开才知道；裸二进制和 `.app` 的 localStorage 还不是同一份。跨窗口共享的东西只能有一个主人
 * （MULTIWINDOW.md 3.5），和「最近打开」收归 Rust 是同一件事。
 */
class SettingsStore {
  v = $state<Settings>(DEFAULT_SETTINGS);
  /** 不在组件里、拿不到 `$effect` 的地方（CM6 的扩展）要知道设置变了：在这儿登记 */
  #subs = new Set<(s: Settings) => void>();

  subscribe(cb: (s: Settings) => void): () => void {
    this.#subs.add(cb);
    return () => this.#subs.delete(cb);
  }

  /** 换上一份新的：存下来、写 CSS 变量。Rust 回的、广播来的都走这里 */
  apply(s: Settings) {
    this.v = s;
    /*
     * 诊断：哪个窗口换上了什么（只在 LITE_IDE_DEBUG=1 时有输出）。「A 里按了 ⌘=，B 跟着变了没有」「改了 settings.json
     * 为什么没生效」这类问题，真 .app 上没有开发者工具，只能看这一行。窗口名直接读 Tauri 挂在页面上的元数据 ——
     * 不为一行诊断去引窗口 API（frontend.md：一个 import 能把入口包拽大一截）
     */
    const label = (globalThis as { __TAURI_INTERNALS__?: { metadata?: { currentWebview?: { label?: string } } } }).__TAURI_INTERNALS__?.metadata?.currentWebview?.label ?? "?";
    invoke("diag", { msg: `settings → ${label}: editor=${s.editorFontSize}px terminal=${s.terminalFontSize}px minimap=${s.minimap} problems=${s.problems.length}` }).catch(() => {});
    for (const cb of this.#subs) cb(s);
    if (typeof document === "undefined") return; // 状态测试里没有 document
    for (const [k, val] of cssVars(s)) document.documentElement.style.setProperty(k, val);
  }

  /**
   * 挂载之前（main.ts）调一次：取设置、迁旧偏好、挂上广播。
   *
   * **在挂载之前 await，不做首屏缓存**：第 0 步实测挂载前一次 IPC 往返 < 1ms（三个窗口一起恢复时也一样），
   * 等它回来再画第一帧，字体字号一开始就是对的。**不许抛**（启动路径上）：取不到就用默认值，应用照常起来
   */
  async init() {
    let s = DEFAULT_SETTINGS;
    try {
      s = await getSettings();
      s = await this.#migrate(s);
    } catch {
      /* 取不到就默认值 —— 设置坏了不能让应用打不开 */
    }
    this.apply(s);
    // 广播：别的窗口改了开关、按了 ⌘=，或者你改了 settings.json
    void listenHere<Settings>("settings-changed", (e) => this.apply(e.payload)).catch(() => {});
  }

  /** 升级后第一次：localStorage 里的旧偏好交给 Rust（收不收那边定），然后记一笔迁过了。旧键不删 */
  async #migrate(s: Settings): Promise<Settings> {
    const legacy = (() => {
      try {
        return readLegacyPrefs((k) => localStorage.getItem(k));
      } catch {
        return null;
      }
    })();
    if (!legacy) return s;
    // 交失败了就不记「迁过了」：下次启动再交。记了的话这几个旧偏好就永远丢了
    const next = await adoptUiState(legacy).catch(() => null);
    if (!next) return s;
    try {
      localStorage.setItem(LEGACY_MOVED_KEY, "1");
    } catch {
      /* 记不下就下次再交一次，Rust 那边不会重复收 */
    }
    return next;
  }

  /** 切一个开关（缩略图、文件树紧凑 / 跟随、Git 分组）。用 Rust 回的那份，不自己先改：两个窗口同时点也只有一个结果 */
  toggle(key: UiKey) {
    const cur = { "editor.minimap": this.v.minimap, "tree.compact": this.v.treeCompact, "tree.follow": this.v.treeFollow, "git.grouped": this.v.gitGrouped }[key];
    setUiState(key, !cur)
      .then((s) => this.apply(s))
      .catch((e) => notify.fail(`改不了：${e}`));
  }

  /** ⌘= / ⌘- / ⌘0（null） */
  zoom(delta: number | null) {
    stepFont(delta)
      .then((s) => this.apply(s))
      .catch((e) => notify.fail(`改不了字号：${e}`));
  }
}

export const settings = new SettingsStore();
