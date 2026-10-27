/**
 * 桩：设置（issue #44）。和 `src-tauri/src/commands/settings.rs` 一一对应。
 *
 * 合并规则（终端字体空 = 跟编辑器、字号 = 基础 + 偏移、夹在 9–28）照 Rust 的 `settings::effective` 抄一份 ——
 * 浏览器里没有 settings.json，基础值就是默认值；`__mockSettingsProblems` 钩子让状态测试 / 调界面时
 * 能造出「设置文件写错了」那种状态（真实现里要去改文件才看得到）。
 */
import { type A, FILES, NOT_MINE } from "./data";
import { mockEmit } from "./events";

const PATH = "/Users/you/Library/Application Support/com.liteide.app/settings.json";
const MIN = 9;
const MAX = 28;
const BASE = 13;

const ui = { minimap: true, treeCompact: true, treeFollow: true, gitGrouped: false, offset: 0 };
let fresh = true;

const clamp = (n: number) => Math.max(MIN, Math.min(MAX, n));

function view() {
  const family = "JetBrains Mono";
  const problems = (globalThis as { __mockSettingsProblems?: unknown[] }).__mockSettingsProblems ?? [];
  return {
    editorFontFamily: family,
    editorFontSize: clamp(BASE + ui.offset),
    editorFontBase: BASE,
    editorHighlightUsages: WRITTEN["editor.highlightUsages"] !== false,
    editorInlayHints: WRITTEN["editor.inlayHints"] !== false,
    editorStickyLines: WRITTEN["editor.stickyLines"] !== false,
    javaHotswapOnSave: WRITTEN["java.hotswapOnSave"] !== false,
    terminalFontFamily: family,
    terminalFontSize: BASE,
    terminalShell: "",
    minimap: ui.minimap,
    treeCompact: ui.treeCompact,
    treeFollow: ui.treeFollow,
    gitGrouped: ui.gitGrouped,
    problems,
    path: PATH,
  };
}

const MOCK_IDEA = [
  {
    name: "ShopApplication (dev)",
    profiles: "dev,local",
    vmOptions: "-Xmx512m '-Dmsg=hello world' -Dserver.port=8081",
    programArgs: "",
    env: [{ name: "DB_URL", value: "jdbc:mysql://localhost/shop" }],
    jdk: "",
    notes: ["IDEA 里选的 JDK「corretto-17」是 IDEA 自己起的名字，这里认不出，跟项目"],
  },
];

