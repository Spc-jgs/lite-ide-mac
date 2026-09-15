/**
 * 改动列表按目录分组（issue #33 ⑮）。纯函数，`tests/git-group.test.ts` 卡着。
 *
 * 平铺是「文件名 + 灰色目录」一行一个，二十个以内一眼扫得完；再多就是一屏
 * 灰字，同一个目录下改了八个文件要读八遍目录名。IDEA 的树形 / VS Code 的
 * tree view 都是为这个。这里做的是**压扁的树**（VS Code 的 compact folders）：
 * 每个目录一个头，底下列文件，不做多层嵌套 —— 改动通常散在几个目录里，
 * 每层都能折叠的那种树点三下才看得到文件。
 *
 * 根目录下的文件放最前面、不带头：它们没有「所属目录」可写，放最后又会
 * 让人以为是某个目录的尾巴。
 */
export interface DirGroup<T> {
  /** 相对仓库根的目录，根目录是空串 */
  dir: string;
  items: T[];
}

export function groupByDir<T extends { path: string }>(entries: T[]): DirGroup<T>[] {
  const by = new Map<string, T[]>();
  for (const e of entries) {
    const p = e.path.endsWith("/") ? e.path.slice(0, -1) : e.path;
    const i = p.lastIndexOf("/");
    const dir = i < 0 ? "" : p.slice(0, i);
    const list = by.get(dir);
    if (list) list.push(e);
    else by.set(dir, [e]);
  }
  const dirs = [...by.keys()].filter((d) => d !== "").sort();
  const out: DirGroup<T>[] = [];
  const root = by.get("");
  if (root) out.push({ dir: "", items: root });
  for (const d of dirs) out.push({ dir: d, items: by.get(d)! });
  return out;
}
