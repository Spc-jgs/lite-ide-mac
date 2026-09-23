<script lang="ts">
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon, type ISearchOptions } from "@xterm/addon-search";
  import { Channel } from "@tauri-apps/api/core";
  import "@xterm/xterm/css/xterm.css";
  import { ptySpawn, ptyWrite, ptyResize, ptyKill, ptyAck } from "../ipc/pty";
  import TermSearch from "./TermSearch.svelte";
  import { docs } from "../state/docs.svelte";
  import { files } from "../state/files.svelte";
  import { project } from "../state/project.svelte";
  import { nav } from "../state/nav.svelte";
  import { openExternal } from "../ipc/app";
  import { stackFrame, frameResolver } from "../logview/stack-frame";
  import { findTermLinks, type LinkCtx, type TermLink } from "./links";
  import { dropText } from "./shell-quote";

  /*
   * 终端里的链接（links.ts）要的三样：文件索引做成 Set（每次悬停都要问好几个路径，几万条的数组
   * `includes` 太慢）、堆栈帧的解析器（按索引缓存）、起始目录相对项目根的那段。索引一换全跟着换。
   */
  let fileSet = $derived(new Set(files.list));
  let resolveFrame = $derived(frameResolver(files.list));
  function linkCtx(): LinkCtx {
    const root = project.root;
    return {
      root,
      cwdRel: root && (cwd === root || cwd.startsWith(`${root}/`)) ? cwd.slice(root.length + 1) : null,
      has: (rel) => fileSet.has(rel),
      frameAt: (text) => {
        const f = stackFrame(text);
        const rel = f && resolveFrame(f.suffix);
        return f && rel ? { from: f.from, to: f.to, rel, line: f.line } : null;
      },
    };
  }

  function openLink(l: TermLink) {
    if (l.kind === "url") {
      void openExternal(l.url).catch((e) => console.warn("打不开链接", e));
      return;
    }
    // 走 jumpTo：先把当前位置压栈，⌥⌘← / ⌘[ 回得来
    void nav.jumpTo({ from: 0, to: 0, text: l.rel, target: { rel: l.rel, line: l.line, why: "终端" } });
  }

  let { cwd, onExit }: { cwd: string; onExit: () => void } = $props();

  let host: HTMLDivElement | undefined = $state();
  let status = $state("正在启动 shell…");

  /*
   * ⌘F 查找（issue #34）。xterm 和 SearchAddon 在下面那条 effect 里建，这两个引用
   * 只是让框的回调够得着它们 —— 普通变量，没人要按它们渲染。
   * 「查找框开着」和「命中计数」才是状态。
   */
  let termRef: Terminal | null = null;
  let search: SearchAddon | null = null;
  let searchOpen = $state(false);
  let count = $state("");
  let box = $state<TermSearch | null>(null);
  /** 打开框那一刻上次搜的词（`last.term` 不是 $state，模板要读它得经这里） */
  let openedWith = $state("");
  /** 上一次的查询，↵ / ⇧↵ 接着它找 */
  let last: { term: string; opts: ISearchOptions } = { term: "", opts: {} };

  /*
   * 高亮的颜色。xterm 的装饰只吃字面色（同 theme 那段的理由），对着 app.css 的
   * `--search-hit`（accent 38%）和 `--accent` 抄；概览尺（右侧那条）用同一对。
   */
  const DECOR = {
    matchBackground: "#3a4f7a",
    activeMatchBackground: "#5b8def",
    matchOverviewRuler: "#5b8def",
    activeMatchColorOverviewRuler: "#ffffff",
  };

  function onQuery(t: string, o: { caseSensitive: boolean; wholeWord: boolean; regex: boolean }) {
    const prev = last.opts;
    last = { term: t, opts: { caseSensitive: o.caseSensitive, wholeWord: o.wholeWord, regex: o.regex, decorations: DECOR } };
    if (!search) return;
    if (!t) {
      search.clearDecorations();
      count = "";
      return;
    }
    /*
     * 开关变了要先把高亮清掉再搜。addon 0.16.0 的 `findNext` 先把 `lastSearchOptions`
     * 换成新的、再问「选项变了没」—— 拿新的和新的比，永远没变，于是词没变只改开关时
     * 高亮和计数留在旧的那套上（实测：Aa 一按，选区跳到大小写匹配的下一个，
     * 计数还写着 2/3）。清掉缓存的词，它就按「新搜索」走一遍。
     */
    if (prev.caseSensitive !== o.caseSensitive || prev.wholeWord !== o.wholeWord || prev.regex !== o.regex) {
      search.clearDecorations();
    }
    // incremental：框里的字还在长的时候，命中就地扩展而不是跳到下一个
    if (!search.findNext(t, { ...last.opts, incremental: true })) count = "无匹配";
  }
  const findNext = () => void (last.term && search?.findNext(last.term, last.opts));
  const findPrev = () => void (last.term && search?.findPrevious(last.term, last.opts));
  function openSearch() {
    openedWith = last.term;
    searchOpen = true;
    // 框这一拍还没挂上；下一拍再把焦点放进去
    queueMicrotask(() => box?.focus());
  }
  function closeSearch() {
    searchOpen = false;
    search?.clearDecorations();
    count = "";
    termRef?.focus();
  }

  $effect(() => {
    if (!host) return;
    let disposed = false;
    let ptyId: number | null = null;

    const term = new Terminal({
      /*
       * 查找的高亮走 `registerDecoration`（issue #34），xterm 6 把它归在 proposed API 里 ——
       * 不开这个开关，`findNext` 带 decorations 就抛。开关的含义只是「这些 API 的签名
       * 可能在版本间变」，而 addon-search 和 xterm 是同仓同版发的，签名变也一起变。
       * 我们自己的代码不直接调任何 proposed API。
       */
      allowProposedApi: true,
      // 必须写具体字体名，不能用 var(--code-font)：
      // xterm 拿这个字符串去做字符宽度测量（建一个测量元素读 offsetWidth），
      // CSS 变量在那个上下文解析不了，整条声明作废，最后回退到浏览器默认
      // 等宽字体——又丑、字距还不准。
      fontFamily: '"SF Mono", "JetBrains Mono", Menlo, Monaco, "Courier New", monospace',
      fontSize: 12.5,
      // 终端惯例是紧凑排布，1.2 太松散
      lineHeight: 1.15,
      letterSpacing: 0,
      cursorBlink: true,
      cursorStyle: "bar",
      cursorWidth: 2,
      // 滚回去看构建输出，一万行够用了
      scrollback: 10000,
      // 双击选中时把这些也算作单词的一部分，选路径和 URL 方便
      wordSeparator: " ()[]{}',\"`",
      // 与 app.css 的 IDEA Dark token 同源
      // 这几个色值必须跟 --content-solid / --text-dim 对齐：xterm 的 theme
      // 只吃字面色，读不了 CSS 变量。改调色板时**这里是唯一需要手动跟的地方**。
      theme: {
        background: "#1e1f22",
        foreground: "#dfe1e5",
        cursor: "#cdd0d5",
        selectionBackground: "#214283",
        black: "#1e1f22",
        red: "#f75464",
        green: "#6aab73",
        yellow: "#d6ae58",
        blue: "#548af7",
        magenta: "#c77dbb",
        cyan: "#2aacb8",
        white: "#dfe1e5",
        brightBlack: "#6f737b",
        brightRed: "#ff7a86",
        brightGreen: "#8fc99a",
        brightYellow: "#e6c67d",
        brightBlue: "#7ba7f9",
        brightMagenta: "#d99ed0",
        brightCyan: "#5cc4ce",
        brightWhite: "#ffffff",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    termRef = term;
    search = new SearchAddon();
    term.loadAddon(search);
    // 命中计数：只有带 decorations 的查找才会发这个事件（addon 的约定）
    search.onDidChangeResults((r) => {
      // 形状同编辑器的 countLabel：不在任何匹配上时「N 处」，在上面时「i/N」
      count = r.resultCount === 0 ? "无匹配" : r.resultIndex < 0 ? `${r.resultCount} 处` : `${r.resultIndex + 1}/${r.resultCount}`;
    });
    /*
     * ⌘F 在这儿截（keymap.ts 里 `term-find` 的 owner 是 `xterm`，同 CM6 的 ⌘F 一样
     * 由控件自己吃、一个字不进菜单）。返回 false = xterm 不再处理这个键。
     * 只认 keydown：xterm 对同一次按键会拿 keydown / keypress / keyup 各问一遍。
     */
    term.attachCustomKeyEventHandler((e) => {
      if (e.type === "keydown" && e.metaKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === "f") {
        openSearch();
        return false;
      }
      /*
       * 切标签的键（⌃Tab、⌘⇧[ ⌘⇧]）和回退 / 前进（⌘[ ⌘]）归窗口（App.svelte 的 keydown）：
       * 返回 false 让 xterm 别碰，事件照样冒泡上去。不放行的话 ⌃Tab 会变成一个 Tab 送进 shell
       * （zsh 当场弹补全），而焦点在终端里正是最想切回编辑器的时候。
       */
      if (e.ctrlKey && e.key === "Tab") return false;
      if (e.metaKey && ["[", "]", "{", "}"].includes(e.key)) return false;
      return true;
    });
    term.open(host);
    fit.fit();

    /*
     * 焦点进终端 = 人离开编辑器去跑命令了：把改过的文件存盘（`autosave.ts` 的 `leave`）。
     * IDEA 同一条（「切到内置终端时保存」）—— 改完代码点进终端敲 mvn，编译的得是改完那份。
     * 听 xterm 自己那个隐藏 textarea 的 focus：点进来、⌘J 打开、切终端标签都走它。
     */
    const onEnter = () => void docs.autosaveSweep(true, true);
    term.textarea?.addEventListener("focus", onEnter);

    // 从 Finder 拖进来的文件：插入转义好的路径（App.svelte 按落点派过来），同 Terminal.app
    const onDropPaths = (ev: Event) => {
      const paths = (ev as CustomEvent<string[]>).detail;
      if (ptyId === null || !paths?.length) return;
      void ptyWrite(ptyId, dropText(paths));
      term.focus();
    };
    // 记下挂在哪个元素上：清理时要从同一个上摘（闭包里的 `host` 到那时可能已经换了）
    const dropHost = host;
    dropHost.addEventListener("lite-drop-paths", onDropPaths);

    /*
     * 输出里的网址、项目文件路径、堆栈帧能 ⌘Click（links.ts）。⌘ 才开：终端里单击 / 拖选是在选字复制，
     * iTerm、VSCode 的终端也是 ⌘Click —— 和编辑器的 ⌘Click 跳转同一个手势。
     *
     * 两件要换算的事：
     * - **折行**：底部面板窄，maven 报的绝对路径动辄一百多个字，大半会折行 —— 而那正是最想点的时候。
     *   从这一行往上找到折行的起点、往下找到终点，拼回一整条逻辑行再找链接，范围可以跨行。
     * - **格子 ≠ 下标**：xterm 的范围按格子算，中文一个字占两格，正则给的是字符串下标。
     *   拼的时候给每个 UTF-16 单元记下它在第几行第几格、占几格。
     */
    const linkReg = term.registerLinkProvider({
      provideLinks(y, done) {
        const buf = term.buffer.active;
        let top = y - 1;
        while (top > 0 && buf.getLine(top)?.isWrapped) top--;
        let bottom = y - 1;
        while (buf.getLine(bottom + 1)?.isWrapped) bottom++;
        let text = "";
        const at: { x: number; y: number; w: number }[] = [];
        for (let r = top; r <= bottom; r++) {
          const line = buf.getLine(r);
          if (!line) break;
          for (let x = 0; x < line.length; x++) {
            const cell = line.getCell(x);
            if (!cell || cell.getWidth() === 0) continue; // 宽字符的后半格
            const ch = cell.getChars() || " ";
            for (let k = 0; k < ch.length; k++) at.push({ x, y: r, w: cell.getWidth() });
            text += ch;
          }
        }
        // 只要碰到问的这一行的（跨行的链接，上下几行各问一次，各自都给出同一条）
        const found = findTermLinks(text.trimEnd(), linkCtx()).filter(
          (l) => at[l.start].y <= y - 1 && at[l.end - 1].y >= y - 1,
        );
        if (found.length === 0) return done(undefined);
        done(
          found.map((l) => {
            const a = at[l.start];
            const z = at[l.end - 1];
            return {
              range: { start: { x: a.x + 1, y: a.y + 1 }, end: { x: z.x + z.w, y: z.y + 1 } },
              text: text.slice(l.start, l.end),
              activate: (e: MouseEvent) => {
                if (e.metaKey) openLink(l);
              },
              hover: () => term.element && (term.element.title = "⌘ + 点击打开"),
              leave: () => term.element && (term.element.title = ""),
            };
          }),
        );
      },
    });

    /*
     * 已经被 xterm 吃下、但还没报回 Rust 的字节数（issue #18 第一条）。
     *
     * **要攒着**，因为第一批数据可能比 `ptySpawn` 的返回值先到 —— 那时
     * 还不知道 pty id，报不出去。不攒的话那几片就永远留在 Rust 侧的
     * 未确认账上，而它是个只增不减的数：账上挂着几十 KB 的话，
     * 水位会被永久抬高一截，极端情况下一开始就卡在闸上。
     */
    let unacked = 0;
    const ack = (n: number) => {
      unacked += n;
      if (ptyId === null || unacked === 0) return;
      const batch = unacked;
      unacked = 0;
      void ptyAck(ptyId, batch);
    };

    // pty 输出流：Rust 侧读线程通过 Channel 推过来
    const chan = new Channel<number[] | ArrayBuffer>();
    chan.onmessage = (msg) => {
      // Vec<u8> 过 IPC 可能落成 number[] 或 ArrayBuffer，两种都接住。
      // 一律交给 xterm 按字节写入 —— 它自己处理 UTF-8 解码，
      // 多字节字符被切在两个 chunk 之间也不会乱码
      const bytes = msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg);
      /*
       * **回调在 xterm 真的解析完这批之后才响**，报的时机就该是那里。
       * 收到就报等于没有背压 —— 要限的正是「收到了但还没被消费」的那一段。
       */
      term.write(bytes, () => ack(bytes.length));
    };

    ptySpawn(cwd, term.cols, term.rows, chan)
      .then((id) => {
        if (disposed) {
          void ptyKill(id);
          return;
        }
        ptyId = id;
        // 把拿到 id 之前攒下的那些一次报上去
        ack(0);
        status = "";
        term.onData((d) => void ptyWrite(id, d));
        term.focus();
      })
      .catch((e) => (status = String(e)));

    // 容器尺寸变了就同步给 pty，否则 vim / less 的排版会错位
    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
        if (ptyId !== null) void ptyResize(ptyId, term.cols, term.rows);
      } catch {
        /* 面板收起时容器为 0，忽略 */
      }
    });
    ro.observe(host);

    return () => {
      term.textarea?.removeEventListener("focus", onEnter);
      dropHost.removeEventListener("lite-drop-paths", onDropPaths);
      linkReg.dispose();
      disposed = true;
      ro.disconnect();
      if (ptyId !== null) void ptyKill(ptyId);
      search = null;
      termRef = null;
      term.dispose();
      onExit();
    };
  });
</script>

<div class="term-wrap">
  {#if status}<div class="status">{status}</div>{/if}
  <div class="term" bind:this={host}></div>
  {#if searchOpen}
    <TermSearch bind:this={box} initial={openedWith} {count} {onQuery} onNext={findNext} onPrev={findPrev} onClose={closeSearch} />
  {/if}
</div>

<style>
  /*
   * 这里用**不透明**的那一份，而不是 --content-bg。
   *
   * xterm 在自己的画布上画底色，要让它跟着窗口透光就得开
   * `allowTransparency` —— 那个开关的代价是每一帧都要做一次额外的合成，
   * 而终端恰恰是全应用里滚得最快的地方（`cargo build` 刷屏时）。
   * 于是容器也用实色：不然画布够不到的那一圈边会透光，画布本身不透，
   * 看着像终端边上镶了一道亮边。
   */
  .term-wrap { position: relative; height: 100%; background: var(--content-solid); overflow: hidden; }
  .term { height: 100%; padding: 4px 0 0 8px; }
  .status {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 12px;
    pointer-events: none;
  }
  /* xterm 自己管内部 DOM，这里只保证它撑满 */
  .term :global(.xterm) { height: 100%; }
  .term :global(.xterm-viewport) { background: transparent !important; }
</style>
