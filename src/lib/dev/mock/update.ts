/**
 * 桩：自动更新（docs/TAKEOVER.md MVP-8），对应 `src-tauri/src/commands/update.rs`（规则在 `src-tauri/src/update.rs`，线在 `crates/updsvc`）。
 *
 * 菜单「检查更新…」走一遍真的次序：查（0.6 秒）→ 下（1.5 秒，广播 downloading）→ 就绪（广播 ready，状态栏出「重启更新」）。
 * 「重启更新」在浏览器里没有东西可换：回 ready 不变，说一句
 */
import type { UpdateInfo } from "../../ipc/commands";
import { type A, NOT_MINE } from "./data";
import { mockEmit } from "./events";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let st: UpdateInfo = { phase: "idle", version: "", current: "1.5.0", notes: "", error: "" };
const set = (p: Partial<UpdateInfo>) => {
  st = { ...st, ...p };
  mockEmit("update-status", st);
};

export function updateCmd(cmd: string, _a: A): unknown {
  switch (cmd) {
    case "update_status":
      return st;
    case "update_check":
      if (st.phase === "ready") return st;
      return (async () => {
        set({ phase: "checking", version: "", error: "" });
        await sleep(600);
        set({ phase: "downloading", version: "1.6.0" });
        await sleep(1500);
        set({ phase: "ready", notes: "桩：新版本说明" });
        return st;
      })();
    case "update_apply":
      return Promise.reject("浏览器中无法安装更新（桩）");
    default:
      return NOT_MINE;
  }
}
