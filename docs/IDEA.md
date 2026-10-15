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
4. ~~**正式包不变大**：jdtls 第一次用时下载~~ —— **2026-10-15 用户推翻**：「这应该是内置的功能」。jdtls 和跑它的 Java 21 都打进 `.app`（第 10 节），`.dmg` 7 → 98 MB（universal 约 137 MB）。
   留下的那半条照旧：**不打开 Java 文件就不起 JVM、不占内存**；不是 Maven / Gradle 项目不起。
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

- **第一次打开一个多模块项目要等 jdtls 导入**（petclinic 实测：本地仓库齐 3.5 s，要上网核对 Maven 元数据的那次 127 s；之后 1.2 s，第 7 节）—— 但等的只是 Java 智能，编辑器、日志、Git、启动服务不等。IDEA 是整个界面都在等。
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
| 内存 | 编辑器本体 <200MB 不变；jdtls 另算，上限按第 0 步实测（多模块大概 1–2GB；你有 48GB） | 第 0 步量了：`-Xmx1G`、RSS 1.1–1.5 GB（第 7 节），第 2 步开工前定 |
| 不做 | 数据库 / Redis 客户端、几百条 inspection、Spring Bean 图 | 不做（偶尔用的交给专门工具） |

## 7. 第 0 步的实测：jdtls 在 petclinic 上（2026-10-14）

原计划在用户的真实项目上量，用户说「先别碰我的项目」，换成 spring-petclinic-microservices（9 个 Maven 项目：根 + 8 个服务）
和自己写的 shoplombok（shopdemo 三模块 + shop-common 里一个 `@Data` 类、order-service 调它的 getter / setter —— 验 Lombok 和兄弟模块）。
jdtls 1.61.0（2026-09-03，sha256 核过），跑在 Temurin 21.0.12.1 上（Homebrew cask，`/Library/Java/JavaVirtualMachines`；
用户的 `.zshrc` 把 `JAVA_HOME` 钉在 17，终端里的 mvn / java 不受影响）。`-Xmx1G`，M 系列 48GB。
量具是两个 Python LSP 客户端（量延迟的 190 行、验 Lombok 的 120 行；不进仓库：第 2 步的 Rust 客户端才是产品），每个请求连发 7 次，第一次单列。

| 量什么 | 冷（数据目录空、本地仓库缺元数据） | 冷（数据目录空、本地仓库齐） | 热（数据目录有缓存） |
|---|---|---|---|
| ServiceReady（导入完、能答语义问题） | **127.1 s** | 3.3–3.6 s | **1.2 s** |
| 刚 ready 的常驻内存（RSS） | 468 MB | 476–493 MB | 376 MB |
| 跳到定义（项目里的类）第一次 / 之后 p50 | 981 / 10.0 ms | 849–857 / 9.9–10.7 ms | 1055 / 12.8 ms |
| 跳到定义（jar 里：`findById` → `CrudRepository.class`） | 3099 / 11.6 ms | 17–23 / 10.4–11.8 ms | 31 / 11.7 ms |
| 查找用法（`Owner`，19 处） | 3904 / 54 ms | 5343–6016 / 70–76 ms | 788 / 65 ms |
| 补全（`ownerRepository.` 后面，40 项） | 59 / 7.7 ms | 44–53 / 7.9–8.1 ms | 42 / 9.1 ms |
| 实时报错（改错一个方法名 → 那条 error 回来） | 439 ms | 431–496 ms | 435 ms |
| 做完这些之后的常驻内存 | 1395 MB | 1462–1494 MB | 1143 MB |

**这些数字说明什么**：

- **127 秒全花在网上**。状态消息里一串 `https://maven.aliyun.com/repository/public/...`：m2e 导入时去镜像核对元数据（版本区间、`maven-metadata.xml`），
  本地仓库 `~/repository` 里多了 787 个文件、**全是元数据**（`.sha1`、`_remote.repositories`、几个 xml，共 3 MB，一个 jar 都没有；313 个在 `org/webjars/npm`，版本区间最多的那片）。
  第二、三次数据目录照样是空的，3.5 秒 —— 区别只在本地仓库已经核对过。**风险**：Maven 的更新策略默认按天，「每天第一次导入」可能又要上网一趟；
  jdtls 有 `java.import.maven.offline.enabled`，第 2 步实测再定要不要默认开（开了的代价：pom 加了新依赖要手动刷新一次）。
