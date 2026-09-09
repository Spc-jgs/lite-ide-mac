/**
 * 把 catch 到的任何东西变成一段能给人看的话。
 *
 * # 为什么不能只 `String(e)`
 *
 * IPC 那层有几条命令 **reject 的是结构化对象而不是字符串** ——
 * `gitSwitch` 的 `SwitchErr`（`{kind, message, files, raw}`）就是，
 * `commands.ts` 里那句注释写得很清楚：
 *
 * > 调用方要 `catch` 之后判 `kind`，不能直接 `String(e)` 往界面上贴。
 *
 * 而 `switchBranch` 只消化了 `kind === "local-changes"` 这一档，
 * 其余（分支不存在、名字重了、被工作树占着）原样 `throw` 给 `gitDo`，
 * 最后落到 `notify.block` 上。`String({...})` 的结果是 **`[object Object]`** ——
 * 横幅上就这七个字，用户既不知道出了什么事，也不知道下一步该干嘛。
 *
 * 桩（`mock-ipc.ts`）扔的也是纯对象，所以浏览器里这条路一样坏。
 *
 * # 判据
 *
 * 按「哪个字段最像给人看的话」依次取：`message` → `raw` → `error`。
 * 都没有就退回 JSON —— 丑，但**至少信息还在**，比 `[object Object]` 强。
 */
export function errText(body: unknown): string {
  const strip = (s: string) => s.replace(/^Error:\s*/, "").trim();

  if (typeof body === "string") return strip(body);
  if (body instanceof Error) return strip(body.message);

  if (body && typeof body === "object") {
    const o = body as Record<string, unknown>;
    // `message` 在前：SwitchErr 那类里它是已经翻译过的一句话，
    // 而 `raw` 是 git 的英文原话 —— 两个都在的时候要前者
    for (const k of ["message", "raw", "error"]) {
      const v = o[k];
      if (typeof v === "string" && v.trim()) return strip(v);
    }
    try {
      // 循环引用会抛，那时只能认了走到最后一行
      return JSON.stringify(body);
    } catch {
      /* 掉到下面 */
    }
  }
  return strip(String(body));
}
