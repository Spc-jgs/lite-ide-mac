/**
 * `.http` 文件的着色（IDEA 的 HTTP Client 格式，docs/IDEA.md 第 19 节）。
 *
 * 不用 `@codemirror/legacy-modes` 里的 `http`：那个认的是**一条原样的 HTTP 报文**（第一行必须是请求行或状态行），
 * `.http` 文件里的 `@变量 = 值`、`###`、注释在它眼里全是错，整行标红（浏览器里看出来的：第一行 `@base = …` 一片红，像报错）。
 * 这里按 `http/http-file.ts` 切请求的同一套规矩认：分隔行、注释、文件变量、请求行、头、空行之后是体；`{{变量}}` 在哪儿都单独着色。
 */
import type { StreamParser } from "@codemirror/language";

interface St {
  /** 在一个请求的哪一段：请求行之前 / 头 / 体 / 响应处理脚本里 */
  at: "pre" | "head" | "body" | "script";
}

const METHOD = /^(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS|TRACE|CONNECT)(?=\s)/;

/** `{{…}}`：变量（不管在 URL、头还是体里） */
function vars(stream: Parameters<StreamParser<St>["token"]>[0]): string | null {
  if (stream.match(/^\{\{[^{}]*\}\}/)) return "variableName.special";
  return null;
}

/** 吃到下一个 `{{` 或者行尾 */
function until(stream: Parameters<StreamParser<St>["token"]>[0]) {
  while (!stream.eol() && !stream.match(/^\{\{/, false)) stream.next();
}

export const httpFile: StreamParser<St> = {
  name: "http",
  startState: () => ({ at: "pre" }),
  blankLine(s) {
    if (s.at === "head") s.at = "body";
  },
  token(stream, s) {
    if (stream.sol()) {
      if (stream.match(/^###.*/)) {
        s.at = "pre";
        return "heading";
      }
      if (s.at === "script") {
        if (stream.match(/^.*%\}/)) s.at = "body";
        else stream.skipToEnd();
        return "meta";
      }
      if (s.at !== "body" && stream.match(/^\s*(#|\/\/).*/)) return "comment";
      if (s.at === "pre") {
        if (stream.match(/^@[A-Za-z_][\w.-]*/)) return "definition(variableName)";
        if (stream.match(METHOD)) {
          s.at = "head";
          return "keyword";
        }
        if (stream.match(/^(https?:\/\/|\{\{)/, false)) s.at = "head";
      }
      if (s.at === "head" && !stream.match(/^\s/, false)) {
        if (stream.match(/^[^:\s{]+(?=\s*:)/)) return "propertyName";
      }
      if (s.at === "body") {
        if (stream.match(/^>\s*\{%.*%\}/)) return "meta";
        if (stream.match(/^>\s*\{%/)) {
          s.at = "script";
          stream.skipToEnd();
          return "meta";
        }
        if (stream.match(/^(<>|<|>)\s+\S.*/)) return "meta";
      }
    }
    const v = vars(stream);
    if (v) return v;
    if (stream.eatSpace()) return null;
    // 请求行上方法之后的 URL、` HTTP/1.1`
    if (s.at === "head" && stream.match(/^HTTP\/[\d.]+\s*$/)) return "keyword";
    until(stream);
    return s.at === "head" ? "string" : s.at === "pre" ? "string" : null;
  },
};
