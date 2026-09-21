<script lang="ts">
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon, type ISearchOptions } from "@xterm/addon-search";
  import { Channel } from "@tauri-apps/api/core";
  import "@xterm/xterm/css/xterm.css";
  import { ptySpawn, ptyWrite, ptyResize, ptyKill, ptyAck } from "../ipc/pty";
  import TermSearch from "./TermSearch.svelte";

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
      count = r.resultCount === 0 ? "无匹配" : `${r.resultIndex < 0 ? "·" : r.resultIndex + 1}/${r.resultCount}`;
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
      return true;
    });
    term.open(host);
    fit.fit();

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