- **「第一次」慢、之后快，是 JVM 预热 + 索引懒加载**：跳转第一次 ~1 s、之后 10 ms；查找用法第一次 0.8–6 s（要搜全部 9 个项目的索引）、之后 ~65 ms。
  补全和实时报错从第一次起就快（< 60 ms、~0.45 s）。界面上的意思：**第一次跳转 / 查用法要给「正在找」的状态**，不能让人以为没按到。
- **实时报错 0.43–0.5 秒**：改完到波浪线出来不到半秒，IDEA 的手感量级。
- **内存**：`-Xmx1G` 下 RSS 到 1.1–1.5 GB（RSS 比堆大：元空间、JIT 代码缓存、线程栈都不在 `-Xmx` 里）；三次日志里没有 OutOfMemory / GC overhead。
  缓存（数据目录）petclinic 198 MB，三模块小项目 55 MB；jdtls 本体 52 MB。
- **正确性都对**：项目内跳转、跳进 jar 里的类（`jdt://` 的 class 文件）、跨 9 个项目查用法、补全里有 `findAll` 系列。

**Lombok 和兄弟模块**（shoplombok，冷启 2.0–2.1 s）：

| | 不挂 Lombok | `-javaagent:lombok-1.18.42.jar` |
|---|---|---|
| order-service 里调 `c.setName` / `c.getName()` | **2 条 error**：「The method setName(String) is undefined」 | 0 条 |
| 跳到定义 `getName`（Lombok 生成的） | 空 | `Customer.java` 第 8 行（那个字段） |
| 悬停 `getName` | 空 | `String com.demo.common.Customer.getName()` |
| 跳到定义 `Customer`（兄弟模块 shop-common 的类） | `Customer.java` 源码 | 同左 |

不挂 agent，用 Lombok 的项目**满屏假报错** —— 用户的本地仓库里有 Lombok 1.18.38–1.18.46 四个版本，他公司的项目几乎肯定用。
所以 Lombok 不是可选项：jdtls 起的时候必须带 `-javaagent`。jar 从哪来：用项目自己依赖的那个版本（本地仓库里就有、不用下载、版本和编译时一致）——
第 2 步定怎么找（读 m2e 解析出的 classpath，或者先扫 pom）。兄弟模块跳的是**源码**，不是 `target/classes` 或本地仓库里的 jar：m2e 把 reactor 里的模块当工作区项目互相引用。

**jdtls 往项目里写什么**（`git status --ignored`）：

- **默认会写** `.project` / `.classpath` / `.settings/` / `.factorypath`（Eclipse 的元数据），每个模块一套。petclinic 的 `.gitignore` 刚好忽略了它们；
  用户的项目不一定 —— 那样一开 jdtls，`git status` 里就多出几十个陌生文件。**这违反第 3 节「不打断」**。
  开关是 `-Djava.import.generatesMetadataFilesAtProjectRoot=false`，**只认 JVM 系统属性**：放进 initialize 的 settings 里实测没用，
  反汇编 `JLSFsUtils.generatesMetadataFilesAtProjectRoot()` 看到它只读 `System.getProperty`、**没设就当 true**。加上之后项目里一个都不多，全进了数据目录的 `.metadata/.plugins/org.eclipse.core.resources/.projects/`。
- **`target/classes`、`target/test-classes` 照写**：jdtls 的增量编译（Eclipse 编译器 ecj）输出到和 mvn（javac）同一个目录 —— IDEA 也是这样，`target/` 本来就被忽略。
  **待定的风险**：两个编译器写同一个目录。第 1 步起 Spring Boot 前先跑 `mvn compile`，maven-compiler-plugin 看到 class 比源码新可能说「Nothing to compile」，
  跑的就是 ecj 编出来的 class。一般没差别；`-parameters` 这类编译参数 m2e 会映射过去。第 2 步接上之后用 spring.sh 实测一遍再下结论，不提前改。

**第 2 步的参数（实测给出的建议，第 2 步开工前和用户定）**：

