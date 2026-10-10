/**
 * 桩：任务（#48），对应 `src-tauri/src/commands/tasks.rs`（规则在 `src-tauri/src/taskdefs.rs`）。
 *
 * 照真实现的规则走，从 `FILES` 里读而不是写死一份列表：`.lite-ide/tasks.json`（注释、末尾逗号照样认）在前，
 * 然后根目录和一层子目录的 package.json scripts（文件里的顺序；生命周期钩子不列；`packageManager` 声明 → 锁文件 → npm），
 * 同名的以 tasks.json 为准。不模拟的：「cwd 跑出项目」那几条校验的措辞（只报一句「跳过了」）；建出来的 tasks.json 不进文件树的目录清单。
 */
import { type A, bump, FILES, NOT_MINE } from "./data";

const FILE = ".lite-ide/tasks.json";
const TEMPLATE = `// lite-ide 的任务（docs/USAGE.md「任务」）。⌃⌥R 选一个跑，⌃R 再跑上一个，⌘F2 停。
// 每个任务：name（列表里显示的名字）、command（一整行，交给你的登录 shell，和在终端里敲一样）、
// cwd（相对项目根，可以不写）、env（额外的环境变量，可以不写）。
// package.json 里的 scripts 不用写在这儿，会自动列出来；同名的以这里为准。
[
  // { "name": "后端", "command": "mvn spring-boot:run", "cwd": "admin" },
  // { "name": "前端", "command": "pnpm dev", "cwd": "web" },
  // { "name": "脚本", "command": "python3 main.py", "env": { "APP_ENV": "dev" } },
]
`;
const LIFECYCLE = new Set(["preinstall", "install", "postinstall", "prepublish", "preprepare", "prepare", "postprepare", "prepublishOnly", "prepack", "postpack", "publish", "postpublish", "preversion", "version", "postversion", "dependencies"]);

type Def = { name: string; command: string; cwd: string; source: "file" | "package" };

/** 和 Rust 的 settings::strip 同一件事的简化版：去掉 // 注释和末尾逗号（不管字符串里的 //，桩里的数据没有那种） */
const jsonc = (t: string) => JSON.parse(t.replace(/\/\/[^\n]*/g, "").replace(/,(\s*[\]}])/g, "$1") || "[]");

const word = (s: string) => (/^[A-Za-z0-9_\-:./@+=,%]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`);

function pmOf(dir: string, root: string, declared?: string): string {
  const d = declared?.split("@")[0];
  if (d && ["pnpm", "yarn", "npm", "bun"].includes(d)) return d;
  for (let cur = dir; ; cur = cur.slice(0, cur.lastIndexOf("/"))) {
    for (const [lock, pm] of [["pnpm-lock.yaml", "pnpm"], ["yarn.lock", "yarn"], ["bun.lock", "bun"], ["bun.lockb", "bun"], ["package-lock.json", "npm"]]) {
      if (`${cur}/${lock}` in FILES) return pm;
    }
    if (cur === root || !cur.includes("/")) return "npm";
  }
}

function list(root: string) {
  const defs: Def[] = [];
  const problems: string[] = [];
  const file = `${root}/${FILE}` in FILES;
  if (file) {
    try {
      const arr = jsonc(FILES[`${root}/${FILE}`]);
      if (!Array.isArray(arr)) problems.push(`${FILE} 最外层要是 [ … ]（任务的列表）`);
      else
        arr.forEach((t: { name?: string; command?: string; cwd?: string }, i: number) => {
          if (!t?.name?.trim() || !t?.command?.trim()) return void problems.push(`${FILE} 第 ${i + 1} 个任务缺 name 或 command（都要是非空的字符串），跳过了`);
          if (defs.some((d) => d.name === t.name)) return void problems.push(`${FILE} 里的「${t.name}」 重名了，用的是前面那个`);
          defs.push({ name: t.name.trim(), command: t.command.trim(), cwd: (t.cwd ?? "").replace(/^\.\/|\/$/g, ""), source: "file" });
        });
    } catch (e) {
      problems.push(`${FILE} 写错了：${(e as Error).message}`);
    }
  }
  const pkgs = Object.keys(FILES)
    .filter((p) => p === `${root}/package.json` || new RegExp(`^${root}/[^/.][^/]*/package\\.json$`).test(p))
    .filter((p) => !p.includes("/node_modules/"))
    .sort((a, b) => (a === `${root}/package.json` ? -1 : b === `${root}/package.json` ? 1 : a.localeCompare(b)));
  for (const p of pkgs) {
    const dir = p.slice(0, -"/package.json".length);
    const rel = dir === root ? "" : dir.slice(root.length + 1);
    let pj: { scripts?: Record<string, unknown>; packageManager?: string };
    try {
      pj = JSON.parse(FILES[p]);
    } catch (e) {
      problems.push(`${rel ? `${rel}/` : ""}package.json 解析不了：${(e as Error).message}`);
      continue;
    }
    // JS 对象按插入顺序遍历（非数字键）—— 和 Rust 那边特意保住的文件顺序一致
    const names = Object.keys(pj.scripts ?? {}).filter((k) => typeof pj.scripts![k] === "string");
    const hook = (k: string) => ["pre", "post"].some((pre) => k.startsWith(pre) && names.includes(k.slice(pre.length)));
    const pm = pmOf(dir, root, pj.packageManager);
    for (const s of names.filter((k) => !LIFECYCLE.has(k) && !hook(k))) {
      const name = rel ? `${rel}/${s}` : s;
      if (!defs.some((d) => d.name === name)) defs.push({ name, command: `${pm} run ${word(s)}`, cwd: rel, source: "package" });
    }
  }
  return { tasks: defs, file, problems };
}

export async function tasksCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "task_list":
      return list(String(a.root));
    case "task_new_file": {
      const p = `${String(a.root)}/${FILE}`;
      // 同真实现：已经有了就不动（create_new）
      if (!(p in FILES)) {
        FILES[p] = TEMPLATE;
        bump(p);
      }
      return p;
    }
    default:
      return NOT_MINE;
  }
}
