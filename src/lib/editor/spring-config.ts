/**
 * Spring Boot 配置文件（`application*.yml` / `.properties`）的补全和悬停文档（docs/TAKEOVER.md G30）。跟着编辑器懒加载。
 *
 * 键从哪来：Rust 问 jdtls 要这个文件所在模块的 classpath，读各个 jar 的 `spring-configuration-metadata.json`（`spring_meta.rs`）——
 * 和 IDEA 读的是同一份，不起 Spring Tools 的语言服务器。第一次要等 jdtls 导入完（几秒到几十秒），之后按文件记住；
 * 等的时候补全照常是「文档里的词」那一份（`completeAnyWord`），不让人干等一个空框。
 *
 * - **写键**：YAML 里按缩进认出父键，只给「父键底下」的；插进去的是相对父键的整段（`hikari.maximum-pool-size: `）——
 *   Spring Boot 认 YAML 里带点的键，不用替人拆成几层缩进（拆的话要去合并已经有的那几层，错了比不拆糟）
 * - **写值**：元数据里给了可选值（`hints`）的列出来；布尔的给 true / false
 * - **悬停**：类型、默认值、说明；过时的说换成什么
 * 键名按 Spring 的宽松绑定比（`contextPath` 和 `context-path` 是一个）
 */
import { autocompletion, completeAnyWord, type Completion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { EditorView, hoverTooltip } from "@codemirror/view";
import { EditorState, type Extension } from "@codemirror/state";
import type { SpringProp } from "../ipc/commands";
import { springEnum, springProps } from "../ipc/java";
import { normKey, propsKeyAt, propsSpot, yamlKeyAt, yamlParents, yamlSpot } from "./spring-path";

const loaded = new Map<string, Promise<SpringProp[]>>();
/** 拿这个文件的键；出错了（不是 Maven / Gradle 项目、jdtls 起不来）就当没有，下次打开再试 */
function propsFor(root: string, path: string): Promise<SpringProp[]> {
  let p = loaded.get(path);
  if (!p) {
    p = springProps(root, path).catch(() => {
      loaded.delete(path);
      return [];
    });
    loaded.set(path, p);
  }
  return p;
}

const short = (ty: string) => ty.replace(/java\.(lang|util|time)\./g, "").replace(/(\w+\.)+(\w+)/g, "$2");

function info(p: SpringProp): string {
  const lines = [p.desc || "（没有说明）"];
  if (p.default) lines.push(`默认：${p.default}`);
  if (p.deprecated !== null) lines.push(p.deprecated ? `已过时，换成 ${p.deprecated}` : "已过时");
  return lines.join("\n");
}

/** 枚举类型的常量，按类型记住（同一个类型在几个文件里只问一次）；出错当没有，下次再问 */
const enums = new Map<string, Promise<string[]>>();
function enumOf(root: string, path: string, ty: string): Promise<string[]> {
  let p = enums.get(ty);
  if (!p) {
    p = springEnum(root, path, ty).catch(() => {
      enums.delete(ty);
      return [];
    });
    enums.set(ty, p);
  }
  return p;
}

/**
 * 写值时的候选：元数据给了可选值（`hints`）用它的；布尔给 true / false；类型是个类（不是 `java.*`、不带泛型）的，可能是枚举 ——
 * 问 Rust 读那个类文件拿常量（`spring.main.web-application-type` → NONE / SERVLET / REACTIVE，元数据里没写 hints，IDEA 也是读枚举类）。
 * Spring 认值时不分大小写、`-` `_` 也宽松，列的是常量原样（同 IDEA）
 */
async function valuesOf(root: string, path: string, p: SpringProp | undefined): Promise<{ value: string; desc: string }[]> {
  if (!p) return [];
  if (p.values.length) return p.values;
  if (/^(java\.lang\.)?Boolean$|^boolean$/.test(p.ty)) return [{ value: "true", desc: "" }, { value: "false", desc: "" }];
  if (/^[a-z][\w$]*(\.[\w$]+)+$/.test(p.ty) && !p.ty.startsWith("java.")) return (await enumOf(root, path, p.ty)).map((value) => ({ value, desc: "" }));
  return [];
}

function source(root: string, path: string, yaml: boolean) {
  return async (ctx: CompletionContext): Promise<CompletionResult | null> => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    const before = line.text.slice(0, ctx.pos - line.from);
    const spot = yaml ? yamlSpot(before) : propsSpot(before);
    if (!spot) return null;
    // 空行上只在手动唤起（⌃Space）时弹：不然每回车一次就弹一个几千条的框
    if (spot.kind === "key" && !spot.typed && !ctx.explicit) return null;
    const props = await propsFor(root, path);
    if (!props.length) return completeAnyWord(ctx);
    const lines = yaml ? ctx.state.doc.toString().split("\n") : [];
    const parents = yaml ? yamlParents(lines, line.number - 1) : [];
    const pre = parents.length ? `${parents.join(".")}.` : "";
    // 父键可以一行写好几段（`spring.datasource:`）：切掉的是「段」不是「行」
    const depth = pre.split(".").filter(Boolean).length;
    if (spot.kind === "key") {
      const np = normKey(pre);
      const options: Completion[] = props
        .filter((p) => normKey(p.name).startsWith(np) && p.name.length > pre.length)
        .map((p) => {
          // 父键是你写的（可能是驼峰），元数据里是短横线：按宽松绑定对齐之后，取父键后面那一段
          const rel = p.name.split(".").slice(depth).join(".");
          return {
            label: rel,
            detail: short(p.ty),
            info: info(p),
            apply: yaml ? `${rel}: ` : `${rel}=`,
            boost: p.deprecated !== null ? -50 : 0,
            type: p.deprecated !== null ? "deprecated" : "property",
          };
        });
      return { from: ctx.pos - spot.typed.length, options, validFor: /^[\w.\-[\]]*$/ };
    }
    const full = normKey(yaml ? `${pre}${spot.key}` : spot.key);
    const vals = await valuesOf(root, path, props.find((p) => normKey(p.name) === full));
    if (!vals.length) return null;
    return { from: ctx.pos - spot.typed.length, options: vals.map((v) => ({ label: v.value, info: v.desc || undefined, type: "enum" })), validFor: /^\S*$/ };
  };
}