/** 和 Rust 的 `settings::DEFS` 一致（键、类型、默认值、次序）；说明文字简写 */
const SCHEMA = [
  { key: "editor.fontFamily", label: "代码字体", kind: "string", default: '"JetBrains Mono"', doc: "代码字体：编辑器、差异、日志视图", min: null, max: null },
  { key: "editor.fontSize", label: "代码字号", kind: "integer", default: "13", doc: "代码字号（9–28）", min: MIN, max: MAX },
  { key: "editor.highlightUsages", label: "光标停在名字上时，标出它在这个文件里的用法", kind: "boolean", default: "true", doc: "IDEA 的 Highlight usages。目前只有 Java", min: null, max: null },
  { key: "editor.inlayHints", label: "参数名提示", kind: "boolean", default: "true", doc: "传字面量的参数前面写参数名。目前只有 Java", min: null, max: null },
  { key: "editor.stickyLines", label: "滚动时把所在的类、方法钉在顶上", kind: "boolean", default: "true", doc: "所在的类、方法、if / for / try 那一行钉在编辑器顶上。最多 4 行", min: null, max: null },
  { key: "terminal.fontFamily", label: "终端字体", kind: "string", default: '""', doc: "终端字体，空 = 跟编辑器", min: null, max: null },
  { key: "terminal.fontSize", label: "终端字号", kind: "integer", default: "13", doc: "终端字号（9–28）", min: MIN, max: MAX },
  { key: "terminal.shell", label: "终端用的 shell", kind: "string", default: '""', doc: "终端用的 shell，空 = $SHELL", min: null, max: null },
  { key: "java.jdtlsHome", label: "jdtls 的位置", kind: "string", default: '""', doc: "Java 智能用的 jdtls，空 = 内置的那份", min: null, max: null },
  { key: "java.jdtlsMemory", label: "jdtls 最多用的内存（MB）", kind: "integer", default: "1024", doc: "jdtls 最多用多少内存（MB）", min: 512, max: 16384 },
  { key: "java.jdks", label: "手动添加的 JDK", kind: "array", default: "[]", doc: "手动添加的 JDK", min: null, max: null },
  { key: "java.hotswapOnSave", label: "调试时存盘就自动构建、热替换", kind: "boolean", default: "true", doc: "调试着的时候存了 .java 文件就自动构建、热替换", min: null, max: null },
  { key: "maven.useWrapper", label: "项目里有 mvnw 就用它", kind: "boolean", default: "true", doc: "项目里有 mvnw 就用它", min: null, max: null },
  { key: "maven.home", label: "Maven 安装目录", kind: "string", default: '""', doc: "Maven 的安装目录，空 = PATH 里的 mvn", min: null, max: null },
  { key: "maven.settingsFile", label: "settings.xml", kind: "string", default: '""', doc: "Maven 的 settings.xml，空 = ~/.m2/settings.xml", min: null, max: null },
  { key: "maven.localRepository", label: "本地仓库", kind: "string", default: '""', doc: "Maven 本地仓库，空 = settings.xml 里写的", min: null, max: null },
  { key: "maven.extraArgs", label: "每条 mvn 都带的参数", kind: "string", default: '""', doc: "每条 mvn 都带上的参数", min: null, max: null },
  { key: "maven.opts", label: "MAVEN_OPTS（Maven 自己的 JVM 参数）", kind: "string", default: '""', doc: "MAVEN_OPTS", min: null, max: null },
  { key: "update.check", label: "自动检查更新", kind: "boolean", default: "true", doc: "启动后和之后每 12 小时看一眼有没有新版", min: null, max: null },
];

/** 设置页写过的值（桩里没有 settings.json：写进这张表，读的时候盖在默认值上） */
const WRITTEN: Record<string, unknown> = {};
/** 按项目的设置（真实现在 project-settings.json / maven-prefs.json） */
type UserCfg = { name: string; kind: string; mainClass: string; module: string; goals: string; command: string; cwd: string; items: string[]; profiles: string; vmOptions: string; programArgs: string; env: { name: string; value: string }[]; jdk: string; host: string; port: number };
const PROJ: Record<string, { jdk: string; offline: boolean; skipTests: boolean; runs: Record<string, Record<string, unknown>>; configs: UserCfg[] }> = {};
const proj = (root: string) => (PROJ[root] ??= { jdk: "", offline: false, skipTests: false, runs: {}, configs: [] });

/** 你建的运行配置（真实现在 projectcfg 的 configs）：任务的桩（mock/tasks.ts）照着它列 */
export const mockUserConfigs = (root: string): UserCfg[] => proj(root).configs;
const MOCK_JDKS = [
  { version: 17, home: "/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home", manual: false },
  { version: 21, home: "/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home", manual: false },
];

function page(root: string | null) {
  const values: Record<string, unknown> = {};
  for (const d of SCHEMA) values[d.key] = d.key in WRITTEN ? WRITTEN[d.key] : JSON.parse(d.default);
  const manual = ((values["java.jdks"] as string[]) ?? []).map((home) => ({ version: 11, home, manual: true }));
  const p = root ? proj(root) : null;
  return {
    defs: SCHEMA,
    values,
    written: Object.keys(WRITTEN),
    path: PATH,
    broken: null,
    jdks: [...MOCK_JDKS, ...manual],
    badJdks: [],
    project: p && root ? { root, jdk: p.jdk, autoJdk: `pom 要 Java 17 → Java 17（${MOCK_JDKS[0].home}）`, offline: p.offline, skipTests: p.skipTests, wrapper: false, codeStyle: MOCK_STYLE } : null,
  };
}