- `-Xmx1G` 起步：petclinic 9 个项目没压到，RSS 在 1.1–1.5 GB。真实项目更大的话看状态栏里的内存再调，设置里能改。
- 空闲收掉：热启 1.2 s 能答，收掉的代价很小 —— 第 3 节的「十分钟没有 Java 标签就收」站得住。
- 必须带的启动参数：`-Djava.import.generatesMetadataFilesAtProjectRoot=false`、Lombok 的 `-javaagent`、`-data` 指到应用数据目录下按项目分的目录。
- 第一次导入要上网核对元数据：状态栏那格要说「Java：导入中（在核对 Maven 依赖）」，不能只给一个转圈。

## 8. 第 1 步的实测：Spring Boot 怎么「照 IDEA 那样」起（2026-10-14）

> **第 1 步前一半做完了**（同日）：认出 Spring Boot 应用、在任务列表（⌃⌥R）的「Spring Boot」一组里一键起（`src-tauri/src/maven.rs`）。
> 真 .app `scripts/accept/spring.sh` 8/8：改公共模块再起不用 install 就生效、web 服务 ⌘F2 停完端口空、整组一个不剩、本地仓库里没多东西；
> petclinic 的 8 个服务按 pom 里的模块顺序全列出来。
> **后一半也做完了**（同日）：Maven 工具窗（导轨上的 Maven 图标，只有 Maven 项目才有）—— 模块树、每个模块底下认出来的 Spring Boot 应用和
> clean / compile / test / package / verify / install / 依赖树，**双击跑**，输出进运行工具窗（标签是 IDEA 的样子 `order-service [package]`）。
> spring.sh 加到 12/12：双击 package 出了 jar。
>
> **生命周期目标第一版写错了**：照 IDEA 用 `-f 模块/pom.xml`，实测 `Could not resolve dependencies … shop-common` —— 只构建这一个模块、兄弟模块要从本地仓库找。
> IDEA 用得了 `-f`，是它的 runner 带「resolve workspace artifacts」（IDEA 自己的扩展，普通 mvn 没有）。改成 `-pl 模块 -am`（`clean` 只带 `-pl`）。
> 单元测试当时把错的命令锁住了还是绿的 —— 抄 IDEA 的界面行为可以，抄它的命令行要先确认那条命令离开 IDEA 还成立。

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

## 9. 第 2 步：只读智能（2026-10-15）

> 做完：**实时报错、⌘B / ⌘点击跳到声明（含 jar 里的类）、⌥F7 查找用法**。真 .app + 真 jdtls `scripts/accept/java.sh` 17/17。
> 参数按第 7 节的建议定了（用户说「按你的思路来」）：`-Xmx1G`、闲 10 分钟收、三样必带参数。都是常量，量出不对再改。

**分层**（第 4 节那张图落地后的样子）：

| 层 | 在哪 | 管什么 |
|---|---|---|
| 线 | `crates/lspsvc/src/conn.rs` | JSON-RPC、分帧、请求配回话、超时发 `$/cancelRequest`、单条消息上限（超了读掉、把「太大」和真字节数交给等它的请求） |
| 进程 | `crates/lspsvc/src/server.rs` | 自己的进程组、stderr 只留尾巴、等待线程收尸并报崩溃、先按协议说再见再杀整组 |
| jdtls 怎么起 | `crates/lspsvc/src/jdtls.rs` | 找 JDK 21（读 `release` 文件，不起 `java -version`）、找 Lombok（本地仓库里最高的正式版）、拼参数 |
| 应用里的一份 | `src-tauri/src/java.rs` | 一个项目一个、状态转给前端、文档同步、闲了收、窗口关了收、退出时收 |
| 编辑器 | `src/lib/editor/java-lsp.ts`（跟着 Java 语言包懒加载） | 波浪线 + 悬停看原因、跳转的缓存、⌥F7 的列表 |

**几个决定和为什么**：

