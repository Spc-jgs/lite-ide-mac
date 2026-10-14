# 接住 IDEA 的 Java 后端日常，但不丢本心（设计稿，2026-10-14）

> 状态：**已拍板（2026-10-14）**：D5 同意、顺序按第 6 节的建议、下载到第 0 步时再问具体文件。以下是拍板前写的原文。起因：用户的 IntelliJ IDEA 订阅到期，要 lite-ide 承接在 IDEA 里做的事 ——「要取代它，但不能丢失我们的本心」。
> 这份稿子回答三件事：IDEA 好在哪（列出来给你选）、哪些和本心冲突、怎么分步做才不变成第二个 IDEA。第 6 节是要你拍板的。

## 0. 一句话

**秒开、从不让人等、从不打断、占用小 —— 这四条不动；Java 的「懂代码」作为一层可以关掉、绝不挡路的增强加上去。**
编辑器、日志、Git、终端、任务照旧 1 秒内能用；懂 Java 的那一层在后台的独立进程里慢慢醒过来，醒之前现有的轻量跳转照常工作，醒了就无缝换上。

## 1. 冲突在哪，说实话

本心（DIRECTION D1、editor-standards 的前四条）和 IDEA 最值钱的部分**天然冲突**：

| 本心 | IDEA 为什么做得到那些事 | 冲突 |
|---|---|---|
| 从不让人等 | 打开项目先「Indexing…」几十秒到几分钟，建一份全项目的语义索引 | 跳转、用法、重构、补全全靠这份索引 |
| 占用小（<200MB） | 常驻 JVM，多模块项目 1.5–4GB | 语义索引就在这个 JVM 里 |
| 从不打断 | 弹窗、通知、后台任务 | 部分是习惯，可以不学 |

ARCHITECTURE 决策 7 当初拒绝语言服务器，理由就是这一条（「jdtls 启动 5–10s、常驻 1GB+，与轻量直接冲突」）；DIRECTION D2 把代码智能冻结。
**这一次推翻它的理由**：IDEA 没了，「深的交给 IDEA」那条退路不存在了。但冲突本身没消失 —— 所以要靠第 3 节的硬约束把它关在笼子里，而不是假装它不存在。

PLAN 立项时其实就留了这个口子：「语言智能走外置 LSP 子进程（天然隔离）、按需」。这次是把二期提前，并且加上本心的约束。

## 2. 大家为什么用 IDEA —— 列出来，你选

★ = 你在问卷里勾了的。「现在」是 lite-ide 今天有没有。

