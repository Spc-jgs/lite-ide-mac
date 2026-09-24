/** 桩：文件系统：探路径、列目录（含单层目录链）、读写文本、编码、剪贴板、新建 / 改名 / 移动 / 废纸篓、监听。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, bump, CONTESTED, DIRS, existsInMock, FILES, GENERATED, nameOf, NOT_MINE, parentOf, sleep, stampOf } from "./data";

/** 桩里按文件名猜编码，用来在浏览器里把编码相关的界面路径走一遍 */
function encOf(path: string): string {
  if (path.includes("gbk")) return "GBK";
  if (path.includes("big5")) return "Big5";
  if (path.includes("sjis")) return "Shift_JIS";
  if (path.includes("bom")) return "UTF-8";
  return "UTF-8";
}

/** 桩里的假文件系统：路径 → 内容 */
/** 存过的文件用什么换行符（`write_text` 记、`read_text` 报），没存过按名字装（见 read_text） */
const EOLS: Record<string, string> = {};
/** 抄 `excludes::VCS_DIRS` + `JUNK_FILES`：版本库目录和系统杂物，树里不列、探链不算 */
const HIDDEN = new Set([".git", ".svn", ".hg", "CVS", ".DS_Store", "Thumbs.db"]);

/**
 * 抄 `fsservice::single_child_chain`：往下每层**只有一个条目、而且是目录**就继续。
 * `HIDDEN` 里的不算数，生成物 / 有争议的目录断开（它们要自己占一行才能压暗）。
 */
function mockChain(dir: string): string[] {
  const out: string[] = [];
  let cur = dir;
  while (out.length < 32) {
    const kids = (DIRS[cur] ?? []).filter(([n]) => !HIDDEN.has(n));
    if (kids.length !== 1 || !kids[0][1]) break;
    const name = kids[0][0];
    if (GENERATED.has(name) || CONTESTED.has(name)) break;
    cur = `${cur}/${name}`;
    out.push(name);
  }
  return out;
}

/**
 * 桩里的名字校验。**规则必须和 `fsservice::validate_name` 一条不差** ——
 * 分叉之后前端的错误提示就是在浏览器里对着一份假规则调的。
 */
function checkName(name: string): string | null {
  if (name.trim() === "") return "名字不能为空";
  if (name.includes("/")) return "名字里不能有 /";
  if (name.includes("\0")) return "名字里不能有空字符";
  if (name === "." || name === "..") return "不能叫 . 或 ..";
  const bytes = new TextEncoder().encode(name).length;
  if (bytes > 255) return `名字太长（上限 255 字节，这个 ${bytes} 字节）`;
  return null;
}

/** 把 old 前缀下的所有条目搬到 next 前缀 —— 改一个目录的名字要连子树一起搬 */
function movePrefix(old: string, next: string) {
  for (const [k, v] of Object.entries(FILES)) {
    if (k === old || k.startsWith(`${old}/`)) {
      FILES[next + k.slice(old.length)] = v;
      delete FILES[k];
    }
  }
  for (const [k, v] of Object.entries(DIRS)) {
    if (k === old || k.startsWith(`${old}/`)) {
      DIRS[next + k.slice(old.length)] = v;
      delete DIRS[k];
    }
  }
}

/** 把 p 及其子树整个抹掉 */
function dropSubtree(p: string) {
  for (const k of Object.keys(FILES)) {
    if (k === p || k.startsWith(`${p}/`)) delete FILES[k];
  }
  for (const k of Object.keys(DIRS)) {
    if (k === p || k.startsWith(`${p}/`)) delete DIRS[k];
  }
  const par = DIRS[parentOf(p)];
  if (par) DIRS[parentOf(p)] = par.filter(([n]) => n !== nameOf(p));
}

