/** 桩：应用自身：初始路径、调试与应用日志、预算、菜单栏（选文件夹 / 保存面板 / 最近 / 灰态）、外部链接、命令行工具。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, bump, FILES, NOT_MINE } from "./data";

/*
 * 应用自己的运行日志。桩里给它几行真的内容 —— 不给的话「打开应用日志」
 * 在浏览器里只能验到「有没有报错」，验不到「打开之后长什么样」，
 * 而后者才是这个功能的全部（它就是用日志模式打开一个文件）。
 */
const APP_LOG = "/Users/you/Library/Logs/com.liteide.app/app.log";

export async function appCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "initial_paths":
      return ["/proj"];
    // 浏览器里没有 Rust 侧的 LITE_IDE_DEBUG，那条内存统计链路整个不存在。
    // 落到 default 的 null 也能让前端不建定时器，但那是碰巧对 ——
    // 显式写出来，读桩的人才看得出这条命令被想过。
    case "diag_enabled":
      return false;
    case "app_log":
      // 桩里不落盘 —— 但要留个响，否则浏览器里「异常有没有被记下来」
      // 完全看不出来
      console.info(`[app_log/${a.level}] ${a.source}: ${a.msg}`);
      return null;
    case "boot_mark":
      return null;
    case "report_budget":
      /*
       * 桩里量不到 `boot` 和 `self`（那两个是 Rust 侧的 FFI），
       * 但**前端送过去的四个数照样要能看见** —— 这条通道在浏览器里
       * 断没断，只有打出来才知道。真实的那一行长这样：
       *   `INFO [budget] boot=412ms self=34MB tabs=3 terms=0 editors=1 nodes=4210 …`
       */
      console.info(
        `[budget] boot=?ms self=?MB tabs=${a.tabs} terms=${a.terms}` +
          ` editors=${a.editors} nodes=${a.nodes}`,
      );
      return null;
    // 浏览器里跑的永远不是 Cargo 编出来的壳，谈不上带不带 Web Inspector。
    // 报 false（正式版）是保守的那一档：误报成调试版会让人白重打一次包
    case "devtools_build":
      return false;
    case "app_log_path":
      return APP_LOG;
    case "clear_app_log":
      // 桩里也要真清 —— 只返回 null 的话，「清完界面刷不刷新」这条
      // 在浏览器里永远看着像对的
      FILES[APP_LOG] = "2026-09-10 09:30:00.000Z INFO [app] 日志已清空\n";
      bump(APP_LOG);
      return null;
    /*
     * 草稿列表：照 Rust 侧的规矩 —— 只要 `.md`、按名字倒序、摘要取第一行有字的
     * （跳空行和 `#`）、截 80 字符。桩里少一条，浏览器上的列表就和真机不一样。
     */
    // 浏览器里装不了命令；给一个「软链没装上」的结果，那条带 sudo 的提示才验得到
    case "install_cli":
      return {
        script: "/Users/you/Library/Application Support/com.liteide.app/bin/lite",
        linked: false,
        replaced: false,
        linkCmd: 'sudo ln -sf "/Users/you/Library/Application Support/com.liteide.app/bin/lite" /usr/local/bin/lite',
      };
    /*
     * 浏览器里没有原生面板，也没有菜单栏 —— 这三条**必须有桩**，
     * 不能落到 default 去。
     *
     * 落到 default 返回 null 的话，`pickFolder()` 拿到 null 看着
     * 就像"用户取消了"，于是「打开文件夹」这个按钮在浏览器里
     * 点了永远没反应，而**没有任何报错** —— 排查时会先怀疑按钮没绑上。
     * 返回一个假目录，至少那条路是通的。
     */
    case "pick_folder":
      return "/proj";
    // 保存面板：浏览器里用 prompt 顶一下 —— 至少能把「另存为」那条路走通
    case "pick_save_path": {
      const dir = a.dir ? String(a.dir) : "/proj";
      const v = window.prompt("另存为（桩）：完整路径", `${dir}/${String(a.name)}`);
      return v && v.trim() ? v.trim() : null;
    }
    // 菜单在浏览器里不存在，这两条是空操作 —— 但要显式写出来
    case "set_recent":
    case "sync_menu_state":
      return null;
    // ── 拉取与推送 ──

    case "open_external": {
      // 真实现只放行 http / https（2026-09-23 为终端链接放开了 http），桩也照做：不然浏览器里试不出那条约束
      const url = String(a.url ?? "");
      if (!url.startsWith("https://") && !url.startsWith("http://")) throw new Error(`只允许 http / https 链接，实得：${url}`);
      console.info(`[mock] open_external ${url}`);
      return null;
    }
    default:
      return NOT_MINE;
  }
}
