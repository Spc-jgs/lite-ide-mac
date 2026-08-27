<script lang="ts">
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { Channel } from "@tauri-apps/api/core";
  import "@xterm/xterm/css/xterm.css";
  import { ptySpawn, ptyWrite, ptyResize, ptyKill } from "../ipc/commands";

  let { cwd, onExit }: { cwd: string; onExit: () => void } = $props();

  let host: HTMLDivElement | undefined = $state();
  let status = $state("正在启动 shell…");

  $effect(() => {
    if (!host) return;
    let disposed = false;
    let ptyId: number | null = null;

    const term = new Terminal({
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
    term.open(host);
    fit.fit();

    // pty 输出流：Rust 侧读线程通过 Channel 推过来
    const chan = new Channel<number[] | ArrayBuffer>();
    chan.onmessage = (msg) => {
      // Vec<u8> 过 IPC 可能落成 number[] 或 ArrayBuffer，两种都接住。
      // 一律交给 xterm 按字节写入 —— 它自己处理 UTF-8 解码，
      // 多字节字符被切在两个 chunk 之间也不会乱码
      const bytes = msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg);
      term.write(bytes);
    };

    ptySpawn(cwd, term.cols, term.rows, chan)
      .then((id) => {
        if (disposed) {
          void ptyKill(id);
          return;
        }
        ptyId = id;
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
      term.dispose();
      onExit();
    };
  });
</script>

<div class="term-wrap">
  {#if status}<div class="status">{status}</div>{/if}
  <div class="term" bind:this={host}></div>
</div>

<style>
  .term-wrap { position: relative; height: 100%; background: var(--editor-bg); overflow: hidden; }
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
