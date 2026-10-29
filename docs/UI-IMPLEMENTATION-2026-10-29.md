# 工作台 UI 交互修复 · 2026-10-29

依据用户认可的 [17 项审查](UI-AUDIT-2026-10-29.md) 实施。保留 IDEA 图标、系统字体、颜色与现有控件尺度；没有新增依赖、修改窗口配置或图标。原有资源治理、任务日志、构建入口和侧栏改动均保留，不计作这轮新增成果。没有提交或推送。

## 行为变化

| 审查项 | 本轮实现 | 主要位置 |
|---|---|---|
| P1-01、02 | Git 日志和提交文件的异步结果均受 effect 生命周期约束；切换对象立即清空旧详情。读取失败显示原因与重试，不显示“零文件”的成功结论 | `git/GitLog.svelte` |
| P1-03 | 确认层挂在窗口层，压暗范围与模态拦截一致；正文可滚，按钮不随编辑岛裁剪 | `shell/Confirms.svelte`、`dialog-surface.ts` |
| P1-04、P2-05 | 设置和运行配置共用 Tab / Shift+Tab 循环、初始焦点、失焦恢复、关闭归还焦点；背景 inert，声明 aria-modal。按钮型确认保留安全默认行为 | `settings/modal-host.ts`、`shell/dialog-surface.ts` |
| P1-05 | Git 按工具窗实际宽度退让作者与时间列；窄区域使用列表 / 详情单栏导航，返回后焦点回到原提交 | `git/GitLog.svelte` |
| P1-06 | 标题栏为项目、分支和运行挂件分配整体宽度；优先保住操作按钮，长名字省略 | `shell/TitleBar.svelte`、`run/RunWidget.svelte` |
| P2-01、02 | Git 列表 / 详情与说明 / 文件区可调；Services、运行结果树、调试调用栈可调。约束当前可用空间，窄窗不覆盖已保存尺寸 | `shell/Splitter.svelte`、`run/*View.svelte`、`DebugPane.svelte` |
| P2-03 | 分隔条支持鼠标、方向键、Shift 大步调整、Home / End、双击默认尺寸及右键增减；取消、捕获丢失、窗口失焦和卸载均清理拖动状态 | `shell/Splitter.svelte` |
| P2-04 | 工具窗按类型记住正常高度；最大化 / 恢复不改正常尺寸；头部单个更多菜单收齐工具操作与布局。视图菜单加入恢复默认布局 | `state/layout.svelte.ts`、`state/session.ts`、`shell/Panel.svelte`、原生菜单 |
| P2-06 | 提交保存后表单忙碌、取消禁用，Esc 不关闭；防止重复提交，避免“取消了却仍保存”的歧义 | 两个设置表单 |
| P2-07 | 保留无效输入文本；确定时回到首个错误的分类、滚动并聚焦字段；字段关联错误说明 | `settings/SettingsDialog.svelte` |
| P2-08 | 导轨按需读取生效键位；运行挂件随键位覆盖更新，不复制默认组合 | `shell/key-hint.ts`、`Rail.svelte`、`run/RunWidget.svelte` |
| P2-09 | 文件树独立开关使用 menuitemcheckbox；互斥选项保留 menuitemradio | `shell/ContextMenu.svelte`、`FileTree.svelte` |
| P2-10 | 原生构建菜单按窗口保存 canBuild，与工具栏使用同一探测结果；无项目或不支持构建时灰显 | `menu.rs`、`windows.rs`、IPC 两侧与桩 |
| P2-11 | 通知在键盘焦点或鼠标停留时暂停；重复暂停不损坏剩余时长；提供关闭按钮和有界全文滚动，保留通知记录 | `shell/Toast.svelte`、`state/notify.svelte.ts` |

恢复默认布局只复位尺寸与分屏比例，保留当前工具、开合与运行任务。不会因整理布局结束终端或任务。原有底部状态栏已有窄窗收纳，本轮未改其业务或增添常驻指标。

共享表单宿主也用于克隆与 Git 凭据：只有最上层接收输入；按宿主登记模态状态，顺序关闭和底层先结束都能恢复背景，避免新焦点守卫阻止凭据输入。

右键菜单也挂到窗口层，避免新增容器查询令 fixed 坐标相对工具窗计算。新增共享组件只服务这些已经存在的分隔线，没有引入停靠、浮动或插件框架。

## 验证

- `pnpm check`：0 错误、0 警告。
- `pnpm test`：112 个测试文件通过。
- 当前工作树 `cargo test --workspace`：735 通过、4 忽略；包含工作树原有其他改动，不表示这轮新增了这些测试。
- clippy 规矩检查通过。
- 浏览器使用真实 Svelte 前端、独立 IPC 桩。主回归 34 项通过：[脚本](../tests/browser/workbench.js)、[测量摘要](../design/workbench-2026-10-29/Implementation.evidence.json)。注入延迟仅用于验证乱序与保存阶段，不是性能测量。
- 工具分栏延伸回归 7 项通过：[脚本](../tests/browser/workbench-tools.js)、[结果](../design/workbench-2026-10-29/Tools.evidence.json)。覆盖 Services、构建结果、调试、侧栏键盘与手势取消，以及菜单开关语义；另有 [嵌套凭据窗口](../tests/browser/workbench-modal-stack.js) 8 项通过；共 49 项浏览器断言。
- 设置与运行配置交替打开 / 关闭 20 轮，焦点均进入表单，关闭后表单宿主移除、背景可交互。
- 测试身份 `.app` 的 `scripts/accept/settings.sh`：25 通过、0 失败；双窗口设置、键位即时更新、菜单模态拦截、保存与标签页限制，前台采样命中测试应用 0 次。
- 错版本验证：还原原始 Git 处理后 3 项失败；还原原始模态宿主后 Tab 与背景隔离失败；恢复共用高度后 3 项失败；恢复重复暂停覆盖剩余时长后通知测试失败；恢复底层窗口拦截后，凭据输入从 demo-user 变成空字符串、检查失败。全部恢复后重跑。浏览器 CLI 会把异常打印为 `Error` 但进程仍可能退出 0，因此按断言结果判红，不把 CLI 退出码单独当证据。

