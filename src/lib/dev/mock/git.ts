/** 桩：Git：状态、差异、暂存、提交、历史、blame、stash、分支、工作树、仓库信任、控制台。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, FILES, NOT_MINE, sleep } from "./data";

/** 造一条分支，字段与 Rust 侧 BranchDto 对齐 */
function b(name: string, isHead: boolean, isRemote: boolean, upstream: string, subject: string) {
  return { name, sha: name.slice(0, 7), upstream, isHead, isRemote, when: "2 天前", subject };
}

/**
 * 一段带合并的假历史。刻意造出「分支岔出去 → 各自提交 → 合并回来」，
 * 泳道图的三种情形（直线、分叉、汇合）在浏览器里就能一眼看全。
 *
 * ⚠ **CI 拿这张表里的字符串当哨兵**（`.github/workflows/ci.yml` 的
 * 「开发桩不许进产物」那一步）。2026-09-07 之前这个模块真的漏进过生产入口包
 * 1,195 字节 —— 漏的就是下面这句 `.map()`：方法调用打包器证明不了没有副作用，
 * 于是连数据字面量一起留下了。改这几行之前先看那一步。
 */
const MOCK_LOG = [
  ["h8", "合并 M12：界面打磨与使用手册", ["h7", "f2"], ["HEAD", "m13/git"]],
  ["f2", "M12 界面打磨：侧边栏、终端字体、使用手册", ["f1"], []],
  ["f1", "终端字体改用具体字体名", ["h7"], []],
  ["h7", "合并 M11：符号大纲", ["h6", "e1"], ["main", "origin/main"]],
  ["e1", "M11 符号大纲：⌘⇧O 文件结构", ["h6"], ["m11/symbols"]],
  ["h6", "合并 M10：堆栈折叠", ["h5"], []],
  ["h5", "M10 堆栈折叠：复用过滤机制", ["h4"], []],
  ["h4", "M9 多终端标签", ["h3"], []],
  ["h3", "M8 语言与日志适配", ["h2"], []],
  ["h2", "M7 外部修改检测", ["h1"], []],
  ["h1", "M0 日志引擎垂直切片", [], []],
].map(([sha, subject, parents, refs], i) => ({
  sha: sha as string,
  short: (sha as string).padEnd(7, "0"),
  author: i % 3 === 0 ? "pc shao" : "李兆义",
  email: "dev@example.com",
  when: `${i + 1} 天前`,
  date: `2026-08-${String(26 - i).padStart(2, "0")}`,
  subject: subject as string,
  parents: parents as string[],
  refs: refs as string[],
}));

/** 造一条 git 状态条目，字段与 Rust 侧 GitEntryDto 严格对齐 */
function g(
  path: string,
  index: string,
  work: string,
  extra: { staged?: boolean; untracked?: boolean; conflicted?: boolean; orig?: string } = {},
) {
  const untracked = extra.untracked ?? false;
  const conflicted = extra.conflicted ?? false;
  return {
    path,
    index,
    work,
    untracked,
    conflicted,
    // 与 Rust 侧 Entry::staged / unstaged 完全一致 —— 包括「冲突条目
    // 既不算已暂存也不算未暂存」这条。桩要是和真实现分叉，它就没用了
    staged: !conflicted && !untracked && index !== "." && index !== " ",
    unstaged: !conflicted && (untracked || (work !== "." && work !== " ")),
    orig: extra.orig ?? null,
  };
}

/*
 * 桩里要有点文件和点目录。真实现 2026-09-06 起把它们列出来了
 * （`fsservice::list_dir` 不再有 `show_hidden`），桩里一个都没有的话，
 * 浏览器里的文件树和 `.app` 里长得不一样 —— 而改 UI 的主循环就在浏览器里。
 *
 * 生成物目录（`node_modules` / `target` / `dist` / `build` / `venv` /
 * `__pycache__` / `vendor`，名单在 `crates/excludes`）**故意不放**：
 * 真实现永远不列它们，桩里放了反而是假的。
 */
/**
 * 被「丢弃改动」丢掉的路径。桩里唯一一处**有状态**的地方。
 *
 * 加它是因为「切分支被本地改动挡住 → 丢弃 → 再切一次」这条路要在浏览器里
 * 走得通：全无状态的桩会让第二次切换撞上同一个拒绝，看着像修复没生效。
 * 而这条路恰恰是这一轮的主角。
 */
