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
          message: "远程比你多了东西，推不上去",
          raw: "! [rejected]  dev -> dev (non-fast-forward)",
        };
      }
      return null;
    }
    case "git_cancel":
      mockCancelled.add(Number(a.id));
      return true;
    case "git_merge_upstream":
      // ff-only 在桩里总是快进不了 —— 那条分岔决策的路才走得到
      if (a.mode === "ff-only") {
        throw {
          kind: "conflict",
          message: "有冲突要先解决",
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
