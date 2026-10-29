# 当前状态与接手清单（2026-10-29）

这份记录回答「现在能做什么、哪里还差、下一步做什么」。起点是 `0e0fda9`，连同当时未提交的 HTTP 链路 ID 改动一起检查；不是重新设计一套路线。
`TAKEOVER.md`、`IDEA.md`、`DIRECTION.md` 保留了分期和历史推导，里面的「现在」「还没做」不能脱离日期使用。
GitHub issues 的逐条核对见 [ISSUE-TRIAGE.md](ISSUE-TRIAGE.md)。
用户授权后已同步远端：8 条 completed、#57 not planned、8 条收窄范围，新增构建卡顿 [#82](https://github.com/Spc-jgs/lite-ide-mac/issues/82) 和 SQL 分句 [#83](https://github.com/Spc-jgs/lite-ide-mac/issues/83)；当前 17 open、66 closed。

## 1. 判断与证据边界

Java 后端日常的主链路已经有实现，生态扩展也进入了数据库、多语言和自动更新。下一步的关键是修正剩余行为、把已有能力在真实项目上验稳，不能用功能数量或 MVP 的勾代替完成度。
本次是接手盘点与重点路径检查，没有穷尽全部界面与项目类型。

对应原分期：MVP-0 到 MVP-5 的主要路径已落地但有延期 / 不完整项；MVP-6 AI 暂缓；MVP-7 已有 Gradle 和三种语言包，声明式配置仍不齐；MVP-8 已有更新器，分发验收未齐；MVP-9 的 DB-1 到 DB-4 已有实现与历史验收。

- **源码证据**：核对命令登记、状态层、视图和引擎；可以证明路径存在，不能证明所有项目都能用。
- **本轮自动检查**：类型检查 0 错误 / 0 警告；前端 111 个测试文件通过。Rust 和真应用验收结果在第 6 节逐项记。
- **历史验收**：JOURNAL 中有 Java / Maven / Gradle / 数据库的真实运行记录；保留它们的日期，不当成本轮执行结果。
- **尚未证明**：用户实际项目连续使用、真实 macOS 按键 / 输入法 / 原生右键、干净机器安装、公开发布的更新链路。

## 2. 已有能力与仍然存在的边界

| 区域 | 已有实现 | 尚未做完整 / 不能据此声称的能力 |
|---|---|---|
| 编辑器与外壳 | 多光标、分屏、草稿、会话恢复、多窗口、书签、最近位置、欢迎页、图片预览、通知记录、标签上限 | 浅色主题、工具窗右侧布局 / 移动、完整的布局设置；PDF 是交给系统「预览」，不是内嵌阅读器 |
| Java 读写 | jdtls 补全、import、定义 / 实现 / 父方法、文档、用法、类型 / 调用层级、生成代码、改名、抽取、内联、移动类、改签名 | 原地联动改名、改名前预览；跨文件引入参数受限；没有 IDEA 全套 inspection / intention / 结构化替换 |
| 构建与运行 | Maven / Gradle、Spring Boot 识别、统一运行配置入口、Services、停止与端口占用提示 | 任意自定义 Gradle `bootRun` 的组外进程停止仍要单独验证；配置类型 / before-run 任务 / 共享 `.run` 文件仍不齐 |
| 调试 | 条件 / 日志 / 命中次数 / 异常断点、断点列表、变量、求值、单步、监视、运行到光标、远程 attach、方法体热替换 | 无效断点没有灰色未验证态；不是任意结构变化都能热替换；构建卡顿历史问题根因未定 |
| 单测 | 类 / 方法运行、失败重跑、调试、按类更新结果、JaCoCo 覆盖率 | `@Nested` 的结果名可显示，但内部类 / 方法的运行目标不正确；不等于单个测试方法实时完成事件 |
| Spring / MyBatis | 路由搜索、`@Value` 跳配置、Bean 注入导航、配置元数据补全、枚举值补全、Mapper ↔ XML | Bean 导航靠文本识别，不能等同于完整容器模型、动态条件 / profile / 工厂语义；没有 Actuator 全套视图 |
| Git | 提交、历史、分支、工作树、推拉、冲突、stash、cherry-pick、交互式 rebase、文件 / 行历史、凭据提问 | GitHub PR 评审尚无集成；改变文件的操作没有统一写入本地历史 |
| HTTP | 环境、文件体、取消、Cookie、受限 `client.global.set`、multipart、历史、JSON 着色、响应头联日志 | 不是任意 JavaScript 响应脚本 / client.test 运行器；联日志只认本机、当前项目、应用管理的运行，不能自动跨远程 / 代理追踪 |
| 日志 | GB 文件、过滤、跟随、堆栈跳转、合并、归类 / 对比、长行详情 | 从服务器拉日志尚未定义方案；GPU 多窗口占用调查未完 |
| 数据库 | JDBC 边车、钥匙串、结构树、控制台、分批结果、取消、历史、保护确认、单元格编辑、导出、补全、IDEA 导入；MySQL / PG / SQLite / H2 驱动 | 一个连接一个游标；更多数据库驱动未加；不是完整 DataGrip。真 MySQL / PG 本轮未重验 |
| 多语言与设置 | Rust / Python / TS 的声明式语言包、通用 LSP 能力、包声明任务、键位覆盖、IDEA Java 风格、`.editorconfig` 的缩进及保存处理 | Java 尚未搬到通用包；运行配置表单不能声明；`.editorconfig` 的 charset / end_of_line 未接；部分固定键位不能改 |
| AI 与发版 | ACP 第一版、权限卡片、签名更新器、本地模拟更新验收脚本 | 真 Claude 适配器直接改盘的审阅未收完，AI 暂缓；Actions secret 是否已配置、实际 Release 安装 / 升级未确认 |

