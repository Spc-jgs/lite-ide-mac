/**
 * 打开设置对话框、运行配置对话框（docs/SETTINGS.md 第 13 节）。菜单动作（`menu-actions`）和 Services 都从这儿进，本身是懒块 ——
 * 入口包里不多一个字节（`menu-actions` 本来就是懒的，Services 也是）
 */
import { openModal } from "./modal-host";
import SettingsDialog from "./SettingsDialog.svelte";
import RunConfigDialog from "./RunConfigDialog.svelte";
import BreakpointsDialog from "../debug/BreakpointsDialog.svelte";

/** 一次只开一个：⌘, 按两下不叠两层。关掉（不管点的是取消、确定还是 Esc）时清掉 */
let open = false;

/** ⌘,：`root` 是这个窗口的项目，没打开项目就没有「此项目」那一类 */
export function openSettingsDialog(root: string | null) {
  if (open) return;
  open = true;
  openModal(SettingsDialog, { root }, () => (open = false));
}

/** 「运行 › 编辑配置…」、Services 的右键：`pick` 是先选中的那个服务 */
export function openRunConfigs(root: string, pick = "") {
  if (open) return;
  open = true;
  openModal(RunConfigDialog, { root, pick }, () => (open = false));
}

/** 「运行 › 查看断点…」（⇧⌘F8，docs/TAKEOVER.md G14）：这个项目的断点和异常断点 */
export function openBreakpoints(root: string) {
  if (open) return;
  open = true;
  openModal(BreakpointsDialog, { root }, () => (open = false));
}
