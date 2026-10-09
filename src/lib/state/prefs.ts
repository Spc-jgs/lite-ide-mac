/**
 * 存在 localStorage 里的小列表（现在只有「最近切过的分支」，按仓库记）。
 *
 * 原来缩略图、文件树紧凑 / 跟随、Git 分组、编辑器字号这 5 个开关也在这儿（`readPref` / `readNumPref`），
 * issue #44 收归 Rust（`ui-state.json`，`state/settings.svelte.ts`）：它们跨窗口共享，而 localStorage
 * 里各窗口各读各的，A 里关了 B 不知道。旧值升级时迁一次（`settings-view.ts` 的 `readLegacyPrefs`），旧键留着不删。
 *
 * 读失败（隐私模式、站点数据被清）就当空，写失败就算了，都不能把启动流程炸掉。
 */
/** 字符串列表偏好（最近切过的分支）。读回来不是数组就当空 */
export function readListPref(key: string): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(`lite-ide.${key}`) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function writeListPref(key: string, v: string[]) {
  try {
    localStorage.setItem(`lite-ide.${key}`, JSON.stringify(v));
  } catch {
    /* 同上 */
  }
}