function hover(root: string, path: string, yaml: boolean) {
  return hoverTooltip(async (view: EditorView, pos: number) => {
    const line = view.state.doc.lineAt(pos);
    const key = yaml ? yamlKeyAt(view.state.doc.toString().split("\n"), line.number - 1) : propsKeyAt(line.text);
    if (!key) return null;
    // 只在键那一段上悬停才说（值上悬停不说键的文档）
    const col = pos - line.from;
    const end = line.text.search(yaml ? /:/ : /[=:]/);
    if (end < 0 || col > end) return null;
    const p = (await propsFor(root, path)).find((x) => normKey(x.name) === normKey(key));
    if (!p) return null;
    return {
      pos: line.from + line.text.search(/\S/),
      end: line.from + end,
      above: true,
      create() {
        const dom = document.createElement("div");
        dom.className = "cm-spring-tip";
        const head = document.createElement("div");
        head.className = "h";
        head.textContent = `${p.name}  ·  ${short(p.ty) || "?"}`;
        const body = document.createElement("div");
        body.textContent = info(p);
        dom.append(head, body);
        return { dom };
      },
    };
  });
}

/** 悬停那一块：同 Java 文件的报错提示（`cm-diag-tip`）的面，字号小一档；说明可能好几行 */
const tipTheme = EditorView.theme({
  ".cm-spring-tip": { padding: "6px 10px", maxWidth: "520px", whiteSpace: "pre-wrap", fontSize: "var(--fs-sm)", lineHeight: "1.5" },
  ".cm-spring-tip .h": { fontFamily: "var(--code-font)", color: "var(--text)", marginBottom: "4px" },
});

/** 挂到 Spring Boot 的配置文件上：补全换成这一份（不另挂通用的，免得一次弹两套）、悬停文档 */
export function springConfig(root: string | null, path: string): Extension {
  if (!root) return [];
  const yaml = /\.ya?ml$/.test(path);
  void propsFor(root, path);
  return [
    autocompletion({ override: [source(root, path, yaml)] }),
    EditorState.languageData.of(() => [{ autocomplete: completeAnyWord }]),
    hover(root, path, yaml),
    tipTheme,
  ];
}