- **服务器问我们的，在另一条线程上答**：答在读线程上的话，读线程卡在写回话时没人排空 jdtls 的 stdout，两边互相等 —— rust.md「两个管道要并发读」那个死锁的同一个形状。测试里让答话卡 400ms，通知照样 200ms 内收到。
- **文档同步用全文**：一个 Java 文件几十 KB，本机管道上全发也就几十 KB；增量要自己把 CM6 的改动换成 UTF-16 的行列，错一个字符服务器那份就和编辑器分叉，那种错不报错、只是报错位置越来越偏。
- **initialize 之前开的文档存全文、回了再补发，不重读盘**：编辑器里没保存的才是真相（第 4 节），读盘补发的话 jdtls 分析的和屏幕上的不是同一份。
- **打开跟着编辑器、关闭跟着标签**：编辑器切标签就销毁重建（frontend.md），在销毁时 didClose 会让 jdtls 每切一次标签就丢一次这个文件的状态。
- **跳转照 java-peek 的「先缓存、回头补下划线」**：⌘hover 每动一格同步问一次，问 jdtls 是异步的。没答案时不画（「有下划线 = 一定跳得过去」是 jump.ts 唯一的承诺），答回来调 `recheck`、鼠标还停着就补上。浏览器里验过：动一次鼠标，第一刻没有、800ms 后有，期间鼠标没动。
- **jar 里的类是单独的模式 `class`**，不是只读的 `edit`：存盘、外部改动检查、换编码那几条碰盘的路都只认 `edit`，新模式天然碰不到，不用一处处加判断。只读要两样：`readOnly` 挡打字，`changeFilter` 挡程序发的事务（右键菜单的粘贴走 `replaceSelection`，`readOnly` 管不着 —— 验收第一轮就这么红的）。
- **不是 Maven / Gradle 项目不起**：随手看一眼别处的 Java 文件，不该起一个 1GB 的 JVM。
- **不下源码 jar**（`maven.downloadSources: false`）：开着的话 m2e 给每个依赖下 sources + javadoc，往用户的本地仓库里下东西没问过他；而且收尾时卡在这个任务上（实测 20 秒不退）。

**数字**（真 .app，三模块 + Lombok 的样例项目，本地仓库齐、jdtls 缓存冷）：

| | |
|---|---|
| 打开 Java 文件到编辑器出来 | 336 ms（不等语言服务器） |
| 到 jdtls 能答 | 3.07 s |
| 改错一个方法名到波浪线出来 | 912–935 ms（含 250 ms 去抖；jdtls 自己 0.43–0.5 s，第 7 节） |
| 入口包 | 155,856 → 156,117 B（+261，CI 取整 152，没到告警）：`tabflow.openClass` 一行转发、nav 认 `uri`、标签的「只读」徽章、快照滤掉 class 标签、`langOf` 认 `jdt:` |

**踩到的**（JOURNAL 同日有全过程）：

- **有源码 jar 时 jdtls 给的地址末尾是 `.java`**：`jdt://contents/spring-boot-4.0.1.jar/org.springframework.boot/SpringApplication.java?=…`。只认 `.class` 时标签名成了 `class.class`。标签名现在用地址里的文件名：有源码是 `X.java`，反编译的是 `X.class`，同 IDEA。
- **冷缓存时收尾卡在 JDT 的索引线程上**：jstack 看到 `JavaCore.stop → JobManager.shutdown` 在 join 一条收尾时新起的「Java indexing」线程。等索引建完再收（30 秒后）1.0 秒干净退出、27 个索引文件落盘。所以收尾等 3 秒、到点杀整组；只有「刚打开几秒就退出」会碰上，代价是那一次的索引没存下、下次重建。

**还没做的**（下一步再定顺序）：

- ~~应用里一键下载 jdtls~~ 第 10 节做了
- 状态栏那格的「重起」按钮（命令 `java_restart` 有了，界面没接）、问题列表（整个项目的错误）、滚动条上的错误点
- IDEA 的「Find」工具窗（⌥F7 现在开的是 Show Usages 那张列表）；跳到项目外的源码（另一个项目）
- 跳进 jar 里的类之后，在那个类里再 ⌘B
- Gradle 项目没验过（判据认 build.gradle，jdtls 的导入也开着，但没有样例跑过）
- `target/classes` 两个编译器同写（第 7 节）：还没用 spring.sh 在 jdtls 开着时验

## 10. jdtls 和 Java 21 内置（2026-10-15）

**第一版是「第一次打开 Java 文件时问要不要下」**（卡片 → 下载 → 校验 sha256 → 原子地装进应用数据目录，提交 088a8d1）。用户看了卡片：
「这个我不认可，这个应该是内置的功能」。我当时把第 3 节「正式包不变大」当硬约束，理由是看日志、记笔记的人用不到 Java ——
可这个应用的主要用户天天写 Java，为了包小让他多点一次下载，是把优先级排反了。主流也是内置：VS Code 的 Java 插件把 jdtls 和一份 JRE 都打在插件里。
下载那一版的代码删了（install.rs、卡片、java_install 命令），不留着「以后也许用得上」。