对应实现集中在 `src/lib/editor/java-lsp.ts`、`src-tauri/src/{java,debug,gradle,spring_beans,spring_meta,langs,db,update}.rs`、`src-tauri/crates/` 和 `src/lib/{run,db,http,debug}/`。

## 3. 当前已确认的问题

| 优先级 | 问题 / 触发条件 | 证据与影响 | 后续动作 |
|---|---|---|---|
| P1 | `@Nested` 方法的 ▶ 指向外层类 | `testTargets` 只记录第一个 class。最小样例 `OuterTest { @Nested class InnerTest { @Test void nestedCase() {} } }` 返回 `OuterTest#nestedCase`，应为 `OuterTest$InnerTest#nestedCase`。见 `src/lib/editor/java-tests.ts`；现有 Nested 测试只测结果展示 / 源码定位 | 收窄 #80 为嵌套测试目标识别，并补真 surefire / Gradle 验收 |
| P1 | PostgreSQL 普通字符串里的反斜杠导致 SQL 分句错误 | 本地调用 `splitSql`，`select 'C:\'; select 2;` 被当成一条；`skipQuoted` 对所有连接都按 MySQL 默认转义。PG 的 `standard_conforming_strings=on` 下反斜杠是普通字符，和 `E'...'` 不同。未连接真实 PG 执行这个样例 | 按连接方言 / 字符串模式切语句，并验证光标在第二条时只发第二条，不能统一改掉反斜杠处理而破坏 MySQL |
| P1 | 断点画出来却不说明该行无法停住 | Rust `debug.rs` 会取得 verified 行，前端 `Bp` 和 `BpMarker` 没有 verified 态；空行、注释行看起来仍是普通断点 | 保留 #73，补会话内验证态与提示；没开调试时不能提前画成无效 |
| P1 | 重构 / Git 丢弃前没有统一的本地历史快照 | 历史接在 `commands/fs.rs` 的保存路径；不能把替换自己的撤销等同于本地历史。#79 的「撤销窗口一关就没」也不准确：跨文件替换 / 改名已有持久撤销日志 | 保留 #79，先列操作 × 保护方式，再补真正缺的快照 |
| P2 | 原地改名与改名前预览未做 | `renameAt → RenamePopup → doRename → javaRename` 直接执行；移动类 / 改签名有预览，不能反推 rename 也有 | 收窄 #81；先做改名前预览，再评估原地联动 |
| P2 | `.editorconfig` 只覆盖一部分 | 已有缩进和保存清理，未接 charset / end_of_line；固定离开自动保存也没有开关 | 明确设置边界，按实际项目需求补齐，不把设置树的每个 ➕ 都机械实现 |

## 4. 仍待复现 / 人工或环境验收

- **构建间歇卡住**：JOURNAL 2026-10-28 记录过约 240 秒后才真正起 JVM，埋点后 12 / 12 通过；没有坐实根因。保留时间线，下一次失败记录一次完整日志，不写成已修。
- **#50 / #53**：原生右键后的焦点、AppKit / WebKit 分发真实键位；DOM 合成事件不能覆盖这一层。保存键跟随设置与错误注释已修，但整张真实按键矩阵未验。
- **#49 / #69**：GPU 增量内存，图标在系统深色 / 着色 Dock 与 universal 发布包中的实际外观；不能为验收擅自切系统外观或抢前台。
- **#47**：干净 macOS 用户 / 机器安装与 Gatekeeper、首次打开；当前机器已有缓存和配置，不能替代干净环境。
- **真实项目**：建议用一个正在维护的多模块 Java 项目，连续验证导入、跨模块重构、运行、调试、测试、切分支及会话恢复。只读真实数据；涉及共享写入另行确认。

## 5. 仓库留下的后续路线，按接手价值重排

以下从分期文档、issues 和源码缺口恢复，不是读取到的 agent 私有任务队列；排序是这次核对后的建议。