| # | IDEA 好在哪 | 现在 | 怎么做 | 代价 | 建议 |
|---|---|---|---|---|---|
| 1 ★ | **跳转**：⌘B 定义 / 实现 / 父类、类型层级、调用层级 | 有一半：Lezer 树 + import 解析，同文件、同包、已 import 的能跳；接口实现、跨模块、jar 里的跳不了 | jdtls | 语言服务器 | **要**（第 2 步） |
| 2 ★ | **查找用法**（⌥F7，穷尽） | 没有（按名字搜只能近似，给不了穷尽） | jdtls | 同上 | **要**（第 2 步） |
| 3 ★ | **实时报错** + ⌥↵ 快速修复 | 没有 | jdtls 的诊断 + code action | 同上；渲染要克制（不打断） | **要**（报错第 2 步、修复第 3 步） |
| 4 ★ | **补全**（带自动 import、按类型） | 只有 CM6 的单词补全 | jdtls | 同上 | **要**（第 3 步） |
| 5 ★ | **重命名 / 重构**（改名连引用、抽方法、移动类、改签名） | 没有；跨文件替换有（两段提交 + 可撤销） | jdtls 的 rename / code action，**先预览再落盘**，复用跨文件替换的两段提交和撤销日志 | 同上 | **要**（第 3 步）。jdtls 的重构种类比 IDEA 少，第 5 节说清 |
| 6 ★ | **看 jar 里的源码**（⌘B 进 Spring 的类） | 没有 | jdtls：有 sources.jar 用它，没有就反编译（jdtls 自带 FernFlower） | 同上 | **要**（第 2 步，和跳转一起） |
| 7 ★ | **Maven 面板**：多模块树、依赖、跑生命周期、刷新 | 没有专门的；任务运行能跑 `mvn ...`（要自己写一行 tasks.json） | **不需要语言服务器**：读 pom 的模块树、跑 `mvn -pl` ；依赖树用 `mvn dependency:tree` 的输出 | 小 | **要，先做**（第 1 步） |
| 8 ★ | **认出 Spring Boot 应用、一键启动**（Services / Run Dashboard，几个服务一起起） | 任务运行有（日志视图、整组停、端口卡片、崩溃后收尸） —— 缺「自动认出来」 | **不需要语言服务器**：扫 `@SpringBootApplication`，按模块生成「运行 xx 服务」；profile、环境变量进 tasks.json | 小 | **要，先做**（第 1 步）—— 这是本心最强的一块（日志视图）直接接上 |
| 9 ★ | **断点调试**（条件断点、看变量、求值、热替换） | 没有 | DAP 协议 + java-debug（jdtls 的插件）；启动时带 JDWP | **最大的一块**：断点栏、变量树、调用栈、单步，全新界面 | **要，放后面**（第 5 步） |
| 10 ★ | **跑单测**（方法旁的绿三角、结果树、重跑失败的） | 没有 | 先**不靠语言服务器**：`mvn -pl x -Dtest=类#方法 test`，读 surefire 的 XML 报告出结果树；输出进日志视图 | 中 | **要**（第 4 步） |
| 11 ★ | **HTTP 请求**（.http 文件，⌘↵ 发） | 没有 | 独立的小功能：解析 .http、Rust 侧发请求（要加 HTTP 依赖，先问）、响应在旁边；和日志视图联动（按 traceId 高亮这次请求的日志） | 小到中 | **要**（第 6 步） |
| 12 ★ | **Git** | 有，而且全：改动、提交、泳道历史、分支、工作树、推拉、双栏差异、冲突 | —— | —— | 已有 |
| 13 | 数据库 / Redis | 没有 | 你说「偶尔」 | 大（驱动、连接管理、结果表格） | **不做**：偶尔用的交给专门工具（DBeaver / TablePlus / redis-cli 在终端里），这是本心「不做 IDE 的全部」 |
| 14 | **本地历史**（Local History：不提交也能回到十分钟前那版） | 没有 | 每次保存存一份快照（按内容哈希去重、设上限），能和旧版比、能回退 | 小 | **建议要**：「从不丢东西」那条的下一层，IDEA 用户常被它救命 |
| 15 | 格式化 / 整理 import | 没有 | jdtls 的 formatting / organizeImports | 跟着第 3 步 | **要**（第 3 步） |
| 16 | 代码生成（⌘N：getter / setter / 构造器）、Lombok | 没有 | jdtls 的 source action；Lombok 靠给 jdtls 挂 lombok.jar 的 javaagent | 跟着第 3 步 | 要（第 3 步） |
| 17 | Spring 专属：Bean 之间跳、所有 `@RequestMapping` 的接口列表、application.yml 补全 | 没有 | 接口列表可以自己扫注解（不用语言服务器）；Bean 图、yml 补全要 Spring Tools 的语言服务器（又一个 JVM） | 中到大 | 接口列表**可以考虑**；其余**后做** |
| 18 | 代码检查（几百条 inspection）、结构化搜索 | 没有 | jdtls 的警告是编译器级别的，没有 IDEA 那么多 | —— | 不追 |

## 3. 本心的硬约束（每一步都要过这几条，过不了就不做）

1. **打开永远秒开，不等语言服务器。** Java 智能是渐进增强：没醒之前现有的 Lezer 跳转、大纲、着色照常；醒了换上。状态栏中间那格（「后台在跑什么」，ui.md 第十一条）说「Java：导入中 43%」，**不弹窗、不挡输入**。
2. **独立进程、按项目、可以关。** 一个项目一个 jdtls，挂了不影响编辑器（重启它是一个按钮）；内存设顶（`-Xmx`，按第 0 步实测定）；**这个项目没有 Java 标签开着十分钟就收掉**，下次要用再起（它的缓存在应用数据目录里，第二次起比第一次快得多）。设置里一个开关能整个关掉。
3. **不打断。** 报错是波浪线 + 滚动条上的点，不弹；不自动改代码；重构**先预览**（改哪些文件、哪几行），确认才落盘，**可撤销**（复用跨文件替换那套，REPLACE.md）。
4. **正式包不变大。** jdtls 和插件第一次用时下载（**先问**，说清是什么、从哪来、多大，校验 sha256），放在应用数据目录；`.dmg` 还是 4MB 左右。不装它，其余功能一样用。
5. **每一项有数字、量过才算做完**：首次导入多久、跳转 / 补全的 p50 / p95、常驻内存。照这个仓库的老规矩，写进文档。
6. **多语言不丢**：LSP 是通用的协议，Python（basedpyright）、TS（tsserver）以后接同一个客户端 —— D2 当时写的「以后加码先考虑跨语言的」，这条路正好是。

