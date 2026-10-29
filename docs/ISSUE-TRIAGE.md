# Issues 核对与整理清单（2026-10-29）

首次通过 GitHub 连接读取本仓库的 open / closed issues，并读取全部 open issue 的评论：**81 条，24 open、57 closed**。用户随后明确授权同步，已执行并回读确认：**83 条，17 open、66 closed**。
核对依据是当前代码、测试 / 验收脚本和带日期的运行记录，详细能力边界见 [STATUS.md](STATUS.md)。

## 1. 已以 completed 关闭的 8 条

每条已更新完成证据与能力边界，再以 completed 关闭；原始正文保留在折叠区域。只有重新跑过的结果才记作本轮通过，公开内容不带本机路径、用户信息或测试敏感数据。

| Issue | 当前实现证据 | 已同步的关闭摘要 / 边界 |
|---|---|---|
| [#59](https://github.com/Spc-jgs/lite-ide-mac/issues/59) 停完检查端口 | `tasksvc::port::lingering`、`task-linger`、`runs-ops::lingered`，`accept/tasks.sh` | 停止后仍被组外进程占用的端口显示卡片。并不承诺能自动停掉所有 Docker / daemon 进程 |
| [#62](https://github.com/Spc-jgs/lite-ide-mac/issues/62) Git 凭据提问 | `gitsvc/src/askpass.rs`、`src/lib/git/askpass.ts`，`accept/git-auth.sh` | 应用自身承担 askpass，支持用户名 / 密码与取消，不把密码落盘。issue 原拟「临时脚本」没有采用 |
| [#72](https://github.com/Spc-jgs/lite-ide-mac/issues/72) 日志 / 命中次数 / 异常断点 | `java-debug-gutter.ts`、`BreakpointsDialog.svelte`、`debug.rs`，`accept/spring.sh` | 三类断点已接界面与 java-debug；无效断点外观单独留在 #73 |
| [#74](https://github.com/Spc-jgs/lite-ide-mac/issues/74) 远程 JVM | `projectcfg` 的 remote 类型、DAP attach，`accept/spring.sh` | 可挂已有 JVM，断开不杀目标，对端退出会结束会话。不是远程开发或服务器部署能力 |
| [#75](https://github.com/Spc-jgs/lite-ide-mac/issues/75) Cookie | `httpsvc/src/lib.rs` 的 cookie 罐、HttpPane Cookie 页，`accept/http.sh` | 按项目 / 环境隔离，支持清除与 no-cookie-jar，权限 0600 |
| [#76](https://github.com/Spc-jgs/lite-ide-mac/issues/76) 响应脚本 | `http-script.ts`、`http-ops.ts`，`http-script.test.ts`、`accept/http.sh` | 采用 issue 建议的安全子集：只认 client.global.set，其他语句列出不执行。关闭摘要必须明确不支持 client.test / 任意 JS |
| [#77](https://github.com/Spc-jgs/lite-ide-mac/issues/77) HTTP 后续 | multipart、历史、JSON 着色已提交；本轮收完 traceId，`accept/http.sh`、`accept/spring.sh` | 四项逐项完成；traceId 仅跳本项目、由应用运行的本机服务 / 任务，不自动追踪远程、代理或组外 JVM |
| [#78](https://github.com/Spc-jgs/lite-ide-mac/issues/78) Gradle | `gradle.rs`、Gradle 工具窗、测试 / 调试 / 覆盖率 / 热替换，`accept/gradle.sh` | IDE 管理的 Spring 运行用 exec java，不是裸 bootRun；原始 bootRun 停止验收继续留在 #60 |

## 2. 已收窄标题 / 范围、继续 open 的 8 条

| Issue | 已做的部分 | 已更新的剩余范围 |
|---|---|---|
| [#47](https://github.com/Spc-jgs/lite-ide-mac/issues/47) 开源门面 | 欢迎页、最近项目已有 | `发版验收：README 截图、干净机器安装与首次打开`；截图脚本仍要迁测试身份 |
| [#53](https://github.com/Spc-jgs/lite-ide-mac/issues/53) 保存键与菜单顺序 | 保存已按动作 id 跟随键位设置；注释不再宣称所有菜单键都先被 AppKit 吃掉 | `真机验证：菜单键在编辑器 / 终端 / 文件树里的分发矩阵`；不能为整洁把 save 改成 cm6 而破坏其他焦点下的保存 |
| [#54](https://github.com/Spc-jgs/lite-ide-mac/issues/54) 验收后续 | #22 已关闭；smoke 走测试通道；测试应用的系统转交焦点问题已有 Accessory 修复 | `验收补齐：真实输入、截图 / 内存脚本迁移、测试废纸篓收尾、CI`；旧评论的猜测不能盖过后续已验证结论 |
| [#69](https://github.com/Spc-jgs/lite-ide-mac/issues/69) 图标后续 | 本轮换成实线代码图形；重生成两种格式，检查小尺寸 | `图标验收：系统深色 / 着色、资源同步门禁与 universal 包`；小尺寸本地看过，不等于系统所有外观验完 |
| [#73](https://github.com/Spc-jgs/lite-ide-mac/issues/73) 断点列表 | 列表、启停、条件编辑、跳转已有 | `调试：未验证 / 无效断点的外观与原因`；verified 结果未成为前端状态 |
| [#79](https://github.com/Spc-jgs/lite-ide-mac/issues/79) 本地历史 | 保存快照、外部修改、恢复已有；替换 / 改名另有持久撤销日志 | `本地历史：补重构与 Git 改盘前快照、目录视图 / 标签 / 局部恢复`；先修正文中「撤销窗口一关就没」的过时说法 |
| [#80](https://github.com/Spc-jgs/lite-ide-mac/issues/80) 单测后续 | 按类更新结果、JaCoCo、Nested 结果名展示已有 | `单测：@Nested 运行目标识别，区分按类更新与方法实时事件`；本轮已复现内部方法被归到外层类 |
| [#81](https://github.com/Spc-jgs/lite-ide-mac/issues/81) Java 重构 | 抽取、内联、移动类、改签名已有 | `Java 改名：执行前预览与原地联动编辑`；不要把已做重构全部留成未完成 |

## 3. 保留需求 / 调查的 7 条

| Issue | 优先级与处理 |
|---|---|
| [#49](https://github.com/Spc-jgs/lite-ide-mac/issues/49) GPU 内存 | P2 调查：同一批窗口一次改一个变量，重测 physical footprint。历史波动不是已复现的内存泄漏 |
| [#50](https://github.com/Spc-jgs/lite-ide-mac/issues/50) 原生右键焦点 | P1 验收缺口：需要安全的进程定向真实输入或单独人工验收；DOM click 不能替代 |
| [#51](https://github.com/Spc-jgs/lite-ide-mac/issues/51) 拉服务器日志 | 待实际工作方式：先确定 scp / 跳板机 / 平台下载，再决定是否内置 |
| [#58](https://github.com/Spc-jgs/lite-ide-mac/issues/58) pty 任务 | P3：只为已确认的管道块缓冲程序加；先验证组信号和输出清洗，不改默认 |
| [#60](https://github.com/Spc-jgs/lite-ide-mac/issues/60) bootRun 停止 | P2 验收：IDE 的 exec java 启停不能替代自定义 ./gradlew bootRun 的取消传播；和 #78 分开 |
| [#63](https://github.com/Spc-jgs/lite-ide-mac/issues/63) 浅色主题 | P2 产品功能：已有全套颜色令牌，但浅色值和代码 / 终端配色仍需整体验收 |
| [#46](https://github.com/Spc-jgs/lite-ide-mac/issues/46) ACP | 按之前决定暂缓；第一版宿主已做，真实适配器直接写盘的审阅与原定目标不一致，不能关闭 |

## 4. 已以 not planned 关闭的 1 条

[#57](https://github.com/Spc-jgs/lite-ide-mac/issues/57) shell 环境缓存：`TASKS.md` 第 21 节已测得登录 shell 中位约 237 ms，对比直接 sh 约 4 ms。历史决定是暂不换默认，避免破坏 alias / 函数和引入环境刷新问题。
已以 `not planned` 收起，保留「实际 rc 达到秒级、确有用户等待时再开」的触发条件；没有标成已实现。

## 5. 已新建的 2 条

1. [#82 P1 构建间歇延迟](https://github.com/Spc-jgs/lite-ide-mac/issues/82)：spring.sh 曾在等满约 240 秒之后才起 JVM；附 JOURNAL 2026-10-28 的时间线，根因 UNKNOWN。新一轮没撞上也保留。
2. [#83 P1 PostgreSQL SQL 分句按 MySQL 转义，可能选错执行范围](https://github.com/Spc-jgs/lite-ide-mac/issues/83)：本地 `splitSql("select 'C:\\'; select 2;")` 返回一条；普通 PG 字符串在 `standard_conforming_strings=on` 下不把反斜杠当转义，见 [PG 17 官方说明](https://www.postgresql.org/docs/17/runtime-config-compatible.html#GUC-STANDARD-CONFORMING-STRINGS)。应按连接方言 / 字符串模式识别，补光标在第二条的验收；本轮没有在真实 PG 执行这个样例。

文档进度漂移本轮已用状态入口和此清单纠正，不为「以后别忘记维护」另开一条没有验收终点的 issue。

## 6. 远端同步结果与证据边界

用户明确要求 issues 也同步之后执行。连接器写入返回 403（integration 权限不足），改用本机已有授权的 gh，核对仓库管理权限后完成。
没有改动现有评论、标签、指派人或 milestone；更新正文时保留原始描述。关闭时附对应验收结果和上述边界；没有本轮运行证据的只说明源码 / 历史证据，没有写成「本轮全过」。

本轮实际执行：HTTP 33 / 33、Spring 111 / 111、任务 44 / 44、Git 凭据 8 / 8、smoke 142 / 142；类型检查、111 个前端测试文件、Rust 727 条（4 跳过）和 clippy 通过。#78 的 Gradle 专项仍只引用历史验收，本轮未重跑，不能在关闭摘要中冒充新结果。