### 布局数值

| 视口宽度 | 审查时提交标题宽度 | 修复后 |
|---:|---:|---:|
| 1280 | 394 | 388 |
| 1000 | 114 | 226 |
| 900 | 14 | 230 |
| 800 | 0 | 352 |
| 720 | 0 | 272 |

720px 长项目 / 分支场景下，停止按钮右边缘 x=700，留在窗口内。高工具窗场景下，保存按钮约 y=181–209，处于 850px 窗口可见范围。

## 资源与打包

资源按 `docs/RESOURCE-GUARD.md` 执行，测试身份、后台、临时文件，不含 JVM。没有稳定条件下的同版本 before，不声称资源优化收益。

- 第一轮 quick 为 REVIEW：关闭后已知合计 174.33、168.86、201.85、204.72 MiB，触发增长调查阈值。每轮关闭后编辑器为 0、DOM 为 227、主进程 FD 为 11；主进程约 29–30 MiB，合计变化主要在 WebKit，未定位具体保留对象。[报告](../src-tauri/target/resource-reports.noindex/20261029T050936Z-lite-bridge.vTaCri/report.md)
- 按要求追加 full：8 轮关闭后 180.18、171.86、195.02、204.02、216.89、183.97、199.75、196.89 MiB；未触发既定持续增长阈值，结论 NO_SIGNAL。恢复后空闲已知合计 209.08 MiB，主进程 FD 为 11，退出残留 0。[报告](../src-tauri/target/resource-reports.noindex/20261029T051509Z-lite-bridge.DEDUXM/report.md)
- 上述 full 的源码 SHA-256 为 `442cd969769d79a423915604886ba5d3c1cd01502eda9a64d903684eaaeb2a6b`，二进制为 `2b0fe7787d3124a836638146c76b5bfa2175633746f271a455a2b5a8aaa731a7`。该轮在补齐嵌套模态输入守卫前完成。

最终版本 quick 仍为 REVIEW：关闭后已知合计 171.24 → 184.55 → 193.38 → 206.49 MiB，后续恢复空闲回落到 172.19 MiB；主进程 FD 始终为 11，退出残留 0。[报告](../src-tauri/target/resource-reports.noindex/20261029T052537Z-lite-bridge.Vks17g/report.md)。同一构建又追加了 full，结果如下。

最终版本 full 为 **NO_SIGNAL**：8 轮关闭后已知合计为 199.82、202.68、175.24、215.02、194.63、193.96、192.82、202.79 MiB；首末增长约 2.97 MiB、拟合斜率约 0.086 MiB/轮，未触发既定持续增长阈值。主进程 FD 始终为 11，退出残留 0，源码与 fixture 未变，系统内存压力级别保持 1。[最终报告](../src-tauri/target/resource-reports.noindex/20261029T052942Z-lite-bridge.UU3Heg/report.md)

最终 quick / full 使用同一测试构建：源码 SHA-256 `2ff48aaa84bd448779dd8fe03c71236d151aefad6ccbab000dce49caab5bed6d`，测试二进制 SHA-256 `d6d89c47699fea9f5583e146cca85740610a5fe2b2c8dbb6bef9ea2865220949`。正式包的身份不同，单列在产物指纹中。

`LITE_BUILD_OFFLINE=1 CARGO_NET_OFFLINE=true pnpm app:bundle` 已通过，正式 `.app` 与 `.dmg` 已更新，没有启动正式应用或覆盖其数据。[产物指纹](../design/workbench-2026-10-29/Bundle.evidence.json)。最终测试身份的原生设置验收再次 25 通过、0 失败，未抢前台。

生产前端入口 `162045 B`（158.25 KiB），低于现有 160 KiB 红线，仍触发 152 KiB 预警；没有调整预算。新增内容主要是共用分隔条、尺寸记忆及焦点 / 通知控制；键位表按需加载。

短测与长测的预热、等待时长和日志大小不同，不能直接当作优化前后比较。NO_SIGNAL 也不证明无泄漏；未做 Allocations 的对象保留归因。

## 限制与回退

浏览器验证不替代 VoiceOver、系统显示缩放和材质对比度验收。Services / 构建结果 / 调试的布局检查不代表真实 JVM、构建工具或调试协议重验；这些后端链路未修改。

回退应只撤销本轮差异。工作树在开始时已有未提交改动，不得用整树 reset / checkout 覆盖。内部尺寸与工具高度是可选快照字段，旧快照照常恢复；回退后旧解析器忽略新增字段。
