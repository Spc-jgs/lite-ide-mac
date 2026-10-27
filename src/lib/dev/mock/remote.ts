/** 桩：Git 拉取与推送：进度一条条推、可取消。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, NOT_MINE, sleep } from "./data";

/** 远程操作的假 id，和被取消的那些 */
let mockRemoteId = 0;
const mockCancelled = new Set<number>();

export async function remoteCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    /*
     * 进度必须**真的分段推**，不能一次给个 100%。
     *
     * 桩比真实现快，而「进度条、节流、取消」全是时序类的东西 ——
     * 一次推完的话，在浏览器里怎么点都是好的，而真机上可能一路是
     * 0% 然后突然结束。这条教训在会话恢复那个闪烁 bug 上吃过一次
     * （AGENTS.md「桩比真实现快，所以验红这一步在它上面会失效」）。
     */
    case "git_fetch":
    case "git_push": {
      const ch = a.onProgress as { onmessage?: (p: unknown) => void } | undefined;
      // id 由调用方给（真实现同理），桩要照做，否则取消对不上号
      const id = Number(a.opId ?? ++mockRemoteId);
      mockCancelled.delete(id);
      const phases: [string, number][] = [
        ["Enumerating objects", 1248],
        ["Counting objects", 1248],
        ["Compressing objects", 498],
        [cmd === "git_push" ? "Writing objects" : "Receiving objects", 1248],
        ["Resolving deltas", 722],
      ];
      for (const [phase, total] of phases) {
        for (const pct of [0, 37, 74, 100]) {
          if (mockCancelled.has(id)) {
            mockCancelled.delete(id);
            throw { kind: "cancelled", message: "已取消", raw: "" };
          }
          await sleep(90);
          ch?.onmessage?.({
            phase,
            percent: pct,
            done: Math.round((total * pct) / 100),
            total,
            finished: pct === 100,
          });
        }
      }
      // 桩里让「没有上游的推送」走一次被拒，好在浏览器里能看到那条路
      if (cmd === "git_push" && a.branch === "rejected-demo") {
        throw {
          kind: "rejected",
          message: "远程包含本地没有的提交，推送被拒绝",
          raw: "! [rejected]  dev -> dev (non-fast-forward)",
        };
      }
      return null;
    }
    /*
     * 克隆（欢迎页，G27）：和 fetch 一样分段推进度、能取消（桩比真实现快，「克隆中」那个状态不让它慢一点在浏览器里看不见）。
     * 地址里带 `auth` 的走一次认证失败、带 `fail` 的走一次别的失败，好看两种报错的样子；成功的「克隆」到 /proj（桩里唯一有内容的项目）
     */
    case "git_clone": {
      const ch = a.onProgress as { onmessage?: (p: unknown) => void } | undefined;
      const id = Number(a.opId);
      mockCancelled.delete(id);
      const url = String(a.url);
      for (const [phase, pct] of [["Cloning into '" + String(a.name) + "'", null], ["Receiving objects", 30], ["Receiving objects", 80], ["Resolving deltas", 100]] as [string, number | null][]) {
        await sleep(400);
        if (mockCancelled.has(id)) {
          mockCancelled.delete(id);
          throw { kind: "cancelled", message: "已取消", raw: "" };
        }
        ch?.onmessage?.({ phase, percent: pct, done: null, total: null, finished: pct === 100 });
      }
      if (url.includes("auth")) {
        throw { kind: "auth-https", message: "此仓库需要先完成认证", raw: "fatal: could not read Username for 'https://github.com': terminal prompts disabled" };
      }
      if (url.includes("fail")) {
        throw { kind: "other", message: "fatal: repository not found", raw: "fatal: repository not found" };
      }
      return "/proj";
    }
    // 从地址猜目录名：抄 `gitsvc::remote::clone_name`
    case "git_clone_name": {
      const last = String(a.url).trim().replace(/\/+$/, "").split(/[/:]/).pop() ?? "";
      const n = last.replace(/\.git$/, "");
      return n === "." || n === ".." ? "" : n;
    }
    case "git_cancel":
      mockCancelled.add(Number(a.id));
      return true;
    case "git_merge_upstream":
      // ff-only 在桩里总是快进不了 —— 那条分岔决策的路才走得到
      if (a.mode === "ff-only") {
        throw {
          kind: "conflict",
          message: "存在冲突，请先解决",
          raw: "fatal: Not possible to fast-forward, aborting.",
        };
      }
      return null;
    case "git_outgoing":
      return ["feat(notary): 补公证订单字段", "fix(notary): 退款审核状态对不上"];
    default:
      return NOT_MINE;
  }
}