const KEYS: Record<string, keyof typeof ui> = {
  "editor.minimap": "minimap",
  "tree.compact": "treeCompact",
  "tree.follow": "treeFollow",
  "git.grouped": "gitGrouped",
};

let KEYMAP: Record<string, string> = {};

/** 「此项目」里的代码风格（真实现从 IDEA 读，`codestyle.rs`）：桩给一份「IDE 的 Default 方案改过阈值」的，看得到来源和转不了的那句 */
const MOCK_STYLE = {
  origin: "IDEA 的「Default」方案（IntelliJIdea2026.1）",
  indent: 4,
  cont: 8,
  margin: 120,
  imports: "其余 · javax · java · static 其余",
  star: 999,
  staticStar: 999,
  notes: ["import 每两组之间都会空一行（Eclipse 的规矩），IDEA 里挨着的两组（比如 javax 和 java）这里中间多一行"],
};

export async function settingsCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "settings":
      return view();
    // 和 Rust 的 settings::DEFS 一致（键、类型、默认值）；说明文字简写
    case "settings_schema":
      return SCHEMA;
    case "set_ui_state": {
      const k = KEYS[String(a.key)];
      if (!k) throw new Error(`不认识的界面状态：${a.key}`);
      (ui as Record<string, unknown>)[k] = Boolean(a.value);
      fresh = false;
      return view();
    }
    case "step_font": {
      ui.offset = a.delta == null ? 0 : clamp(BASE + ui.offset + Number(a.delta)) - BASE;
      fresh = false;
      return view();
    }
    case "adopt_ui_state": {
      if (fresh) {
        fresh = false;
        if (a.minimap != null) ui.minimap = Boolean(a.minimap);
        if (a.treeCompact != null) ui.treeCompact = Boolean(a.treeCompact);
        if (a.treeFollow != null) ui.treeFollow = Boolean(a.treeFollow);
        if (a.gitGrouped != null) ui.gitGrouped = Boolean(a.gitGrouped);
        if (a.editorFont != null) ui.offset = clamp(Number(a.editorFont)) - BASE;
      }
      return view();
    }
    case "settings_page":
      return page((a.root as string | null) ?? null);
    case "settings_apply": {
      // 同 Rust：先全部校验，一个不对一个都不写
      const g = (a.global ?? {}) as Record<string, unknown>;
      for (const [k, v] of Object.entries(g)) if (v !== null && !SCHEMA.some((d) => d.key === k)) throw new Error(`未知的设置项：${k}`);
      for (const [k, v] of Object.entries(g)) if (v === null) delete WRITTEN[k];
      else WRITTEN[k] = v;
      const pr = a.project as { root: string; jdk: string; offline: boolean; skipTests: boolean } | null;
      if (pr) Object.assign(proj(pr.root), { jdk: pr.jdk, offline: pr.offline, skipTests: pr.skipTests });
      // 真实现：写进 settings.json → 文件监听重读 → 广播（settingsctl::reload）。桩没有文件，直接广播 —— 不然设置页里切的开关在页面上没反应
      if (Object.keys(g).length) setTimeout(() => mockEmit("settings-changed", view()), 50);
      return page((a.root as string | null) ?? null);
    }
    // 键位覆盖（docs/TAKEOVER.md R3）：真实现写 keymap.json、菜单换键、广播 keymap-changed；桩只记着、照样广播
    case "keymap_overrides":
      return { ...KEYMAP };
    case "keymap_set": {
      KEYMAP = { ...(a.overrides as Record<string, string>) };
      setTimeout(() => mockEmit("keymap-changed", { ...KEYMAP }), 20);
      return null;
    }
    // 录键时摘掉菜单的键：浏览器里没有原生菜单，什么都不用做
    case "keymap_record":
      return null;
    case "run_configs": {
      const root = String(a.root);
      const runs = proj(root).runs;
      const { tasks } = (await import("./tasks")).mockTaskList(root);
      const services = tasks
        .filter((t) => t.source === "spring")
        .map((t, i) => {
          const r = runs[t.name] ?? {};
          return {
            name: t.name,
            mainClass: /(\S+)$/.exec(t.command)?.[1] ?? t.name,
            module: t.module ?? "",
            profiles: (r.profiles as string) ?? "",
            vmOptions: (r.vmOptions as string) ?? "",
            programArgs: (r.programArgs as string) ?? "",
            env: (r.env as unknown[]) ?? [],
            jdk: (r.jdk as string) ?? "",
            // 真实现读项目的 .idea/workspace.xml（ideacfg.rs）；桩给第一个服务一份，看得到「从 IDEA 带过来」那一条
            idea: i === 0 ? MOCK_IDEA : [],
          };
        });
      // Maven 模块：有 pom.xml 的目录（桩不读 <modules>，演示数据里够用）
      const modules = Object.keys(FILES)
        // 根上那个单独加（下面 concat），这里不算 —— 算两遍是重复的键（each_key_duplicate，浏览器里撞见的）
        .filter((p) => p.startsWith(`${root}/`) && p.endsWith("/pom.xml") && p !== `${root}/pom.xml`)
        .map((p) => p.slice(root.length + 1, -"/pom.xml".length))
        .concat(FILES[`${root}/pom.xml`] !== undefined ? [""] : [])
        .sort();
      const others = tasks.filter((t) => t.source === "package" || t.source === "file").map((t) => ({ name: t.name, source: t.source, command: t.command }));
      return { services, jdks: MOCK_JDKS, ideaProblems: [], user: proj(root).configs.map((c) => ({ ...c })), others, modules, problems: [] };
    }
    case "run_config_user_set": {
      // 同 Rust 的 settingsctl::check_user，挑几条最常撞的
      const root = String(a.root);
      const old = String(a.old ?? "");
      const c = { ...(a.cfg as UserCfg), name: String((a.cfg as UserCfg).name ?? "").trim() };
      if (!c.name) throw new Error("名字不能空着");
      const { mockTaskList } = await import("./tasks");
      const taken = mockTaskList(root).tasks.some((t) => t.name === c.name && !(t.name === old && t.source === "user"));
      if (taken) throw new Error(`已存在名为「${c.name}」的运行配置`);
      if (c.kind === "application" && !/^[A-Za-z_$][\w$]*(\.[A-Za-z_$][\w$]*)*$/.test(c.mainClass.trim())) throw new Error("主类要写全名（com.demo.Tool）");
      if (c.kind === "maven" && !c.goals.trim()) throw new Error("目标要填（空格隔开，比如 clean package）");
      if (c.kind === "shell" && !c.command.trim()) throw new Error("命令不能空着");
      if (c.kind === "compound" && !c.items.length) throw new Error("组合配置至少要选一份");
      if (c.kind === "compound" && c.items.includes(c.name)) throw new Error("组合配置不能包含它自己");
      const list = proj(root).configs;
      const i = old ? list.findIndex((x) => x.name === old) : -1;
      if (i >= 0) list[i] = c;
      else list.push(c);
      if (old && old !== c.name) for (const x of list) x.items = x.items.map((it) => (it === old ? c.name : it));
      return null;
    }
    case "run_config_user_delete": {
      const p = proj(String(a.root));
      p.configs = p.configs.filter((x) => x.name !== String(a.name));
      for (const x of p.configs) x.items = x.items.filter((it) => it !== String(a.name));
      return null;
    }
    case "run_config_set": {
      const env = (a.env as { name: string }[]) ?? [];
      const bad = env.find((e) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(e.name));
      if (bad) throw new Error(`环境变量名「${bad.name}」不行：要字母或下划线开头，后面只有字母、数字、下划线`);
      proj(String(a.root)).runs[String(a.name)] = { profiles: a.profiles, vmOptions: a.vmOptions, programArgs: a.programArgs, env, jdk: a.jdk };
      return null;
    }
    case "open_settings":
      // 同 Rust：不存在才建，已经有了不碰
      if (!(PATH in FILES)) FILES[PATH] = '{\n  // lite-ide 的设置（浏览器桩里的模板只是示意）\n  // "editor.fontSize": 13,\n}\n';
      return PATH;
    default:
      return NOT_MINE;
  }
}
