/**
 * `@Value("${demo.greeting:hi}")` 上 ⌘B 跳到配置文件（docs/IDEA.md 第 25 节）：光标在不在一个 `${…}` 里、是哪个键。
 * 不带依赖，裸 node 里测（tests/spring-key.test.ts）。只看这一行：`${` 和 `}` 不会跨行写
 *
 * `${key:默认值}` 去掉默认值；`${a}-${b}` 按光标在哪一个算；嵌套的 `${a:${b}}` 取光标所在的最里面那个。
 * 光标在 `${` 外面（比如 `@Value` 上）不算 —— ⌘B 交回给原来的跳转
 */
export function configKeyAt(line: string, col: number): string | null {
  let best: { from: number; to: number } | null = null;
  for (let i = line.indexOf("${"); i >= 0; i = line.indexOf("${", i + 2)) {
    // 配对的 `}`：中间再有 `${` 的要跳过它的那一对
    let depth = 0;
    let end = -1;
    for (let j = i + 2; j < line.length; j++) {
      if (line.startsWith("${", j)) {
        depth++;
        j++;
      } else if (line[j] === "}") {
        if (depth === 0) {
          end = j;
          break;
        }
        depth--;
      }
    }
    if (end < 0) continue;
    // 光标在 `${` 的 `$` 上到 `}` 上都算；取最里面（最短）的那个
    if (col >= i && col <= end && (!best || end - i < best.to - best.from)) best = { from: i, to: end };
  }
  if (!best) return null;
  const inner = line.slice(best.from + 2, best.to);
  // 去掉默认值：第一个不在嵌套 `${…}` 里的 `:`
  let depth = 0;
  let cut = inner.length;
  for (let j = 0; j < inner.length; j++) {
    if (inner.startsWith("${", j)) depth++;
    else if (inner[j] === "}") depth--;
    else if (inner[j] === ":" && depth === 0) {
      cut = j;
      break;
    }
  }
  const key = inner.slice(0, cut).trim();
  return key && !key.includes("${") ? key : null;
}
