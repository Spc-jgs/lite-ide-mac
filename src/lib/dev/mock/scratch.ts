/** 桩：草稿：目录、新建、列表、丢弃空草稿、在草稿里搜索。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, bump, DIRS, existsInMock, FILES, grepMatcher, NOT_MINE, SCRATCH_DIR, stampOf } from "./data";
import { splitFrontmatter } from "../../state/frontmatter";

export async function scratchCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    /*
     * 草稿目录在浏览器里也得是个**真的目录**（`DIRS` 里有它），
     * 否则「打开草稿目录」把它当项目根打开时，文件树列出来是空的，
     * 而真实现里那儿至少有你刚建的那份草稿 —— 桩一分叉就开始骗人。
     */
    case "scratch_dir":
      return SCRATCH_DIR;
    case "create_scratch": {
      const stem = String(a.stem);
      if (!DIRS[SCRATCH_DIR]) DIRS[SCRATCH_DIR] = [];
      // 撞名加序号，跟 Rust 侧 fsservice::create_scratch 一样
      for (let n = 1; n <= 99; n++) {
        const name = n === 1 ? `${stem}.md` : `${stem}-${n}.md`;
        const path = `${SCRATCH_DIR}/${name}`;
        if (existsInMock(path)) continue;
        DIRS[SCRATCH_DIR] = [...DIRS[SCRATCH_DIR], [name, false]];
        // 锚点写成文件头，形状同 fsservice::Anchor::frontmatter：空字段不写，全空不写头
        const an = (a.anchor ?? null) as null | Record<string, string>;
        const kv = an ? (["project", "branch", "head", "at"] as const).filter((k) => an[k]).map((k) => `${k}: ${an[k]}\n`) : [];
        FILES[path] = kv.length ? `---\n${kv.join("")}---\n\n` : "";
        bump(path);
        return path;
      }
      throw new Error("同一分钟里已经有 99 份草稿了");
    }
    case "list_scratches": {
      const entries = (DIRS[SCRATCH_DIR] ?? [])
        .filter(([n, isDir]) => !isDir && n.endsWith(".md") && !n.startsWith("."))
        .map(([n]) => {
          const path = `${SCRATCH_DIR}/${n}`;
          const [anchor, body] = splitFrontmatter(FILES[path] ?? "");
          const firstLine =
            (FILES[path] ?? "")
              .slice(body)
              .split("\n")
              .map((l) => l.trim().replace(/^#+/, "").trim())
              .find((l) => l.length > 0) ?? "";
          return {
            name: n,
            path,
            mtimeMs: stampOf(path).mtimeMs,
            firstLine: [...firstLine].slice(0, 80).join(""),
            anchor,
          };
        })
        .sort((a, b) => {
          // 同 Rust 侧 scratch_sort_key：同一分钟的 `-2` 排在 `-1`（无后缀）上面
          const key = (n: string) => {
            const stem = n.replace(/\.md$/, "");
            const m = /^(.* .*)-(\d+)$/.exec(stem);
            return m ? ([m[1], Number(m[2])] as const) : ([stem, 1] as const);
          };
          const [ab, an] = key(a.name), [bb, bn] = key(b.name);
          return ab === bb ? bn - an : ab < bb ? 1 : -1;
        });
      return entries;
    }
    case "discard_empty_scratch": {
      const path = String(a.path);
      // 判据照着 Rust 侧抄一遍。桩里少一条，浏览器上就走得通而真机上走不通
      if (!path.startsWith(`${SCRATCH_DIR}/`)) throw new Error("不在草稿目录里");
      if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
      // 「空」= 正文空；带头没正文的也算（同 Rust 侧）
      if ((FILES[path] ?? "").slice(splitFrontmatter(FILES[path] ?? "")[1]).trim() !== "") throw new Error("这份草稿里有东西，不能这么丢");
      delete FILES[path];
      DIRS[SCRATCH_DIR] = (DIRS[SCRATCH_DIR] ?? []).filter(
        ([n]) => `${SCRATCH_DIR}/${n}` !== path,
      );
      return null;
    }
    case "grep_scratches": {
      // 只搜草稿目录；文件头（锚点）里的命中滤掉，同 Rust 侧
      const hit = grepMatcher(String(a.pattern));
      const out: Array<{ path: string; line: number; text: string }> = [];
      for (const [full, content] of Object.entries(FILES)) {
        if (!full.startsWith(`${SCRATCH_DIR}/`)) continue;
        const [anchor, off] = splitFrontmatter(content);
        const headLines = anchor ? content.slice(0, off).replace(/\n$/, "").split("\n").length : 0;
        content.split("\n").forEach((text, i) => {
          if (i + 1 > headLines && hit(text)) out.push({ path: full, line: i + 1, text });
        });
      }
      return out.slice(0, Number(a.limit) || 60);
    }
    default:
      return NOT_MINE;
  }
}