## 4. 怎么搭

```
crates/lspsvc/     新 crate，不依赖 Tauri（照服务 crate 的老规矩，能脱离界面测）
  - 起语言服务器子进程（进程组、stdin / stdout 上的 JSON-RPC、Content-Length 分帧、stderr 设闸）
  - 请求 / 响应配对、取消（$/cancelRequest）、进度（workDoneProgress → 状态栏那格）
  - 反方向的请求（workspace/configuration、applyEdit）—— DIRECTION 第 6 节为 ACP 想过的「反方向 RPC」，这里先用上
  - 文档同步：编辑器里**未保存**的内容才是真相（didOpen / didChange），不是盘上那份
  - 生命周期：按项目一个实例、空闲收掉、崩了能重起、退出不留孤儿（tasksvc 的进程组 + 账本那套）
crates/dapsvc/     第 5 步：调试适配器协议（java-debug 通过 jdtls 的命令拿到端口，DAP 走 TCP）
commands/lsp.rs    薄命令层
src/lib/lang/      懒加载：CM6 扩展 —— 诊断（lint）、补全源、悬停、跳转接现有的 nav、改名 / 修复的界面
src/lib/run/       第 1 步：Maven 工具窗、Spring Boot 识别，接现有的任务运行
```

**为什么是 jdtls，不是自己写**：Java 的语义（重载解析、泛型推断、继承、注解处理、Lombok）自己写就是再写一个编译器前端。
jdtls 是 VS Code 的 Java 支持用的同一个引擎（Eclipse 的 Java 编译器），Maven 多模块导入（m2e）、反编译、调试、单测的插件都现成。
IntelliJ 的引擎不对外，用不了。

**和 AI（ACP，#46）的关系**：IDEA 那种「一个键就做对」的日常操作不靠 agent；agent 接的是 jdtls 做不到的那部分（大重构、写测试、解释陌生代码）。两件事不互相替代。

## 5. 说清楚：和 IDEA 比会差在哪

- **第一次打开一个多模块项目要等 jdtls 导入**（几十秒到几分钟，第 0 步量）—— 但等的只是 Java 智能，编辑器、日志、Git、启动服务不等。IDEA 是整个界面都在等。
- **重构的种类少**：改名、抽方法 / 变量 / 常量、内联、移动类、改签名、整理 import 有；IDEA 那上百种 intention、结构化替换没有。
- **Spring 智能弱**：Bean 导航、yml 补全要另一个语言服务器（Spring Tools），先不上。
- **代码检查是编译器级别的**：IDEA 那几百条 inspection 没有。
- **调试是最后做的一块**：第 5 步之前，调试还得靠在终端里 `mvn spring-boot:run -Dspring-boot.run.jvmArguments=...` 配合远程调试，或者另一个工具。

## 6. 要你拍板的

| | 问题 | 建议 |
|---|---|---|
| **D5** | 定位从「取代 Sublime」扩成「取代 Sublime + 接住 IDEA 的 Java 后端日常」；D2（代码智能冻结）和 ARCHITECTURE 决策 7（不上 LSP）推翻，改成第 3 节的约束 | 同意。理由在第 1 节 |
| 下载 | jdtls（约 50MB，download.eclipse.org）+ java-debug / java-test 插件（各几 MB）；**你机器上只有 JDK 17，jdtls 要 JDK 21 才能跑**（它编译的项目可以是 8–25），要再装一个 JDK 21（约 200MB，Adoptium）当「工具 JDK」，你的项目照旧用 17 | 第 0 步实测时下载，下载前再问一次具体的文件 |
| 顺序 | 第 1 步 Maven + Spring Boot（不需要语言服务器，几天就能用上）→ 第 2 步只读智能（跳转、用法、报错、jar 源码）→ 第 3 步写的智能（补全、改名、重构、修复、格式化）→ 第 4 步单测 → 第 5 步调试 → 第 6 步 HTTP 请求；本地历史穿插 | 按这个顺序：先拿到不用等的，再上要等的 |
| 内存 | 编辑器本体 <200MB 不变；jdtls 另算，上限按第 0 步实测（多模块大概 1–2GB；你有 48GB） | 第 0 步量完再定 |
| 不做 | 数据库 / Redis 客户端、几百条 inspection、Spring Bean 图 | 不做（偶尔用的交给专门工具） |

