/**
 * 纯偏好的开关，存 localStorage。
 *
 * 这些是「怎么显示」的选择（缩略图开不开、文件树跟不跟着标签），不是现场 ——
 * 不进会话快照（那份是「上次开了什么」，两件事的生命周期不同：换项目快照换，
 * 偏好不换）。读失败（隐私模式、站点数据被清）就用默认值，写失败就算了，
 * 都不能把启动流程炸掉。
 */
export function readPref(key: string, dflt: boolean): boolean {
  try {
    const v = localStorage.getItem(`lite-ide.${key}`);
    return v === null ? dflt : v === "1";
  } catch {
    return dflt;
  }
}

export function writePref(key: string, v: boolean) {
  try {
    localStorage.setItem(`lite-ide.${key}`, v ? "1" : "0");
  } catch {
    /* 存不下就算了，下次开还是默认值 */
  }
}

/** 数字偏好（编辑器字号）。读回来不是有限数就用默认值 —— 手改过的 localStorage 什么都可能是 */
export function readNumPref(key: string, dflt: number): number {
  try {
    const v = Number(localStorage.getItem(`lite-ide.${key}`));
    return Number.isFinite(v) && v > 0 ? v : dflt;
  } catch {
    return dflt;
  }
}

export function writeNumPref(key: string, v: number) {
  try {
    localStorage.setItem(`lite-ide.${key}`, String(v));
  } catch {
    /* 同上 */
  }
}
