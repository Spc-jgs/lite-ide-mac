/**
 * 查找 / 替换面板 —— 换掉 CM6 自带的那一个。
 *
 * # 为什么要自己画
 *
 * 默认面板是一排**裸的 HTML 控件**：三个 `<input type=checkbox>` 各带一个
 * 文字标签（`match case` / `regexp` / `by word`）、五个 `<button>`
 * （`next` `previous` `all` `replace` `replace all`）、两个输入框，
 * 一律浏览器默认样式，横着摊了一整行。这个应用别的地方一个原生控件都没有，
 * 它一开就是唯一一块「长得不像这个应用」的界面。
 *
 * 更实际的两条：
 *
 * 1. **没有「第几个 / 共几个」。** 这是查找里信息量最大的一个数 ——
 *    「有没有」「还有多少」「我转了一圈没有」全靠它。默认面板一个字都不给。
 * 2. **替换那一行永远占着位置。** 十次查找里有九次不替换，而它一直在那儿
 *    挡着两行正文。
 *
 * 换掉之后：开关做成 `Aa` `W` `.*` 三个方块钉在输入框**里面**（IDEA / VSCode
 * 都是这么放的 —— 它们是这次查找的属性，不是旁边的独立控件），计数跟着它们
 * 一起放在框里，替换那一行默认收起来。
 *
 * # ⌥⌘F 以前是个假键位
 *
 * `keymap.ts` 里 `cm-replace` 标的是 ⌥⌘F，注释写「CM6 的 `searchKeymap` 给的」——
 * **那是错的**：`searchKeymap` 一共七条绑定，没有 `Mod-Alt-f`
 * （`Mod-f` `F3` `Mod-g` `Escape` `Mod-Shift-l` `Mod-Alt-g` `Mod-d`）。
 * 也就是说速查表上挂着一个按下去什么都不会发生的键。现在它真的接上了：
 * 打开面板并展开替换行。
 *
 * # 计数是有闸的
 *
 * 数匹配数要扫全文，而它挂在每一次输入上。两道闸：**命中数到 999 就停**
 * （再多也只会显示 `999+`，人不会去数第 1000 个），**文档超过 4MB 直接不数**
 * （那种文件本来就该用日志模式的过滤，而不是在这儿一遍遍全文扫）。
 * 另外整件事压后 90ms 做，连打时只算最后一次。
 */

import {
  SearchQuery,
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  openSearchPanel,
  replaceAll,
  replaceNext,
  search,
  setSearchQuery,
} from "@codemirror/search";
import type { Extension } from "@codemirror/state";
import { Prec } from "@codemirror/state";
import { countLabel, countMatches } from "./search-count";
import { EditorView, keymap, runScopeHandlers, type Panel } from "@codemirror/view";

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls: string,
  attrs: Record<string, string> = {},
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  e.className = cls;
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}

/**
 * 面板本体。
 *
 * `openReplace` 是**模块级**的，不是每个面板一份：人一旦展开过替换，
 * 下一次开面板多半还要 —— 每次都收回去等于每次都要多按一下。
 * 它跟着窗口活着，不落盘（这是手感不是设置）。
 */
let openReplace = false;

/** 让 ⌥⌘F 能在面板已经开着的时候把替换行展开 —— 它得找得到那个面板 */
const panels = new WeakMap<EditorView, { expand: () => void }>();