1. **收当前工作、恢复可验收构建**：HTTP traceId 联日志，本轮处理；随后先补第 3 节的具体错误和 #50 / #53 的输入证据。
2. **先修执行对象的准确性，再补调试状态**：Nested 选择器、PostgreSQL SQL 分句、无效断点、构建卡顿时间线，避免「点的是一处，实际跑的是另一处」。
3. **补变更的可追溯性**：#79 本地历史、#81 改名前预览；先厘清现有撤销机制，避免重复造一套。
4. **补日常外观与设置**：#63 浅色主题；工具窗布局、保存 / 编码选项按真实使用频率排。书签、⌘O 找类、通知记录已经做了，不再排一次。
5. **完成 MVP-7 声明式生态的剩余范围**：运行配置表单、Java 是否搬包；Rust / Python / TS 包和任务声明已有，不再写「未开始」。
6. **发版收尾**：#47 README 截图、首次打开、干净安装；#69 系统外观 / universal；验证签名配置与真实更新路径。自动更新器代码已做，不代表 Release 运维已完成。
7. **待需求再定**：#51 拉服务器日志；#58 pty 任务；#60 任意 bootRun 的停止；GitHub PR 集成。
8. **明确暂缓**：#46 AI 的真实 agent 审阅与提交信息；#57 shell 环境缓存已测得约 240 ms 的成本，历史结论是暂不做，不是遗忘。

## 6. 本轮收尾与验收记录

- traceId：接上响应头按钮，跳普通任务的运行窗或 Spring 的 Services；搜索整个 ID；查端口期间关闭的任务不再被选中；远程地址不给本机同端口的假结果。
- Services：补齐缺失导入；聚焦请求只消费对应的变化，自动展开服务所在分组，不因后台运行状态变化反复抢回选中项。
- 测试验红：去掉项目隔离，12 条状态检查中 2 条失败；去掉查询的字面量引用，2 条失败；错误地用整个 traceparent 搜索，45 条头解析检查中 2 条失败；恢复后分别 12 / 12、45 / 45。
- 图标：改脚本并重生成 `.icon`、`Assets.car`、完整 `.icns` 和 PNG；检查 16 / 32 / 128 / 256 / 512 / 1024 尺寸。未切系统外观，未宣称已验 Dock 深色 / 着色。

| 本轮命令 / 检查 | 结果 |
|---|---|
| `pnpm check` | 0 错误、0 警告 |
| `pnpm test` | 111 个文件通过 |
| `cargo test --workspace --offline --locked` | 727 通过、0 失败、4 跳过；跳过项包括真实废纸篓、真实登录 shell、性能测量 |
| clippy（含 test-bridge，只查 disallowed_methods） | 通过 |
| `scripts/accept/http.sh` | 33 通过、0 失败 |
| `scripts/accept/spring.sh` | 111 通过、0 失败、0 未验；含真实调试 / 单测 / 覆盖率 / 热替换 / 远程 attach，以及本轮联日志 |
| `scripts/accept/tasks.sh` | 44 通过、0 失败；含停止后的端口占用卡片 |
| `scripts/accept/git-auth.sh` | 8 通过、0 失败；仅本机临时认证服务器，不碰真实钥匙串 / 仓库远端 |
| `scripts/smoke.sh` | 142 通过、0 失败；AI 默认关闭；前台记录中除已知启动转交外为 0 次 |
| `pnpm app:bundle`（离线 Cargo，Java 资源缓存命中） | 正式 `.app` + aarch64 `.dmg` 已生成，DMG 102,391,438 B |
| 正式前端入口 | 156,668 B，接手前后差 0 B |
| 正式包资源 / 隔离 | Assets.car 与 icon.icns 和生成文件的 SHA-256 一致；图标名 AppIcon 对得上；正式二进制没有测试通道标记，JS 没有测试钩子 / 桩哨兵 |

包在 `src-tauri/target/release/bundle/{macos/lite-ide.app,dmg/lite-ide_1.5.0_aarch64.dmg}`。本轮没有运行正式身份去碰真实会话；也没有运行 `db-real.sh`、`gradle.sh`、`java.sh`、`lsp.sh`、`update.sh`，这些领域的历史结果不升级为本轮结果。
smoke 仍会留下一个自己生成的 `lite-ide-smoke-trash-*` 测试文件在废纸篓；本轮尝试按确切名称和内容清理，被 macOS 权限拒绝，没有改动其他废纸篓内容。#54 的这项收尾仍应保留。

## 7. 维护上的问题

完成标记、README、issues 长时间不同步，是这次接手最容易误判的地方。MVP 的 ✅ 也混入了主动延期项；`TAKEOVER.md` 同时保存最初设计和后续结论，读一个局部会得出相反结果。
以后每轮把「本轮实际跑过哪些、没跑哪些、原 issue 剩什么」一并写到这份状态页和 issue 核对表；历史推导留原日期，避免为追求整洁抹掉已知限制。