const discarded = new Set<string>();
/**
 * 标记为解决的冲突文件。真 git 里 `git add` 一个冲突文件就是「解决了」，
 * 状态从 UU 变成 M（已暂存）—— 桩不跟着变的话，浏览器里解决完冲突
 * 「提交」按钮永远是灰的，「提交并推送」那条路根本走不到。
 */
const resolved = new Set<string>();

/** 挡住切到 `m11/symbols` 的那两个文件。和 `git_status` 里的条目对得上 */
const BLOCKERS = ["src/App.svelte", "docs/old.md"];
/**
 * stash 桩（issue #33 ⑪）：push 把「挡路的那两个」当成收进去了（它们从改动列表
 * 消失，切分支也不再被挡），pop 放回来。只有已跟踪的算 —— 和真实现一样，
 * 未跟踪的留在原地。
 */
const stashes: { index: number; message: string }[] = [];
let stashed = new Set<string>();

/**
 * 当前分支。`git_switch` 成功之后要变 —— 否则切完了 `git_status` 还报老名字，
 * 而应用的成功提示说的正是**刷新后的实际分支**（它有意不复述「我请求切到哪儿」）。
 * 桩不跟着变，那句提示在浏览器里就永远是错的。
 */
let curBranch = "m13/git";
/**
 * 游离检出（`git_switch` 切到一个 sha）。真实现的 status 那时 `branch` 是 `(a1b2c3d)`、
 * `detached: true`；桩原来直接把分支名设成 sha（issue #39 顺手对齐）。切回分支就清掉
 */
let detachedAt: string | null = null;

/** 本地分支。可变 —— 删除 / 改名之后再拉列表要看得出变化 */
const LOCAL: { name: string; subject: string }[] = [
  { name: "main", subject: "M12 界面打磨" },
  { name: "m13/git", subject: "M13 Git 版本管理" },
  { name: "m11/symbols", subject: "M11 符号大纲" },
];

/** 本地分支的上游。切分支之后提示语里的「（跟踪 …）」要跟着走 */
const UPSTREAM: Record<string, string> = {
  main: "origin/main",
  "m13/git": "origin/m13/git",
  "m11/symbols": "",
};

/** 这一次会话里信任过的指纹（issue #24 的桩） */
const TRUSTED = new Set<string>();

