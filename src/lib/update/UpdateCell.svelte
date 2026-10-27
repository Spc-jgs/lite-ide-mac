<script lang="ts">
  /**
   * 状态栏「重启更新到 x.y.z」那一格（docs/TAKEOVER.md MVP-8）。只在新版下好、验过签名之后出现，常驻到点了为止，不弹框（D8 不打断）；
   * 查到一半、没有新版、出错都不占状态栏 —— 菜单「检查更新…」自己说结果。检查、下载、验签都在 Rust（`src-tauri/src/update.rs`）。
   * 懒加载（状态栏在入口里，这一格加进去入口涨了 900 B，离告警线只剩 177 B）：状态栏挂上后拉，Rust 那边最早也是启动 30 秒后才查
   */
  import { invoke } from "@tauri-apps/api/core";
  import { listenHere, type UpdateInfo } from "../ipc/commands";
  import { restartToUpdate } from "./restart";

  let ready = $state<UpdateInfo | null>(null);
  const take = (u: UpdateInfo) => (ready = u.phase === "ready" ? u : null);
  void invoke<UpdateInfo>("update_status").then(take, () => {});
  void listenHere<UpdateInfo>("update-status", (e) => take(e.payload)).catch(() => {});
</script>

{#if ready}
  {@const u = ready}
  <!-- 点了存好现场重启：重启照 ⌘Q 的路走（未保存的自动保存、会话恢复），所以不再问；有任务在跑才先说一句（restart.ts） -->
  <!-- 通用的 `.btn.sm.quiet` 就是状态栏一格的样子（24 高、小一号字、悬停有底）：状态栏的 `.cbtn` 是它自己的局部样式，懒加载的组件够不着 -->
  <button
    class="btn sm quiet upd"
    title={`现在是 ${u.current}${u.notes ? `\n\n${u.notes}` : ""}\n\n点一下：存好现场、换成新版、重启`}
    onclick={() => restartToUpdate(u.version)}
  >重启更新到 {u.version}</button>
  <span class="sep" aria-hidden="true"></span>
{/if}

<style>
  /* 强调色：这是状态栏上唯一一格「有件事等你点」（同「已修改」那格用 accent 的理由） */
  .upd { color: var(--accent); }
  /* 同状态栏的 `.vsep`（分区的竖线） */
  .sep { flex: none; width: 1px; height: 11px; background: var(--border); }
</style>