function createPanel(view: EditorView): Panel {
  const q0 = getSearchQuery(view.state);

  const root = el("div", "cm-lite-search");
  const twist = el("button", "ls-twist", {
    type: "button",
    "aria-label": "展开 / 收起替换",
    title: "替换（⌥⌘F）",
  });
  const rows = el("div", "ls-rows");
  const rFind = el("div", "ls-row");
  const rRep = el("div", "ls-row ls-rep");

  // ── 查找行 ───────────────────────────────────────────────────
  const fBox = el("div", "ls-box");
  const find = el("input", "ls-in", {
    // CM6 靠这个属性找输入框：⌘F 再按一次要能重新聚焦并把选中的词填进来
    // （`openSearchPanel` 里 `getSearchInput` 查的就是它）。少了它，
    // 「选中一个词按 ⌘F」这条最常用的路会变成「打开一个空面板」。
    "main-field": "true",
    placeholder: "查找",
    "aria-label": "查找",
    spellcheck: "false",
  });
  find.value = q0.search;
  const count = el("span", "ls-count");
  const tgCase = el("button", "ls-tg", { type: "button", title: "区分大小写" });
  tgCase.textContent = "Aa";
  const tgWord = el("button", "ls-tg", { type: "button", title: "全词匹配" });
  tgWord.textContent = "W";
  const tgRe = el("button", "ls-tg", { type: "button", title: "正则表达式" });
  tgRe.textContent = ".*";
  fBox.append(find, count, tgCase, tgWord, tgRe);

  const prev = el("button", "ls-nav", { type: "button", title: "上一个（⇧↵）" });
  prev.textContent = "↑";
  const next = el("button", "ls-nav", { type: "button", title: "下一个（↵）" });
  next.textContent = "↓";
  const close = el("button", "ls-nav ls-close", { type: "button", title: "关闭（esc）" });
  close.textContent = "✕";
  rFind.append(fBox, prev, next, close);

  // ── 替换行 ───────────────────────────────────────────────────
  const rBox = el("div", "ls-box");
  const rep = el("input", "ls-in", {
    placeholder: "替换为",
    "aria-label": "替换为",
    spellcheck: "false",
  });
  rep.value = q0.replace;
  rBox.append(rep);
  const doOne = el("button", "ls-bt", { type: "button", title: "替换当前这个（↵）" });
  doOne.textContent = "替换";
  const doAll = el("button", "ls-bt", { type: "button" });
  doAll.textContent = "全部";
  rRep.append(rBox, doOne, doAll);

  rows.append(rFind, rRep);
  root.append(twist, rows);

  // ── 状态同步 ─────────────────────────────────────────────────

  function commit() {
    view.dispatch({
      effects: setSearchQuery.of(
        new SearchQuery({
          search: find.value,
          replace: rep.value,
          caseSensitive: tgCase.classList.contains("on"),
          wholeWord: tgWord.classList.contains("on"),
          regexp: tgRe.classList.contains("on"),
        }),
      ),
    });
  }

  function setTwist() {
    root.classList.toggle("open", openReplace);
    twist.textContent = openReplace ? "⌄" : "›";
    twist.setAttribute("aria-expanded", String(openReplace));
  }

  const syncToggles = (q: SearchQuery) => {
    tgCase.classList.toggle("on", q.caseSensitive);
    tgWord.classList.toggle("on", q.wholeWord);
    tgRe.classList.toggle("on", q.regexp);
    find.classList.toggle("bad", !!q.search && !q.valid);
  };

  /*
   * 计数压后做。90ms 是「连打时不算中间态」和「停手之后立刻有数」之间的折中 ——
   * 比它短，连打十个字就是十次全文扫；比它长，停下来之后那格会明显地空一拍。
   */
  let timer: ReturnType<typeof setTimeout> | null = null;
  function scheduleCount() {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      const q = getSearchQuery(view.state);
      count.textContent = countLabel(q, countMatches(view.state, q));
    }, 90);
  }

  // ── 事件 ─────────────────────────────────────────────────────

  find.addEventListener("input", commit);
  rep.addEventListener("input", commit);
  for (const t of [tgCase, tgWord, tgRe]) {
    t.addEventListener("click", () => {
      t.classList.toggle("on");
      commit();
      find.focus();
    });
  }
  prev.addEventListener("click", () => findPrevious(view));
  next.addEventListener("click", () => findNext(view));
  close.addEventListener("click", () => {
    closeSearchPanel(view);
    view.focus();
  });
  doOne.addEventListener("click", () => replaceNext(view));
  doAll.addEventListener("click", () => replaceAll(view));
  twist.addEventListener("click", () => {
    openReplace = !openReplace;
    setTwist();
    (openReplace ? rep : find).focus();
  });

  root.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      // 在替换框里按 ↵ 就是替换这一个 —— 手已经在那儿了
      if (e.target === rep) replaceNext(view);
      else if (e.shiftKey) findPrevious(view);
      else findNext(view);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      closeSearchPanel(view);
      view.focus();
      return;
    }
    /*
     * **剩下的键要手动转交，否则面板里按什么快捷键都没反应。**
     *
     * CM6 的 `keymap` 挂在 `contentDOM` 上，而面板不在 `contentDOM` 里面 ——
     * 焦点一进查找框，⌘F、F3、⌘G、⌥⌘F 全都到不了。默认面板正是靠这一句活的，
     * 抄它的时候只看见了 DOM，没看见这一句。
     *
     * 浏览器里实测到的形状：按 ⌥⌘F 什么都不发生，面板还开着、也没报错 ——
     * 典型的「装了不生效」，不看代码根本猜不到是作用域的问题。
     *
     * 走 `search-panel` 作用域：`searchKeymap` 里那几条正是按这个作用域注册的，
     * 我们自己那条 `Mod-Alt-f` 也标了同一个。
     */
    if (runScopeHandlers(view, e, "search-panel")) e.preventDefault();
  });

  syncToggles(q0);
  setTwist();
  panels.set(view, {
    expand: () => {
      openReplace = true;
      setTwist();
      rep.focus();
      rep.select();
    },
  });

  return {
    dom: root,
    top: true,
    mount() {
      /*
       * **必须自己抢焦点。** CM6 不会替面板做这件事 —— 默认面板是在自己的
       * `mount` 里 `select()` 的。少了这两句，⌘F 之后面板确实开了、看着也对，
       * 而敲进去的字**全落进正文**（浏览器里实测：按 ⌘F 打「Client」，
       * 第 8 行当场变成 `orderClieClientnt`，标签还多了个未保存的点）。
       *
       * `select()` 是配套的：⌘F 常常带着一个选中的词进来
       * （`openSearchPanel` 会用选区填 query），全选上才能一打就换掉。
       */
      find.focus();
      find.select();
      scheduleCount();
    },
    update(u) {
      const q = getSearchQuery(u.state);
      // 只在真不一样时写回：这个 handler 也会被自己 dispatch 的事务叫醒，
      // 无条件赋值会把正在输入的光标位置冲掉
      if (find.value !== q.search) find.value = q.search;
      if (rep.value !== q.replace) rep.value = q.replace;
      syncToggles(q);
      if (u.docChanged || u.selectionSet || u.transactions.some((t) => t.effects.length)) {
        scheduleCount();
      }
    },
    destroy() {
      if (timer) clearTimeout(timer);
      panels.delete(view);
    },
  };
}