export async function gitCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    /*
     * Git 控制台（issue #29）。桩里给几条**形状真实**的：
     * 一条成功的 status、一条失败的 push、一条被打过码的远程 URL。
     * 三条一起才能验到这一页的三件事 —— 加固参数看不看得见、
     * 失败那条红不红、凭据有没有漏。
     */
    case "git_console": {
      const 加固 = [
        "git", "-c", "core.fsmonitor=", "-c", "diff.external=",
        "-c", "protocol.ext.allow=never",
      ];
      const now = Date.now();
      // 真实现只给 `root` 那个仓库的（按路径段比）：桩里三条都在 /proj
      const root = String(a.root ?? "");
      const under = (cwd: string) => cwd === root || cwd.startsWith(`${root}/`);
      return [
        {
          ms: now - 800, cwd: "/proj",
          argv: [...加固, "push", "https://***@github.com/o/r.git", "main"],
          code: 1, durMs: 2140,
          err: "fatal: Authentication failed for 'https://github.com/o/r.git/'",
          errTruncated: false,
        },
        {
          ms: now - 4200, cwd: "/proj",
          argv: [...加固, "status", "--porcelain=v2", "-uall"],
          code: 0, durMs: 41, err: "", errTruncated: false,
        },
        {
          ms: now - 9000, cwd: "/proj",
          argv: [...加固, "rev-parse", "--show-toplevel"],
          code: 0, durMs: 12, err: "", errTruncated: false,
        },
      ].filter((r) => under(r.cwd));
    }
    case "clear_git_console":
      return null;
    // ── Git ──
    // 造一份含所有状态位的假仓库：改动 / 新增 / 删除 / 改名 / 未跟踪目录 /
    // 冲突都占一条，浏览器里就能把染色和分组全看一遍。
    case "git_root":
      return "/proj";
    case "git_status":
      return {
        root: "/proj",
        branch: detachedAt ? `(${detachedAt})` : curBranch,
        head: detachedAt ?? "h800000",
        upstream: detachedAt ? "" : (UPSTREAM[curBranch] ?? ""),
        ahead: detachedAt ? 0 : 2,
        behind: 0,
        detached: detachedAt !== null,
        unborn: false,
        truncated: false,
        /*
         * 整个未跟踪的目录**不在 entries 里** —— 真实现把它摊成里面的
         * 文件，目录名单独走这一路给文件树。桩要照着摊，否则在浏览器里
         * 改动列表长得和 .app 里不一样。
         */
        untrackedDirs: ["scratch/"],
        entries: [
          g("src/OrderService.java", "M", ".", { staged: true }),
          g("src/App.svelte", ".", "M"),
          g("README.md", "A", "."),
          g("docs/old.md", ".", "D"),
          g("src/renamed.ts", "R", ".", { orig: "src/before.ts" }),
          g("scratch/draft.md", ".", "?", { untracked: true }),
          g("scratch/tmp/notes.md", ".", "?", { untracked: true }),
          g("notes.txt", ".", "?", { untracked: true }),
          // 未跟踪的 Java 文件：新文件 diff 的真实形态（多行 + 有语法着色），
          // 用户 2026-09-22 截的那张丑图就是这种
          g("src/NewController.java", ".", "?", { untracked: true }),
          resolved.has("src/conflict.rs")
            ? g("src/conflict.rs", "M", ".", { staged: true })
            : g("src/conflict.rs", "U", "U", { conflicted: true }),
        ].filter((e) => !discarded.has(e.path) && !(stashed.has(e.path) && !e.untracked)),
      };
    case "git_apply_cached":
    case "git_apply_worktree": {
      // 桩里没有真的暂存区，差异也是写死的改不动：认下来、记一句，让按钮那条路走得通。
      // 记改动行数是给按行暂存看的（issue #38）：patch 里该只剩选中的那几行
      await sleep(80);
      const patch = String(a.patch);
      const n = patch.split("\n").filter((l) => /^[+-]/.test(l) && !/^(\+\+\+|---)/.test(l)).length;
      console.info(`[mock] git apply${cmd === "git_apply_cached" ? " --cached" : ""}${a.reverse ? " -R" : ""}：${n} 行改动\n${patch.slice(0, 200)}`);
      return null;
    }
    case "git_blame": {
      // 桩：每 5 行一段，三个作者轮着来，最后 2 行「未提交」
      const n = (FILES[`${a.root}/${a.path}`] ?? "").split("\n").length;
      const who = [["a1b2c3d", "李兆义", "M12 界面打磨"], ["e4f5a6b", "张三", "M11 符号大纲"], ["c7d8e9f", "王五", "M10 日志引擎"]];
      const hunks = [];
      for (let start = 1; start <= n; start += 5) {
        const k = Math.floor((start - 1) / 5) % who.length;
        const uncommitted = start + 5 > n && n > 6;
        hunks.push({
          sha: uncommitted ? "0".repeat(40) : who[k][0].padEnd(40, "0"),
          short: uncommitted ? "0000000" : who[k][0],
          author: uncommitted ? "Not Committed Yet" : who[k][1],
          time: uncommitted ? 0 : Math.floor(Date.now() / 1000) - 86400 * (k + 1) * 3,
          summary: uncommitted ? "" : who[k][2],
          start,
          count: Math.min(5, n - start + 1),
        });
      }
      await sleep(80);
      return { hunks, truncated: false };
    }
    case "git_stash_list":
      return stashes.map((x) => ({ ...x }));
    case "git_stash_push": {
      await sleep(120);
      const tracked = ["src/OrderService.java", "src/App.svelte", "README.md", "docs/old.md", "src/renamed.ts"]
        .filter((f) => !discarded.has(f) && !stashed.has(f));
      if (tracked.length === 0) throw "没有可以收进 stash 的改动（未跟踪的文件不算）";
      for (const f of tracked) stashed.add(f);
      stashes.unshift({ index: 0, message: `WIP on ${curBranch}: a1b2c3d 上一条提交` });
      stashes.forEach((x, i) => (x.index = i));
      return null;
    }
    case "git_stash_pop": {
      await sleep(120);
      if (stashes.length === 0) throw "No stash entries found.";
      stashes.shift();
      stashes.forEach((x, i) => (x.index = i));
      if (stashes.length === 0) stashed = new Set();
      return null;
    }
    case "git_head_text": {
      /*
       * HEAD 里那份 = 桩文件去掉第 3 行、再把第 5 行改一个字。这样一打开
       * 就能看到 add / mod 两种标记，删掉一行还能看到 del。README 在桩里是
       * 新增（A），HEAD 里没有 → null，正好走「没有基线就不标」那条路。
       */
      const p = `${a.root}/${a.path}`;
      if (String(a.path) === "README.md" || !(p in FILES)) return null;
      const lines = FILES[p].split("\n");
      if (lines.length > 5) {
        lines.splice(2, 1);
        lines[3] = lines[3] + " // HEAD 里是这样";
      }
      return { text: lines.join("\n"), truncated: false };
    }
    case "git_diff": {
      /*
       * 未跟踪文件（`--no-index` 那条路）：整份是新增的。**这个形态桩原来没有**，
       * 于是「新文件的 diff 左边空一整栏」这件事在浏览器里一次都看不见 ——
       * 用户拿真 .app 截了图才发现（2026-09-22）。桩要覆盖真实现的每一种形态，
       * 不只是最常见那种。
       */
      // 整个文件被删（`docs/old.md` 在 git_status 里是 D）：单边差异的另一半形态
      if (String(a.path) === "docs/old.md") {
        const body = ["# 旧的说明", "", "这份文档已经并进 README，删掉。", "", "- 迁移记录见 JOURNAL"];
        return {
          truncated: false,
          text: [
            `diff --git a/${a.path} b/${a.path}`,
            "deleted file mode 100644",
            "index 1111111..0000000",
            `--- a/${a.path}`,
            "+++ /dev/null",
            `@@ -1,${body.length} +0,0 @@`,
            ...body.map((l) => `-${l}`),
          ].join("\n"),
        };
      }
      if (a.untracked) {
        const body = (FILES[`${a.root}/${a.path}`] ?? "还没写内容\n").replace(/\n$/, "").split("\n");
        return {
          truncated: false,
          text: [
            `diff --git a/${a.path} b/${a.path}`,
            "new file mode 100644",
            "index 0000000..1111111",
            "--- /dev/null",
            `+++ b/${a.path}`,
            `@@ -0,0 +1,${body.length} @@`,
            ...body.map((l) => `+${l}`),
          ].join("\n"),
        };
      }
      /*
       * 两个 hunk 是有意的：
       *
       * - 第一个是「连续新增一段长 import」。双栏差异的两个已知问题
       *   （右列横向溢出、左边一大块连续斜纹）只在这种形态下才看得出来，
       *   而原来的桩只有一处两行的小改动，在浏览器里怎么看都是好的。
       * - 第二个是普通的行内小改动，保住原来那份覆盖。
       */
      return {
        truncated: false,
        text: `diff --git a/${a.path} b/${a.path}
index 1a2b3c4..5d6e7f8 100644
--- a/${a.path}
+++ b/${a.path}
@@ -14,6 +14,11 @@ import java.util.List;
 import java.util.List;
 import java.util.Optional;
+import com.etianqu.evaluation.client.EvaluationTemplateFeignClient;
+import com.etianqu.evaluation.client.dto.EvaluationTemplateQueryRequest;
+import com.etianqu.evaluation.client.dto.EvaluationTemplateDetailResponse;
+import com.etianqu.evaluation.common.exception.EvaluationServiceException;
+import com.etianqu.evaluation.common.constant.EvaluationTemplateConstants;
 import org.springframework.stereotype.Service;
 import org.springframework.beans.factory.annotation.Autowired;
@@ -42,7 +47,8 @@ public void persist(Order order) {
     var conn = pool.getConnection();
-    int timeout = 300;
+    int timeout = 5000;
     try {
-        repo.save(order);
+        repo.saveAndFlush(order);
+        metrics.record("order.persist", order.id());
     } finally {
         conn.close();
     }`,
      };
    }
    case "git_stage":
      // 暂存一个冲突文件 = 标记为解决（真 git 就是这么算的）
      for (const x of a.paths as string[]) resolved.add(x);
      return null;
    case "git_unstage":
      return null;
    case "git_discard":
      // 记下来 —— `git_status` 和 `git_switch` 都要看它，
      // 否则「丢弃挡路的改动之后再切一次」在浏览器里永远走不通
      for (const x of [...(a.paths as string[]), ...(a.untracked as string[])]) discarded.add(x);
      return null;
    case "git_commit":
      /*
       * **故意慢。** 真实现里 `git commit` 跑在阻塞池上，因为
       * pre-commit 钩子跑什么是仓库说了算 —— 跑一遍 eslint 三十秒
       * （rules/rust.md 那张表）。桩原来 0ms 返回，于是围着这件事
       * 建的两样东西在浏览器里**一次都验不到**：
       *
       * - 「慢操作才说话」那条 300ms 的线（`gitDo` 里的 tip 定时器）
       * - 「一次只允许一个写操作」那道守卫（issue #23）
       *
       * 1.2 秒够跨过 300ms 那条线、也够在它跑着的时候手动点一次拉取，
       * 又不至于让浏览器里调 UI 变难受。
       */
      await sleep(1200);
      return "[m13/git abc1234] 桩提交";

    // 造一段带合并的历史，泳道图的分叉与汇合都能看到
    case "git_log_entries":
      return MOCK_LOG;
    case "git_commit_files":
      return [
        g("src/OrderService.java", "M", "."),
        g("src/App.svelte", "A", "."),
        g("docs/gone.md", "D", "."),
      ];
    case "git_commit_diff":
      return {
        truncated: false,
        text: `diff --git a/${a.path || "src/OrderService.java"} b/${a.path || "src/OrderService.java"}
@@ -8,4 +8,5 @@
 public class OrderService {
-    private int retries = 3;
+    private int retries = 5;
+    private Duration backoff = Duration.ofMillis(800);
 }`,
      };
    case "git_commit_vs_worktree":
      // 「和本地比较」：那次提交到现在，比 commit_diff 多出后来的改动（多一块）
      return {
        truncated: false,
        text: `diff --git a/${a.path || "src/OrderService.java"} b/${a.path || "src/OrderService.java"}
@@ -8,4 +8,5 @@
 public class OrderService {
-    private int retries = 3;
+    private int retries = 5;
+    private Duration backoff = Duration.ofMillis(800);
 }
@@ -42,3 +43,4 @@
     var conn = pool.getConnection();
-    int timeout = 300;
+    int timeout = 5000;
+    // 本地还没提交的
     try {`,
      };
    case "git_cherry_pick": {
      /*
       * `e1`（M11 那条）一定撞冲突 —— 冲突那条路（报错、不回滚、改动列表出现冲突中）
       * 在桩上必须走得到，浏览器里没有真仓库。别的都成功，图不动（桩的提交图是写死的）
       */
      await sleep(200);
      const hit = MOCK_LOG.find((c) => c.sha === a.sha);
      if (a.sha === "e1") {
        throw new Error(
          "error: could not apply e100000... M11 符号大纲：⌘⇧O 文件结构\n" +
            "hint: After resolving the conflicts, mark them with\n" +
            'hint: "git add/rm <pathspec>", then run\nhint: "git cherry-pick --continue"',
        );
      }
      return `[${curBranch} ${hit?.short ?? a.sha}] ${hit?.subject ?? ""}`;
    }
    case "git_branches":
      return [
        /*
         * `isHead` 必须和 `git_status` 的 `branch` 是同一个分支。
         * 真实现里两者都来自 checkout 的那一个（`%(HEAD)` 只标它），
         * 而这里原来 status 说 m13/git、branches 却把 main 标成 HEAD ——
         * 于是分支面板的「当前」和 Git 栏的分支名各说各的。
         */
        ...LOCAL.map((l) => b(l.name, curBranch === l.name, false, UPSTREAM[l.name] ?? "", l.subject)),
        b("origin/main", false, true, "", "M12 界面打磨"),
        b("origin/dev", false, true, "", "开发主线"),
      ];
    case "git_branch_delete": {
      /*
       * `m11/symbols` 第一次删一定被「还有没合并的提交」拦下来 —— 那条
       * 「仍然删除」的出路在浏览器里得走得到。reject 的是对象，和 `BranchErrDto` 一致。
       */
      if (a.name === "m11/symbols" && !a.force) {
        throw {
          kind: "not-merged",
          message: "这条分支上还有没合并的提交",
          raw: `error: the branch '${a.name}' is not fully merged\nhint: If you are sure you want to delete it, run 'git branch -D ${a.name}'`,
        };
      }
      if (a.name === curBranch) {
        throw { kind: "other", message: `error: cannot delete branch '${a.name}' used by worktree at '/proj'`, raw: "" };
      }
      const i = LOCAL.findIndex((l) => l.name === a.name);
      if (i >= 0) LOCAL.splice(i, 1);
      return null;
    }
    /*
     * 仓库信任（issue #24）。桩里没有 .git/config 可扫，用一个开关模拟「这个仓库有
     * 可疑配置」：localStorage 里 `lite-ide.mock-trap` = "1" 就报两条可疑项，
     * 点信任之后（`git_trust_grant`）这一次会话就当信过了。
     */
    case "git_trust_scan": {
      let trap = false;
      try {
        trap = localStorage.getItem("lite-ide.mock-trap") === "1";
      } catch {}
      const fingerprint = "mock-fp-" + (trap ? "trap" : "clean");
      const suspects = trap
        ? [
            { key: "core.fsmonitor", value: "touch /tmp/pwned; false", origin: "file:.git/config" },
            { key: "filter.lfs.clean", value: "git-lfs clean -- %f", origin: "file:.git/config" },
          ]
        : [];
      return {
        root: String(a.root),
        trusted: suspects.length === 0 || TRUSTED.has(fingerprint),
        suspects,
        hooks: trap ? ["pre-commit"] : [],
        fingerprint,
      };
    }
    case "git_trust_grant":
      TRUSTED.add(String(a.fingerprint));
      return null;
    case "git_branch_rename": {
      if (LOCAL.some((l) => l.name === a.new)) throw `fatal: a branch named '${a.new}' already exists`;
      const l = LOCAL.find((l) => l.name === a.old);
      if (l) {
        l.name = String(a.new);
        if (UPSTREAM[String(a.old)] !== undefined) UPSTREAM[l.name] = UPSTREAM[String(a.old)];
      }
      if (curBranch === a.old) curBranch = String(a.new);
      return null;
    }
    case "git_switch": {
      /*
       * **切到 `m11/symbols` 一定失败，而且失败成「本地改动挡着」那一档。**
       *
       * 这条路在桩上必须走得到：真实现里它要工作区脏 + 两边改了同一个文件
       * 才触发，而浏览器里没有真仓库。挡不住的话，那条「去提交 / 丢弃这些
       * 改动 / 取消」的确认条一次都验不了 —— 而它恰恰是这一轮的主角。
       *
       * reject 的是**对象不是字符串**，和 Rust 侧的 `SwitchErrDto` 一致；
       * 桩要是抛个 Error，前端 `err.kind` 读出来是 undefined，
       * 就会走到「原样上抛」那条分支去，在浏览器里看着像功能没做。
       */
      const blocking = BLOCKERS.filter((f) => !discarded.has(f) && !stashed.has(f));
      if (a.name === "m11/symbols" && !a.create && blocking.length > 0) {
        throw {
          kind: "local-changes",
          message: `有 ${blocking.length} 个文件的本地改动挡着`,
          files: blocking,
          raw:
            "error: Your local changes to the following files would be overwritten by checkout:\n" +
            blocking.map((f) => `\t${f}`).join("\n") +
            "\nPlease commit your changes or stash them before you switch branches.\nAborting",
        };
      }
      const asked = String(a.name);
      // 切到一个提交（提交历史里「检出到此提交」）：真实现是游离检出，分支名变成 `(sha)`
      const hit = !a.create && MOCK_LOG.find((c) => c.sha === asked || c.short === asked);
      if (hit) {
        detachedAt = hit.short;
        return `HEAD is now at ${hit.short} ${hit.subject}`;
      }
      detachedAt = null;
      curBranch = asked.replace(/^origin\//, "");
      if (a.create && !LOCAL.some((l) => l.name === curBranch)) {
        LOCAL.push({ name: curBranch, subject: `从 ${a.from || "HEAD"} 分出` });
      }
      // 检出远程分支时真实现走 `switch --track`，**会给新分支设上上游**。
      // 桩不设的话，界面上那句「（跟踪 …）」在浏览器里永远不出现
      if (asked.startsWith("origin/")) UPSTREAM[curBranch] = asked;
      return `Switched to branch '${curBranch}'`;
    }
    case "git_worktrees":
      return [
        { path: "/proj", sha: "abc1234", branch: "m13/git", detached: false, bare: false, locked: false, current: true },
        { path: "/proj-hotfix", sha: "def5678", branch: "hotfix/urgent", detached: false, bare: false, locked: false, current: false },
      ];
    case "git_worktree_add":
      return `/proj-${a.branch || "new"}`;
    case "git_worktree_remove":
      return null;

    // ── 菜单栏 ──
    default:
      return NOT_MINE;
  }
}
