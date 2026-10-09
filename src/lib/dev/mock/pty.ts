/** 桩：终端：起、写、改尺寸、背压确认、杀。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, NOT_MINE } from "./data";

/** 假 shell 的句柄表（见 `pty_spawn`）：id → 回传 Channel */
let mockPtyId = 0;
const mockPty = new Map<number, { onmessage?: (b: number[]) => void } | undefined>();
/** 起在哪个目录。假 shell 不执行 `cd`，所以前台进程的目录永远是它（`pty_cwd`） */
const mockCwd = new Map<number, string>();

export async function ptyCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    /*
     * 假 shell：只会回显（issue #34 加的）。原来 spawn 只返回一个 id、write 落进黑洞，
     * 终端里永远是一个光标 —— 于是「终端里的 ⌘F 查找」在浏览器里**一个字都搜不到**，
     * 只能等 45 秒一轮的 .app。现在：spawn 先吐一条横幅，敲什么回显什么，
     * 回车换行并再给一个提示符。不模拟任何命令。
     */
    case "pty_spawn": {
      const ch = a.onData as { onmessage?: (b: number[]) => void } | undefined;
      const id = ++mockPtyId;
      mockPty.set(id, ch);
      mockCwd.set(id, String(a.cwd));
      const enc = new TextEncoder();
      const say = (t: string) => ch?.onmessage?.([...enc.encode(t)]);
      setTimeout(() => say(`lite-ide 桩 shell（只回显，不执行）  cwd=${String(a.cwd)}\r\n$ `), 30);
      return id;
    }
    case "pty_write": {
      const ch = mockPty.get(Number(a.id));
      const enc = new TextEncoder();
      const d = String(a.data);
      // 回车 → 换行 + 新提示符；退格 → 退一格擦掉；其余原样回显（粘贴进来的一串里也可能夹着回车）
      const out = d === "\x7f" ? "\b \b" : d.replace(/\r\n|\r|\n/g, "\r\n$ ");
      ch?.onmessage?.([...enc.encode(out)]);
      return null;
    }
    case "pty_cwd":
      return mockCwd.get(Number(a.id)) ?? null;
    case "pty_resize":
    // 桩里没有真 pty，也就没有要背压的对象。但这条 case 必须在 ——
    // 落到 default 的话浏览器里每写一批终端输出就报一次「未知命令」
    case "pty_ack":
      return null;
    case "pty_kill":
      // 同真实现：返回「这个终端之前在不在」。原来这里少了 return，一路贯穿进 open_log ——
      // 关一个终端，桩就开了一个日志句柄（前端丢弃返回值，所以一直没人看见）
      mockCwd.delete(Number(a.id));
      return mockPty.delete(Number(a.id));
    default:
      return NOT_MINE;
  }
}
