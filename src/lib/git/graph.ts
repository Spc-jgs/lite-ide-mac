/**
 * 提交泳道图布局。
 *
 * git 只给我们「每条提交的父是谁」，图是要自己排的。经典做法是维护一组
 * **泳道**（lane），每条泳道记着「我下一个在等哪条 sha」：
 *
 * - 一条提交出现时，认领那条正在等它的泳道；没人等就新开一条
 * - 它的第一个父继续占用同一条泳道 —— 这让主线保持一条直线
 * - 其余的父（合并提交才有）各自另开泳道，画成从这个点岔出去
 * - 若有多条泳道同时在等这条 sha，说明几条分支在这里汇合，多余的泳道收回
 *
 * **输入必须是拓扑序**（`git log --topo-order`）—— 子提交一定排在父提交前面。
 * 默认的提交时间序不保证这一点：两条提交时间戳相同时，父可能排在子前面，
 * 那时「认领正在等我的泳道」就落空，主线会断掉跳到别的泳道去。
 *
 * 有一个刻意的取舍：**泳道位置永不重排**。空出来的槽位留着给后来的提交复用，
 * 而不是把右边的泳道整体左移。重排会让线在视觉上横向漂移，看着像发生了
 * 并不存在的分支切换 —— 稳定的位置比紧凑的宽度重要。
 */

export interface Commitish {
  sha: string;
  parents: string[];
}

export interface GraphRow {
  /** 本提交所在泳道 */
  lane: number;
  /** 从上方下来、在本行汇入本提交的泳道（分支合并进来） */
  ins: number[];
  /** 从本提交往下走的泳道；含自己那条（第一父）和岔出去的（其余父） */
  outs: number[];
  /** 本行直穿而过、与本提交无关的泳道 */
  through: number[];
}

export interface Graph {
  rows: GraphRow[];
  /** 用到的最大泳道数，决定图区宽度 */
  width: number;
}

export function layout(commits: Commitish[]): Graph {
  /** 每条泳道正在等的 sha；null 表示空槽 */
  const lanes: (string | null)[] = [];
  const rows: GraphRow[] = [];
  let width = 0;

  const firstFree = () => {
    const i = lanes.indexOf(null);
    if (i >= 0) return i;
    lanes.push(null);
    return lanes.length - 1;
  };

  for (const c of commits) {
    // 1. 认领泳道
    let lane = lanes.indexOf(c.sha);
    if (lane < 0) {
      lane = firstFree();
      lanes[lane] = c.sha;
    }

    // 2. 别的泳道也在等这条 sha —— 它们在这里汇进来
    const ins: number[] = [];
    for (let j = 0; j < lanes.length; j++) {
      if (j !== lane && lanes[j] === c.sha) {
        ins.push(j);
        lanes[j] = null;
      }
    }

    // 3. 本行有哪些泳道是"路过"的（此刻还没动父提交，状态最准）
    const through: number[] = [];
    for (let j = 0; j < lanes.length; j++) {
      if (j !== lane && lanes[j] !== null) through.push(j);
    }

    // 4. 父提交占位
    const outs: number[] = [];
    const [first, ...rest] = c.parents;
    lanes[lane] = first ?? null;
    if (first) outs.push(lane);
    for (const p of rest) {
      // 已经有泳道在等这个父就并过去，别重复开
      let k = lanes.indexOf(p);
      if (k < 0) {
        k = firstFree();
        lanes[k] = p;
      }
      if (!outs.includes(k)) outs.push(k);
    }

    rows.push({ lane, ins, outs, through });
    width = Math.max(width, lanes.length);
  }

  return { rows, width: Math.max(1, width) };
}

/**
 * 泳道配色。同一条泳道始终同色，眼睛才能顺着一条线走下去。
 * 取的是 IDEA 日志里那套克制的中彩度，不跟错误红/警告黄撞。
 */
export const LANE_COLORS = [
  "#4f9ee3",
  "#63b76c",
  "#d6ae58",
  "#b389d6",
  "#57b6b6",
  "#e08f5a",
  "#9aa7b0",
];

export const laneColor = (i: number) => LANE_COLORS[i % LANE_COLORS.length];