/*
 * 面板的样子跟着面板走，不进 `theme-idea-dark.ts` —— 那份是**配色**
 * （语法着色、选区、匹配高亮），换主题时整份替换；这些是这一个组件的布局，
 * 换什么主题都一样。
 *
 * **每条选择器都是两个 class**，这一点是有意的：`theme-idea-dark.ts` 里那条
 * `.cm-panels input, .cm-panels button`（给 ⌥⌘G 跳行那个默认面板用的，还得留着）
 * 会把边框、内边距、底色刷到这里每一个 input 和 button 上。
 * 两个 class 是 (0,2,0)，压得过它的 (0,1,1) —— 靠特指度定胜负，
 * 不靠「谁后注入」。样式表的注入顺序是 CM6 说了算的，赌它就是赌运气。
 */
const panelTheme = EditorView.theme({
  ".cm-lite-search": {
    display: "flex",
    alignItems: "flex-start",
    gap: "6px",
    padding: "6px 8px",
    fontFamily: "var(--ui-font)",
  },
  ".cm-lite-search .ls-rows": {
    flex: "1",
    minWidth: "0",
    display: "flex",
    flexDirection: "column",
    gap: "5px",
  },
  ".cm-lite-search .ls-row": { display: "flex", alignItems: "center", gap: "5px" },
  // 收起时**不占位**，不是变透明 —— 面板要真的只有一行高
  ".cm-lite-search .ls-rep": { display: "none" },
  ".cm-lite-search.open .ls-rep": { display: "flex" },

  ".cm-lite-search .ls-twist": {
    flex: "none",
    width: "16px",
    height: "26px",
    padding: "0",
    background: "transparent",
    border: "none",
    color: "var(--text-faint)",
    fontSize: "11px",
    lineHeight: "1",
    cursor: "default",
  },
  ".cm-lite-search .ls-twist:hover": { color: "var(--text)" },

  /*
   * 开关和计数长在输入框**里面**。它们讲的是「这一次查找是什么样的」，
   * 摆在框外面就变成了三个和输入框平级的独立控件 —— 默认面板正是那样，
   * 一行摊开七八个东西，眼睛得先分辨哪个跟哪个是一伙的。
   */
  ".cm-lite-search .ls-box": {
    flex: "1",
    minWidth: "0",
    display: "flex",
    alignItems: "center",
    gap: "2px",
    height: "26px",
    padding: "0 3px 0 8px",
    background: "var(--elevated-hi)",
    border: "1px solid var(--border)",
    borderRadius: "var(--r-sm)",
  },
  ".cm-lite-search .ls-box:focus-within": { borderColor: "var(--accent)" },
  ".cm-lite-search .ls-in": {
    flex: "1",
    minWidth: "0",
    height: "100%",
    padding: "0",
    background: "transparent",
    border: "none",
    outline: "none",
    color: "var(--text)",
    fontFamily: "var(--code-font)",
    fontSize: "12px",
  },
  // 正则写坏了：让框里的字变红，不弹任何东西 —— 边打边写的正则大半时间都是坏的
  ".cm-lite-search .ls-in.bad": { color: "var(--lvl-error)" },
  ".cm-lite-search .ls-count": {
    flex: "none",
    padding: "0 4px",
    color: "var(--text-faint)",
    fontFamily: "var(--code-font)",
    fontSize: "11px",
    whiteSpace: "nowrap",
  },
  ".cm-lite-search .ls-tg": {
    flex: "none",
    display: "grid",
    placeContent: "center",
    width: "20px",
    height: "20px",
    padding: "0",
    background: "transparent",
    border: "none",
    borderRadius: "var(--r-sm)",
    color: "var(--text-faint)",
    fontFamily: "var(--code-font)",
    fontSize: "10.5px",
    fontWeight: "600",
    lineHeight: "1",
    cursor: "default",
  },
  ".cm-lite-search .ls-tg:hover": { background: "var(--hover)", color: "var(--text-dim)" },
  ".cm-lite-search .ls-tg.on": { background: "var(--accent)", color: "#fff" },

  ".cm-lite-search .ls-nav": {
    flex: "none",
    display: "grid",
    placeContent: "center",
    width: "24px",
    height: "26px",
    padding: "0",
    background: "transparent",
    border: "none",
    borderRadius: "var(--r-sm)",
    color: "var(--text-dim)",
    fontSize: "12px",
    lineHeight: "1",
    cursor: "default",
  },
  ".cm-lite-search .ls-nav:hover": { background: "var(--hover)", color: "var(--text)" },
  ".cm-lite-search .ls-bt": {
    flex: "none",
    height: "26px",
    padding: "0 10px",
    background: "transparent",
    border: "1px solid var(--border)",
    borderRadius: "var(--r-sm)",
    color: "var(--text-dim)",
    fontFamily: "var(--ui-font)",
    fontSize: "11.5px",
    cursor: "default",
  },
  ".cm-lite-search .ls-bt:hover": { background: "var(--hover)", color: "var(--text)" },
});

