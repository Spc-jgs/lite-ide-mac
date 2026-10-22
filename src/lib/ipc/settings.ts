/**
 * 设置对话框、运行配置对话框的命令包装（docs/SETTINGS.md 第 13 节）。单独一个文件、只有那两个对话框引：
 * 不放进 `ipc/app.ts`：那个文件在入口包里，封装要跟着调用方走（frontend.md）。（第一版入口 +362 B，起初以为是放错了文件，
 * 挪过来没变 —— 真原因是对话框里的 `value={…}` 用到了入口里没有的运行时函数、以及 `await import()` 了入口里的 notify，见 frontend.md 的账）
 */
import { invoke } from "@tauri-apps/api/core";
import type { EnvVar, RunConfigs, SettingsPage, UserConfig } from "./commands";
/** 设置对话框（docs/SETTINGS.md 第 13 节）。`root`：这个窗口的项目，没有就不给「此项目」那一栏 */
export const settingsPage = (root: string | null) => invoke<SettingsPage>("settings_page", { root });

/** 设置对话框的「应用」：`global` 里值是 null 的恢复默认；`project` 是「此项目」那一栏（没改是 null）；`root` 是这个窗口的项目（回来的整页按它画） */
export const settingsApply = (
  root: string | null,
  global: Record<string, unknown>,
  project: { root: string; jdk: string; offline: boolean; skipTests: boolean } | null,
) => invoke<SettingsPage>("settings_apply", { root, global, project });

/** 运行配置对话框：这个项目认出来的 Spring 服务和各自的配置 */
export const runConfigs = (root: string) => invoke<RunConfigs>("run_configs", { root });

/** 记下一个服务的运行配置（下次起它时生效） */
export const runConfigSet = (root: string, name: string, c: { profiles: string; vmOptions: string; programArgs: string; env: EnvVar[]; jdk: string }) =>
  invoke<void>("run_config_set", { root, name, ...c });

/** 存一份你建的运行配置（docs/TAKEOVER.md R1）：新建时 `old` 是空串，改名时是原来的名字 */
export const runConfigUserSet = (root: string, old: string, cfg: UserConfig) => invoke<void>("run_config_user_set", { root, old, cfg });

/** 删一份你建的运行配置 */
export const runConfigUserDelete = (root: string, name: string) => invoke<void>("run_config_user_delete", { root, name });
