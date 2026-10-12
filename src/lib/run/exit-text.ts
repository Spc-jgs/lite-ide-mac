/**
 * 任务非零退出时状态栏那句话。退出码大多是各个程序自己定的，**只翻译 shell 定死的两个**（2026-10-12 走查）：
 * 走查时 `npm run dev` 报「退出了（退出码 127）」—— 很多人不知道 127 是「命令找不到」，更不知道该去哪查。
 * 不点名是哪个命令：退出事件里没有输出，拿命令的第一个词去猜（`npm run dev` → npm）会猜错 —— 找不到的可能是脚本里调的另一个
 */
export function exitText(name: string, code: number | null): string {
  const base = `「${name}」退出了`;
  if (code === 127) return `${base}：有命令没找到（127）—— 看输出里 command not found 后面那个，终端里敲得出它才行`;
  if (code === 126) return `${base}：命令跑不了（126）—— 多半是脚本没有执行权限，chmod +x`;
  return code !== null ? `${base}（退出码 ${code}）` : base;
}
