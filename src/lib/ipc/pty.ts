/** 终端（pty）的命令包装（issue #32 瘦身）：只有 Terminal.svelte 用，它本身是懒的 */
import { invoke, type Channel } from "@tauri-apps/api/core";

/** 起一个终端；输出通过 Channel 流式回传 */
export const ptySpawn = (
  cwd: string,
  cols: number,
  rows: number,
  onData: Channel<number[] | ArrayBuffer>,
) => invoke<number>("pty_spawn", { cwd, cols, rows, onData });

export const ptyWrite = (id: number, data: string) => invoke<void>("pty_write", { id, data });

export const ptyResize = (id: number, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });

export const ptyKill = (id: number) => invoke<boolean>("pty_kill", { id });

/**
 * 报「这批字节 xterm 已经吃下去了」，把 Rust 侧的背压水位降下来（issue #18）。
 *
 * **必须在 `term.write(bytes, cb)` 的回调里叫。** 那个回调在 xterm 真的
 * 解析完之后才响 —— 而要限的正是「收到了但还没被消费」的那一段。
 * 收到就叫等于没有背压。
 *
 * 自己 catch：终端刚关掉时最后几条一定是打空的，那不是错误，
 * 更不该在界面上糊一句红字。
 */
export const ptyAck = (id: number, bytes: number) =>
  invoke<void>("pty_ack", { id, bytes }).catch(() => {});