export async function fsCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    // 文件监听：浏览器里没有盘可监听。桩里改文件走 __mockEditFileOutside，
    // 那条路自己不发事件 —— 焦点刷新那条路在桩上照常验
    case "watch_root":
      return null;
    case "probe_path": {
      const path = String(a.path);
      if (DIRS[path]) {
        return { kind: "dir", mode: "edit", path, name: path.split("/").pop(), size: 0, reason: "" };
      }
      /*
       * **`.log` 不等于日志模式。** 真实现（`logengine::probe`）只看
       * 体积 / 行数 / 最长行 / 二进制，**从不看文件名** —— 一个 3KB 的
       * `app.log` 在真机上走的是编辑模式。
       *
       * 桩原来对任何 `.log` 都硬报「1GB、超过 32MB」，于是
       * 「小 .log 在编辑模式下能不能切回日志模式」这条路在浏览器里
       * 走不到，而那正是 c16f12d 修的那个 bug 的现场。
       * 现在的判据是「桩手里有没有这个文件的真内容」：
       * 有就按真大小走真判据，没有的才是那个演示用的 1GB 日志。
       */
      const known = FILES[path] !== undefined;
      const isLog = path.endsWith(".log") && !known;
      /*
       * 不认识的路径要**报错**，不能凭空造一个文件出来。
       *
       * 原来对任意路径都返回一个假文件，于是「打开一个不存在的文件」
       * 这条路在浏览器里根本走不到 —— 而真实现是直接 Err。会话恢复
       * 正好高度依赖这条路（上次开着的文件这次可能已经删了/换分支没了），
       * 桩不还原它，那部分逻辑就等于没测过。
       */
      if (!isLog && !known) {
        throw new Error(`读不到 ${path}：No such file or directory (os error 2)`);
      }
      return {
        kind: "file",
        mode: isLog ? "log" : "edit",
        path,
        name: path.split("/").pop(),
        size: isLog ? 1_073_741_885 : (FILES[path]?.length ?? 0),
        reason: isLog ? "文件超过 32MB" : "",
      };
    }
    case "list_dir": {
      const path = String(a.path);
      /*
       * `node_modules` **故意慢**：文件树展开目录的等待指示（折叠箭头换成环）150ms 之内不出，
       * 而桩 0ms 返回的话它在浏览器里一次都验不到 —— 同 `git_commit` 那条的道理。
       * 挑 node_modules 是因为真机上它就是最慢的那个（几万个条目）。
       */
      if (path.endsWith("/node_modules")) await sleep(900);
      // 排序规则抄 Rust 侧 list_dir：目录在前，同类按名称不区分大小写。
      // 桩里原来是按写死的顺序返回的 —— 新建一个文件之后它会吊在列表最后，
      // 而真实现会把它排到该在的位置，「新建完滚过去」那段交互就白验了
      const sorted = [...(DIRS[path] ?? [])].filter(([name]) => !HIDDEN.has(name)).sort(
        (x, y) =>
          Number(y[1]) - Number(x[1]) ||
          x[0].toLowerCase().localeCompare(y[0].toLowerCase()),
      );
      return sorted.map(([name, isDir]) => ({
        name,
        path: `${path}/${name}`,
        isDir,
        size: isDir ? 0 : (FILES[`${path}/${name}`]?.length ?? 0),
        // 判据抄 Rust 侧：只有**目录**才谈得上生成物目录。
        // 一个叫 build 的文件（shell 脚本）不算
        generated: isDir && GENERATED.has(name),
        contested: isDir && CONTESTED.has(name),
        chain: isDir && !GENERATED.has(name) && !CONTESTED.has(name) ? mockChain(`${path}/${name}`) : [],
      }));
    }
    case "detect_encoding":
      return encOf(String(a.path));
    case "list_encodings":
      return [
        ["UTF-8", "UTF-8"],
        ["GB18030", "GB18030（简体中文，GBK 的超集）"],
        ["GBK", "GBK（简体中文）"],
        ["Big5", "Big5（繁体中文）"],
        ["Shift_JIS", "Shift_JIS（日文）"],
        ["UTF-16LE", "UTF-16 小端"],
      ];
    case "read_text": {
      // 形状必须与 Rust 侧 TextDto 一致，否则桩就失去了验证价值
      const p = String(a.path);
      const enc = (a.label as string) || encOf(p);
      return {
        content: FILES[p] ?? "// 桩里没有这个文件\n",
        encoding: enc,
        bom: p.includes("bom"),
        // 桩里模拟「按 UTF-8 读一个 GBK 文件」的乱码情形：
        // 换成 GBK 重新打开就不再有损，正好把状态栏的告警路径走一遍
        lossy: p.includes("gbk") && enc.toLowerCase() === "utf-8",
        // 桩里没有真的 CRLF 文件：名字里带 crlf 的装成 CRLF，状态栏那格才有得看；
        // 存过的按存的那次算（issue #37 改换行符 → 保存 → 重开，这条路要在桩上走得通）
        eol: EOLS[p] ?? (p.includes("crlf") ? "CRLF" : "LF"),
      };
    }
    case "write_text": {
      const path = String(a.path);
      // 写到一个还不存在的路径（另存为）：目录列表也要多出这一项，否则文件树看不见它
      if (FILES[path] === undefined) {
        const parent = parentOf(path);
        if (!DIRS[parent]) throw new Error(`${parent} 不是目录`);
        DIRS[parent] = [...DIRS[parent], [nameOf(path), false]];
      }
      FILES[path] = String(a.content);
      // 真实现（`fsservice::eol`）：混用的统一成 LF 存，下次读出来就是 LF
      if (a.eol != null) EOLS[path] = a.eol === "mixed" ? "LF" : String(a.eol);
      return bump(path);
    }
    case "file_stamp":
      return stampOf(String(a.path));
    /*
     * 浏览器里没有 Finder。桩不能一律返回成功 ——「路径不在盘上就报错」
     * 是这条命令唯一有分支的行为，桩里抹平它，前端的错误处理就等于没测过。
     */
    case "reveal_in_finder": {
      const path = String(a.path);
      if (!DIRS[path] && FILES[path] === undefined) {
        throw new Error(`${path} 不在盘上了`);
      }
      console.info(`[mock] 在 Finder 中显示 ${path}`);
      return null;
    }
    /*
     * 新建 / 改名 / 废纸篓：桩里**保留每一条错误分支**。
     * 抹平它们，前端那几条错误提示就等于从没走到过 —— 而它们恰恰是
     * 这批功能里最该验的部分（撞名、非法名字、目标已存在）。
     */
    case "create_entry": {
      const dir = String(a.dir);
      const name = String(a.name);
      const isDir = Boolean(a.isDir);
      const bad = checkName(name);
      if (bad) throw new Error(bad);
      const path = `${dir}/${name}`;
      if (existsInMock(path)) throw new Error(`这里已经有一个叫「${name}」的了`);
      DIRS[dir] = [...(DIRS[dir] ?? []), [name, isDir]];
      if (isDir) DIRS[path] = [];
      else {
        FILES[path] = "";
        bump(path);
      }
      return path;
    }
    case "rename_entry": {
      const path = String(a.path);
      const name = String(a.name);
      // 校验顺序跟着 Rust 侧走：先看名字，再看源在不在
      const bad = checkName(name);
      if (bad) throw new Error(bad);
      if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
      const parent = parentOf(path);
      const to = `${parent}/${name}`;
      if (to === path) return to;
      if (existsInMock(to)) throw new Error(`这里已经有一个叫「${name}」的了`);
      const wasDir = DIRS[path] !== undefined;
      movePrefix(path, to);
      const old = nameOf(path);
      DIRS[parent] = (DIRS[parent] ?? []).map(
        ([n, d]) => (n === old ? [name, d] : [n, d]) as [string, boolean],
      );
      if (!wasDir) bump(to);
      return to;
    }
    case "move_entry": {
      // 判据跟着 Rust 侧：源在不在、目标是不是目录、同名、目录进自己、挪回原处
      const path = String(a.path);
      const dest = String(a.dest);
      if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
      if (DIRS[dest] === undefined) throw new Error(`${dest} 不是目录`);
      const name = nameOf(path);
      const to = `${dest}/${name}`;
      if (to === path) return to;
      const wasDir = DIRS[path] !== undefined;
      if (wasDir && (dest === path || dest.startsWith(`${path}/`))) throw new Error("不能把目录移到它自己里面");
      if (existsInMock(to)) throw new Error(`${dest} 里已经有一个叫「${name}」的了`);
      const parent = parentOf(path);
      movePrefix(path, to);
      DIRS[parent] = (DIRS[parent] ?? []).filter(([n]) => n !== name);
      DIRS[dest] = [...(DIRS[dest] ?? []), [name, wasDir] as [string, boolean]];
      if (!wasDir) bump(to);
      return to;
    }
    case "trash_entry": {
      const path = String(a.path);
      if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
      // 故意慢：废纸篓走的是 Finder（NSWorkspace），Finder 忙的时候真会等上一秒。
      // 弹窗里主动作按钮的 busy 态（`.btn.busy`）只在这条路上能在浏览器里看到
      await sleep(800);
      dropSubtree(path);
      console.info(`[mock] 移到废纸篓 ${path}`);
      return null;
    }
    case "read_clipboard":
      // 真实现是 Rust 侧 pbpaste（不弹 WKWebView 的「Paste」确认）。桩里给一段固定的，
      // 浏览器里的 navigator.clipboard 会要权限，调 UI 时不想每次都点
      return "/* 桩里的剪贴板 */";
    default:
      return NOT_MINE;
  }
}