## 7. 第 0 步（拍板之后、写代码之前）

在**你的一个真实多模块项目**上量（不是造的项目）：

1. jdtls 第一次导入多久、第二次（有缓存）多久
2. 常驻内存（RSS），空闲和刚导入完各多少
3. 跳转定义、查找用法、补全的延迟 p50 / p95
4. Lombok、多模块之间的跳转、jar 里的类能不能跳

量完决定：`-Xmx` 设多少、空闲多久收掉、哪些能力值得上。数字写进这份文档。

## 8. 第 1 步的实测：Spring Boot 怎么「照 IDEA 那样」起（2026-10-14）

> **第 1 步前一半做完了**（同日）：认出 Spring Boot 应用、在任务列表（⌃⌥R）的「Spring Boot」一组里一键起（`src-tauri/src/maven.rs`）。
> 真 .app `scripts/accept/spring.sh` 8/8：改公共模块再起不用 install 就生效、web 服务 ⌘F2 停完端口空、整组一个不剩、本地仓库里没多东西；
> petclinic 的 8 个服务按 pom 里的模块顺序全列出来。**后一半（Maven 工具窗：模块树、clean / package / install、依赖树）还没做。**

用户说「先别碰我的项目」，验证用两份：GitHub 上 Spring 官方的 spring-petclinic-microservices（8 个服务模块，浅克隆 4.5MB；
AGENTS.md 规定下载来的代码不跑构建，这次是用户点头的例外 —— 跑之前读过全部 pom：会执行东西的 exec / libsass / unpack 都在不自动启用的 profile 里，
默认构建只有 build-info、git-commit-id、enforcer），和一份**自己写的**三模块样例（shop-common + order-service + user-service，验「兄弟模块」那种情形：
petclinic 的服务互不依赖，而真实项目几乎都有一个 xxx-common）。

**认出来**：扫各模块 `src/main/java` 里带 `@SpringBootApplication` 且有 `static void main` 的类 —— petclinic 8 个服务一个不漏。
这是作者写下的声明（同 package.json 的 scripts），不是猜（TASKS.md Q2 当初不自动认 Maven 的理由是「猜错比不猜糟」，这里不猜）。
不带 profile 起（IDEA 的默认也是不带），用户说他的运行配置「什么都没配，都是 IDEA 自己配的、配置全在 yml 里」。

**怎么起**：照 IDEA 的做法 —— 编译 reactor、算出 classpath、直接 `java -cp … 主类`，不走 `spring-boot:run`：

```bash
mvn -q -B -pl <模块> -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile=target/.lite-ide-cp.txt \
  && cd <模块> && exec java -cp "target/classes:$(cat target/.lite-ide-cp.txt)" <主类>
```

| | 数 |
|---|---|
| 自己的三模块样例：离线编译 + 算 classpath | **1.67 s** |
| 兄弟模块在 classpath 里是 | `shop-common/target/classes`（**不是本地仓库里的 jar**：改了公共模块再起就是新的，不用 install） |
| 起来（非 web 的样例） | 0.53 s，探针那行同时打出兄弟模块的类和 application.yml 里的值 |
| petclinic customers-service 第一次（含下载依赖） | 51.7 s，classpath 204 项 |

**排除的路**（实测）：`mvn -pl order-service spring-boot:run` —— `Could not find artifact com.demo:shop-common`：不带 `-am` 时兄弟模块要从本地仓库找，
没 install 过就没有；install 过的话跑的是旧 jar（改了公共模块不生效，而且不报错）。直接起 java 还有一个好处：调试（第 5 步）就是多加一个 JDWP 参数。

**顺带的事实**：这台机器的本地仓库在 `~/repository`（settings.xml 里设的），不是默认的 `~/.m2/repository` —— 一律经 `mvn` 自己读 settings，别在代码里写死路径。
