/**
 * 状态栏左边那条面包屑，以及标题栏上的项目名。纯函数，零 import。
 *
 * 从 App.svelte 里搬出来（issue #9 第 3 步）—— 原来是一段 `$derived.by`，
 * 逻辑没变，只是现在裸 node 就能测。
 */

/** 项目根的最后一段。没打开项目时是应用名 —— 标题栏挂件在两个状态下位置一致 */
export function projectName(root: string | null): string {
  return root ? root.slice(root.lastIndexOf("/") + 1) || root : "lite-ide";
}

export interface Crumb {
  name: string;
  path: string;
  /** 目录段可点（在文件树里定位），文件段不可点 */
  dir: boolean;
}

/**
 * 项目名 › 中间目录 › 文件名。
 *
 * 只对**在项目根底下**的路径展开；差异 / 合并标签的 path 是 `git-diff:xxx`
 * 这类合成 key，打开的应用日志也在项目外 —— 那些只给一段文件名。
 */
export function crumbsOf(root: string | null, path: string, name: string): Crumb[] {
  if (!root || !path.startsWith(`${root}/`)) return [{ name, path, dir: false }];
  const rel = path.slice(root.length + 1).split("/");
  const out: Crumb[] = [{ name: projectName(root), path: root, dir: true }];
  let acc = root;
  rel.forEach((seg, i) => {
    acc += `/${seg}`;
    out.push({ name: seg, path: acc, dir: i < rel.length - 1 });
  });
  return out;
}