/**
 * 装上查找面板 + 那条一直没接上的 ⌥⌘F。
 *
 * `Prec.highest` 的理由同 `jump-ext` 的 ⌘B：`searchKeymap` 里没有这条，
 * 但别的扩展将来可能有，而「装了不生效」这类问题不报错、只是按了没反应。
 */
export function searchPanel(): Extension {
  return [
    search({ top: true, createPanel }),
    panelTheme,
    Prec.highest(
      keymap.of([
        {
          key: "Mod-Alt-f",
          // 两个作用域都要：焦点在正文里按（editor）和焦点在查找框里按
          // （search-panel，经面板的 `runScopeHandlers` 转交）都得管用
          scope: "editor search-panel",
          preventDefault: true,
          run: (view) => {
            const p = panels.get(view);
            if (p) {
              p.expand();
              return true;
            }
            /*
             * 面板还没开：先把展开状态置位，`createPanel` 会照着它画。
             *
             * **这里必须自己调 `openSearchPanel`。** `searchKeymap` 里没有
             * `Mod-Alt-f`（那正是这条键位以前是假的原因），返回 `false`
             * 底下没有人接得住 —— 按下去仍然什么都不会发生。
             */
            openReplace = true;
            return openSearchPanel(view);
          },
        },
      ]),
    ),
  ];
}