用户又选了**连 Java 21 运行时也内置**（不只 jdtls）：任何机器开箱即用，代价是再大 40–60 MB。

**怎么打进去**：

- `scripts/java-bundle.sh`：jdtls 发行包（钉死的版本 + sha256，先找 `~/Library/Caches/lite-ide-build/`，没有才从 download.eclipse.org 下）解到
  `src-tauri/java-bundle/jdtls/`（去掉用不上的 Python 启动脚本和别的系统的配置）；Java 运行时用 **jlink** 从一个 JDK 21 裁出来，一个架构一份 `runtime-<架构>/`。
  输入（版本、模块表、架构）没变就什么都不做，`app:bundle` 每次都跑它、第二次零开销。
- `src-tauri/tauri.java.json` 把这个目录当资源放进 `.app/Contents/Resources/java/`，**只在打包时**用 `--config` 叠上去：
  tauri-build 在**编译期**检查资源路径在不在，写进 `tauri.conf.json` 的话 CI 的 `cargo test`（没有这个目录）当场编不过。
- 找的次序（`java.rs` 的 `find`）：jdtls —— 设置里的 `java.jdtlsHome` → 内置的 → 都没有（开发构建）就照实说；跑它的 Java —— 内置的 → 机器上的 JDK 21+（开发构建用）。

**运行时裁哪些**（量出来的）：

| | 模块数 | 解开 | 压缩 |
|---|---|---|---|
| 全部（`ALL-MODULE-PATH`） | 69 | 91 MB | 68 MB |
| **裁过的** | 35 | **57 MB** | **39 MB** |

留的是：`java.se` 整套（项目找不到对应 JDK 时 jdtls 拿它当类库，缺一块就是假报错）；jdtls 跑起来要的（`jdk.unsupported`、`jdk.zipfs`、
`jdk.crypto.ec` —— m2e 走 https 拉依赖、`jdk.charsets` —— GBK 的源码、`jdk.management`）；第 5 步调试要的（`jdk.jdi`、`jdk.jdwp.agent`、`jdk.attach`）。
拿掉的是开发工具（调试代理、javadoc、jshell、打包工具…）。裁过的这份跑 jdtls 实测：3.4 秒能答、Lombok 不报假错、跳转都对、stderr 里没有报错。

**项目编译用的还是你机器上的 JDK**：内置的那份只跑 jdtls 自己。jdtls 自己会认机器上的 JDK —— Eclipse 的 `MacInstalledJREs` 跑 `/usr/libexec/java_home -X`，
项目按声明的版本挑对应的那个当类库，⌘B 进 `String` 是 JDK 17 带的源码（验收那条）。**一度自己配了 `java.configuration.runtimes`**：验红时把它清空，结果照样是 JDK 17 的源码，
反汇编 jdtls 才找到 `MacInstalledJREs` —— 配了等于重复，删了。

**两个踩到的**：

- jlink 的 `legal/` 里每个模块的许可证都是指向 `java.base` 的**软链**（102 个）。许可证要留（GPLv2 + Classpath Exception），软链换成实体（`cp -RL`）：打包时资源拷贝对软链的处理靠不住。
- **第二次打包就挂**：`Permission denied (os error 13)`，构建脚本那一步。jlink 产出的不少文件是只读的，tauri-build 每次把资源拷进 `target/`、第二次要覆盖上一次的 ——
  只读的盖不了。是验 `runtimes` 那条的改坏版本要重打测试包时撞上的；脚本里补 `chmod -R u+w`，之后连打两次都过。

**数字**：本机架构的 `.dmg` 7 → **98 MB**（`.app` 装好 121 MB：jdtls 52 + 运行时 58）；universal 的发版包多一份 x86_64 运行时，估 137 MB（没真打过，见下面）。
真 .app `scripts/accept/java.sh` 20/20：跑 jdtls 的是 `.app` 里的 Java（看进程的命令行）、jdtls 也是内置那份、2.6 秒能答。

**没验到的**：universal 发版那条路（`release.yml` 装两个架构的 JDK 21、x86_64 那份 jmods 交给 arm64 的 jlink）要到下一次打标签才跑；
从 `.dmg` 装、带隔离标记（quarantine）的 `.app` 里，Gatekeeper 对包里没签名的 `java` 可执行文件放不放行，也要那次真装一遍才知道。
