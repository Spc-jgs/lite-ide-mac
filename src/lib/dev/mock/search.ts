/** 桩：搜索：被忽略的目录、项目文件索引、全文搜索。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, CONTESTED, FILES, GENERATED, GIT_IGNORED, grepMatcher, NOT_MINE } from "./data";

/**
 * 搜索（⌘P / ⇧⌘F）跳不跳这条路径。
 *
 * 桩原来**一个目录都不跳**，于是「node_modules 里的东西搜不搜得到」
 * 在浏览器里怎么试都是「搜得到」，和真实现正好相反。
 */
function searchSkips(full: string): boolean {
  const parts = full.replace(/^\/proj\//, "").split("/");
  let rel = "";
  for (const seg of parts.slice(0, -1)) {
    rel = rel ? `${rel}/${seg}` : seg;
    if (seg.startsWith(".")) return true;
    if (!GENERATED.has(seg)) continue;
    // 有争议的按 git 的答案；确定的直接跳
    if (!CONTESTED.has(seg) || GIT_IGNORED.has(rel)) return true;
  }
  return false;
}

export async function searchCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "ignored_dirs":
      /*
       * 桩里 git 的答案是写死的（`GIT_IGNORED`）：`dist/` 被忽略、
       * `build/` 不被忽略 —— 两个名字都在「有争议」那一档，
       * 而结果一个压暗一个不压暗。**这就是 issue #13 的全部意思。**
       *
       * 返回数组（不是 null）表示「问到了 git」。要试「不是 git 仓库」
       * 那条退路，把这里改成 return null。
       */
      return [...GIT_IGNORED];
    case "list_project_files": {
      const files = Object.keys(FILES)
        .filter((f) => !searchSkips(f))
        .map((f) => f.replace(/^\/proj\//, ""));
      // localStorage 里 `lite-ide.mock-truncated` = "1" 就说索引截断了，看界面怎么说这件事
      let truncated = false;
      try {
        truncated = localStorage.getItem("lite-ide.mock-truncated") === "1";
      } catch {
        /* 拿不到就当没截断 */
      }
      return { files, truncated };
    }
    case "grep_project": {
      const hit = grepMatcher(String(a.pattern));
      const out: Array<{ path: string; line: number; text: string }> = [];
      for (const [full, content] of Object.entries(FILES)) {
        // 只搜项目根底下的：真 rg 跑在 /proj 里，草稿目录那些它根本看不见
        if (!full.startsWith("/proj/") || searchSkips(full)) continue;
        const rel = full.replace(/^\/proj\//, "");
        content.split("\n").forEach((text, i) => {
          if (hit(text)) out.push({ path: rel, line: i + 1, text });
        });
      }
      return out.slice(0, Number(a.limit) || 60);
    }
    default:
      return NOT_MINE;
  }
}
