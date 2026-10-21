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
| 11 ★ | **HTTP 请求**（.http 文件，⌃⇧R 发 —— 原来写的「⌘↵」不对，第 19 节） | 已做（第 19 节） | 独立的小功能：解析 .http、Rust 侧发请求（做的时候换成了系统的 curl，不加依赖，第 19 节）、响应在运行窗；和日志视图联动（按 traceId 高亮这次请求的日志） | 小到中 | **要**（第 6 步） |
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
- **调试只有基本的那一套**（第 5 步，第 18 节）：Spring Boot 服务和单测调试着起、断点、条件断点、调用栈、变量、求值、单步 / 继续 / 暂停有；日志断点、异常断点、热替换、挂到远程 JVM 还没有。

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

## 11. Maven 工具窗照 IDEA 重做（2026-10-19）

用户：「maven 这个我们做的不好，idea 的 maven 啥样我们也啥样，必要的功能和展示」。第 1 步那版只有「模块摊平 + 7 个目标 + 认出来的应用」。
这次结构直接抄 intellij-community 的源码（`plugins/maven/.../navigator/structure`，不凭印象）：

| IDEA 的 | 我们 |
|---|---|
| `RootNode`：Profiles 在最前，然后是项目 | 同 |
| `ProjectNode`：Lifecycle、Plugins、Run Configurations、Dependencies，然后是子模块（**嵌着**） | Lifecycle、Plugins、Dependencies、子模块。Run Configurations 没做（我们的「运行配置」就是任务列表） |
| `LifecycleNode`：默认显示 9 个基本阶段 | 同（`maven::PHASES`） |
| `PluginsNode`：有效 POM 里的插件，展开是插件描述里的目标 | 同：`help:effective-pom` 拿插件（**不算 `pluginManagement`**），目标读本地仓库里那个插件 jar 的 `META-INF/maven/plugin.xml` |
| `DependenciesNode`：依赖树 | 同：`dependency:tree` 的输出，直接依赖在最上层、展开一层露一层 |
| 工具栏 `Maven.NavigatorActionsToolbar` | 重新加载、执行 Maven 目标、运行选中的、离线、跳过测试、全部展开、全部收起。下载源码、依赖分析图、设置没做 |

几个决定：

- **插件和依赖一次 mvn 算两样**：JVM 起来、读 pom、解析 reactor 是大头，分两次就付两次。`-q`、两样都写文件不走 stdout。打开工具窗时在后台跑（三模块 2.4 秒），别的节点不等它；结果按项目根记在模块里，切走再回来不重跑。
- **目标的白名单换成「按字符卡」**：插件目标、自己敲的命令都要能跑，7 个词的白名单不够了。每个词只许 `[A-Za-z0-9_-.:=,@/+%]` —— 没有空格、引号、`;` `$` `|` `&`，拼不出第二条命令（Rust、桩、前端三处都卡）。
- **开关按项目记在应用数据目录**（`maven-prefs.json`），**跑目标和起 Spring Boot 服务都带上**：勾了 profile、起服务却没带，是最难查的那种「怎么跟 IDEA 里跑的不一样」。
- **Spring Boot 应用挪出 Maven 窗**（IDEA 的 Maven 窗里没有）—— 下一步 Services 工具窗接住它们。
- 图标 12 个从 intellij-community 取（`public/icons/SOURCES.md` 逐个记了上游路径；节点用的是 IDEA 新 UI 映射过去的那一套）。

**真 .app `scripts/accept/spring.sh` 19/19**：子模块嵌在 shopdemo 底下、Profiles 里有 probe、9 个阶段、Plugins 有 compiler / surefire **没有 spring-boot**
（父 pom 只在 pluginManagement 里给了默认值，模块没声明就不用 —— IDEA 也不列）、插件目标是读 jar 的、依赖里有兄弟模块；勾 probe + 开跳过测试之后双击 package，
命令是 `mvn -B -ntp -P probe -DskipTests -pl order-service -am package`、出了 jar。

## 12. Services 工具窗（2026-10-19）

用户：「服务启动这块，应该有个 service 的 tab 在侧边栏的下边，当识别到服务的时候」。第 11 节把 Spring Boot 应用挪出了 Maven 窗，这里接住。
分组照 intellij-community 的 `RunDashboardRunConfigurationStatus`（不凭印象）：

```
Spring Boot                     ┃ 选中那个服务的输出（就是运行窗那一格：级别着色、过滤、跟随、堆栈跳源码）
  Running                       ┃
    ● UserApplication   :8081   ┃
  Finished                      ┃
    ● OrderApplication          ┃
  Not Started                   ┃
    ○ AdminApplication          ┃
```

| IDEA 的 | 我们 |
|---|---|
| 底部工具窗，Spring Boot 配置自动进来 | 同：导轨下面那组（终端 / Git / 运行旁边）一个按钮，**认出了服务才出现**（ui.md 第十条「有内容才出现」） |
| 状态分组 Running(10) → Failed(20) → Finished(30) → Not Started(40)，空组不画 | 同 |
| 服务后面印端口 | 同：`lsof -a -g <进程组> -iTCP -sTCP:LISTEN`，查的是这次运行那一组进程在听的，不是「8080 上有没有人」 |
| 工具栏 Run / Debug / Stop 作用在选中的节点上，选中分组 = 整组 | Run / Stop 同；Debug 等第 5 步 |
| Spring Boot 的运行不进 Run 工具窗 | 同：服务的运行记录带 `service`，运行窗的标签、导轨角标、⌃R 的当前格都不算它 |
| Ultimate 的 Actuator 页（Beans、Mappings、Health） | 没做：要应用带 actuator，第 6 步 HTTP 请求之后再看 |

几个决定：

- **认服务和查端口分成两条命令。** `services_list` 要把每个模块的 `src/main/java` 翻一遍找 `@SpringBootApplication`（`maven::boot_apps`），
  开项目时问一次、Services 开着时有服务换了状态再问；端口是 `task_ports`（只跑一次 lsof），只在「跑着、还没端口」时隔 1.5 秒问一次、最多一分钟 ——
  Spring Boot 起来要几秒；一分钟还没端口的多半不是 web 服务或者起不来，那时候看日志比看端口有用。第一版拿 `services_list` 轮询，一秒翻一遍源码，改掉了。
- **⌘F2 停哪个**：Services 开着就是那边选中的服务，否则运行窗的当前格；那一个没在跑、而整个窗口只有一个在跑的，就停那一个。
  不这么做的话，服务不进运行窗的标签，Services 收起时 ⌘F2 永远停不到它（IDEA 只有一个在跑时也是直接停）。
- **刚起来的服务自动选中**（从任务列表、⌃R、Services 里起的都算）：起它就是要看它在说什么。
- **入口包 +330 B**（156,333 → 156,663，离告警线 8 B）：Services 本体、端口轮询、⌘F2 的取舍全在懒块里；入口里是导轨按钮、面板头的名字、懒加载壳。
  第一版 +848 B 越过告警线，挪了三处：认服务并进已有的 `checkStale`（首屏之后那次 `import()`，不另开一处）、导轨上「运行」「Services」两个按钮一个模板（−284 B）、
  面板里五处一样的「正在载入…」收成一个 snippet。

没做的：Debug（第 5 步）、Spring Boot 以外的服务类型（Docker、数据库）、Actuator 页、左右两栏之间拖宽度。

**真 .app `scripts/accept/spring.sh` 24/24**（新加 5 条）：起 UserApplication 亮的是 Services 不是运行窗；它在 Running、端口是 lsof 从它那个进程组查到的 `:58567`；⌘F2 停的是 Services 里选中的它（端口空了、组里一个不剩）、停完挪进 Finished；在 Services 里双击再起、点工具栏的停止，同样停干净。运行窗的标签里只有 `order-service [package]`，服务没混进去。状态层 `runs.state.test.ts` 新加 10 条，6 种改坏的版本都红。

## 13. 第 3 步之一：补全（2026-10-19）

先用探针（`lspsvc/examples/probe.rs`，自造的小项目，不碰你的项目）看 jdtls 的原样回包，再定怎么接。三件量出来的事：

| | 实测 | 决定了什么 |
|---|---|---|
| 方法项插什么 | `insertText` 只有名字 `add`；**真要插的在 `textEdit.newText`**：`add(${0});`（`guessMethodArguments` 开着是 `add(${1:name}, ${2:count});`） | 插 `textEdit`，按 snippet 走（光标停在括号里）。默认不猜参数 —— 光标落在空括号里，和 IDEA 一样。之前记的「要客户端自己补括号」是看错了字段 |
| 自动 import 在哪 | 补全列表里**没有**；只在 `completionItem/resolve` 的 `additionalTextEdits` 里给 | 要 resolve，而 resolve 慢（下一行） |
| 多快 | 补全 59–66 ms（`.` 后面 47 项 232 ms）；resolve 137–254 ms | 回车时不能等 resolve：选中项高亮时就在后台问（CM6 要说明框的时候本来就会来要），回车时手上有就一次做完；没有就先插正文、import 回来再补到上面 |

几个决定：

- **问的时候带着全文。** 前端改动去抖 250ms 才发，刚敲的那个字 jdtls 还没看到。`java_complete` 先把这份文本作为一次改动发过去、再在同一根管道上问 —— jdtls 按顺序处理，问到的是新文本。
- **只在第一个字、`.`、`@` 后面问**，之后接着敲在已有的里面筛（CM6 的 `validFor`）；jdtls 说它截了（`isIncomplete`）才每个字都问。
- **jdtls 的排序要保住**：CM6 先按「和敲的字的匹配度」排，jdtls 按「类型对不对、离得近不近」排。按名次给 ±20 的加分，只够在匹配度相近时排出先后。
- **import 只收补全那一行上面的**：它们是在补全前那份文本上算的行列，回来之前人可能接着敲了 —— 敲在光标处，上面的行没动。
- jdtls 没起好、或者不是 Maven / Gradle 项目：退回「文档里出现过的词」（原来的行为），补全框不会在 Java 文件里整个没了。
- 一次最多 200 项（`java::COMPL_MAX`）、说明文字最多 3000 字（JDK 的 Javadoc 动辄几千字），截了照实标。

**真 .app `scripts/accept/java.sh` 24/24**（新加 4 条）：敲 `ArrayLi` 到候选出来 450–472 ms（含 CM6 敲字后等的 100 ms）；选中后那一行是 `ArrayList`、文件顶上多了
`import java.util.ArrayList;`；`c.getNa` 补出 Lombok 生成的 `getName`，插的是 `getName()`。去掉「接受时去 resolve」的错版本，import 那条红。

还没做（第 3 步剩下的）：参数提示（敲 `(` 之后浮出参数表）、改名、重构、快速修复、格式化。

## 14. 第 3 步之二：改名（⇧F6，2026-10-19）

探针先看 jdtls 回什么（`RENAME=旧=新`），三件事决定了接法：

| | 实测 | 决定了什么 |
|---|---|---|
| 改 public 类名时文件跟不跟着改名 | 只声明支持 `rename` 一种文件操作时，jdtls **只改类名、不改文件名**（类名和文件名对不上，编译不过）；**建 / 改名 / 删三种都声明**才回 `documentChanges` 里那条 `Shop.java → Store.java` | 三种都声明（`jdtls::capabilities`）；回来的「新建 / 删除」这一版整个拒绝、一处都不改 |
| 多慢 | 方法改名第一次 4.3 s（要搜全部索引），之后一两百毫秒；类改名 160 ms | ↵ 之后框立刻关，「正在改名」去状态栏的进度格，不让一个框挂着等 |
| 回包形状 | `changes` 是空的、`documentChanges` 里先改两个文件的文字、最后一条改名 | 两种都认；先改全部内容、最后改名（改名之后又改新路径的，换回原路径） |

**落盘复用跨文件替换**（docs/REPLACE.md）：执行时每个文件重读、核对指纹，开着的文件改进编辑器（有没存改动的只改编辑器），整次一条撤销日志、两段提交。
为它加了三样：每一处命中可以自带新文字（`Hit.repl`）、按「行列 + 新文字」建扫描结果（`scan_edits`，列按 UTF-16）、撤销日志记下改过的文件名（`Manifest.renames`，老日志没有这一栏按空的读）。
次序是 `java::apply_rename` 存在的理由：

1. **先查改名做不做得了**（新名字被占、跨目录）—— 内容改完了名字却改不了，就是编译错误
2. **有一个文件改不了就整个不做**（有损编码、混用换行符、位置对不上）：查找替换跳过一个文件是常态，改名少改一处就编译不过
3. 两段提交改内容；执行时被跳过的 → 撤回去、报错
4. 最后改名；改名失败 → 内容也撤回去

撤销：**先把名字改回去、再还原内容**（日志里记的是改名前的路径）。开着的标签 Rust 按文件身份（inode）认，报回标签此刻的路径，前端在那儿放改动、再把标签挪回原名。

**盘上的改动要告诉 jdtls**（`workspace/didChangeWatchedFiles`）。第一版只在改名后补了一句通知 —— 撤销、查找替换、切分支、别的编辑器写的它全不知道，
索引一直是旧的。改成**从文件监听这一处来**：每个 jdtls 实例挂一个只认 `.java` 和构建文件的监听（`fsservice::watch::watch_paths`，防抖同文件树那个；
一批最多 2000 个，超了记 WARN），新建 / 改了 / 没了按此刻盘上在不在判，不信事件说的类型。开着的文件 jdtls 以编辑器那份为准，说了也不会盖掉它。

**真 .app `scripts/accept/java.sh` 32/32**（新加 8 条）：⇧F6 预填 Money；record 跨模块改成 Cash，`Money.java` 改名成 `Cash.java`（331 ms），order-service 的引用和 import 都改了；
jdtls 不报错（**去掉「告诉 jdtls」的错版本这条红：`Cash` 找不到**）；点撤销名字和内容一字不差地回来；改成已有的 `Customer` 一个字都不动、说「Customer.java 已经有了」
（这条是 jdtls 自己先拒绝的，原话英文，换成了中文）。

还没做：IDEA 那种在代码里原地改（所有用到的地方一起变色）—— 现在是挂在名字底下的小框；改名之前的预览（IDEA 的 Refactoring Preview）；跨目录的移动。

## 15. 第 3 步之三：格式化（⌥⌘L，2026-10-19）

Java 文件里 ⌥⌘L 交给 jdtls（IDEA 的 Reformat Code）：有选区就只排选中的那几行（`rangeFormatting`），没有就整份。别的文件里照旧是「格式化 JSON」（键位表里同一行，说明写了两种）。

- **缩进照编辑器此刻的设置给它**（状态栏能改）。不给的话 jdtls 用 Eclipse 的默认 —— Tab 缩进，和文件现在的缩进对不上就是满屏改动。探针实测：给了「4 个空格」它就照着排（118 ms）。
  错版本（不传编辑器的设置）在验收里红：排出来是 Tab。
- 回来的是几十处很小的改动（补一个空格、换一行），**一笔事务**放进去：⌘Z 一下全撤，光标跟着改动挪。
- 问的时候带着全文；回来时文件又被改过（等的时候接着敲了）就不放、说一句 —— 那些位置是在旧文本上算的。

**真 .app `java.sh` 35/35**（新加 3 条）：整份排成 4 个空格缩进、`=` 两边有空格、一个 Tab 都没有；⌘Z 一下全回去；选中一行只排那一行。

还没做：存盘时自动格式化、整理 import（IDEA 的 ⌃⌥O）、项目自己的格式化配置（`.editorconfig` 之外的 Eclipse 格式化 XML）。

## 16. 第 3 步之四：快速修复（⌥↵，2026-10-19）

IDEA 的 Show Context Actions：光标那儿（或选中的几行）能做的修复挂一张列表，↑↓ 选、↵ 做。探针摸清了三件事：

| | 实测 | 决定了什么 |
|---|---|---|
| 不声明支持「字面量形式的修复」 | jdtls 一项都不回 | 声明 `codeActionLiteralSupport` + `resolveSupport`（先给标题、选中了再要改动，resolve 3 ms） |
| 按工作区构建出的第一批报错问 | 那批报错**只有行号、列是 0**，jdtls 按它的位置找要修的东西，找不到 `List` —— 只给「加 Javadoc」「整理 import」，没有「Import 'List'」 | 报错要用打开文件之后实时校验那一轮的（位置准）。Rust 按文件存 jdtls 最近发来的**原样**报错（带 `code`，jdtls 按它找修复；前端画波浪线那份没带），问的时候挑出和那几行重叠的交回去。**去掉报错的错版本在验收里红：只剩加 Javadoc 那几条** |
| 候选的次序 | jdtls 按字母：`com.sun.tools.javac.util`、`java.awt`、`java.util`（真 .app 验收第一次第一条是 `java.awt.List`） | 修报错的在前；import 候选按包分三档：`java.*` / `javax.*` → 项目和第三方 → JDK 内部的和桌面图形界面的（`com.sun`、`sun`、`jdk`、`java.awt`、`javax.swing`）。IDEA 靠使用统计，我们没有统计 |

- **只改当前文件的修复**（补 import、改名字、加修饰符）放进编辑器、一笔事务：⌘Z 能撤，不写盘。**改到别的文件的**照改名那套落盘（第 14 节），带撤销卡片。
- **不列要弹交互框的**（`source.generate.*`：VS Code 里是让人勾字段的对话框）；**要新建 / 删除文件的**（Create class）选了说「这一版还不支持」，一处都不改。
- 问的时候带着全文；改动回来时文件又改过了就不放，说一句。

**真 .app `java.sh` 38/38**（新加 3 条）：缺 import 的 `List` 按 ⌥↵，第一条是 `Import 'List' (java.util)`、不列生成 getter；回车补上 import、波浪线消失；⌘Z 一下撤回。

还没做：IDEA 那个灯泡图标（光标停在有修复的地方时行首亮一下）、新建类的修复、生成 getter / 构造器的交互框、参数提示。

## 17. 第 4 步：跑单测（2026-10-19）

照第 2 节第 10 条定的：**不靠语言服务器**。测试就是 `mvn -Dtest=类#方法 test`，跑在任务里；结果是 surefire 自己写的报告。

| IDEA 的 | 我们 |
|---|---|
| 测试类、测试方法旁的绿三角 | 同：`src/test/java/` 底下的文件，按注解认（`@Test` / `@ParameterizedTest` / `@RepeatedTest` / `@TestFactory` / `@TestTemplate`，JUnit 4 和 5 都是这些名字；`java-tests.ts`）。jdtls 没起好也能跑。认不出自己写的元注解（IDEA 认得：它有完整的语义模型） |
| 跑完绿三角换成 ✓ / ✗ | 同（`run/test-outcomes.ts`：运行那边写、编辑器那边读，两边都是懒加载的块，这张表单独一个小文件） |
| ⌃⇧R 跑光标所在的 | 同：光标往上最近的测试方法；在第一个测试方法上面就是整个类 |
| Run 窗口的测试视图：左边结果树、右边输出 | 同：运行窗里测试那一格左边是结果树（类 → 方法，耗时），右边是日志视图那套输出；点没过的那条展开原因和堆栈，双击跳到方法 |
| 重跑没过的 | 同：surefire 的选择器 `A#x+y,B#z` |

- **任务名用 Maven 自己的写法**：`order-service [test com.demo.OrderTest#adds]`，和 Maven 工具窗跑目标的「模块 [目标]」同一个形状 —— 运行窗、⌃R 重跑、⌘F2 停、同名先停旧的全是现成的。模块那一段认 artifactId、认目录（行号旁的 ▶ 从文件路径就算得出，不用先问 Rust），根模块写 `.`。
- **命令**：`mvn -Dtest=… -Dsurefire.failIfNoSpecifiedTests=false -pl 模块 -am test`。`-am` 让兄弟模块不用 install（同跑目标）；它也会在上游模块里跑 test 阶段，那里没有这个类，surefire 默认当失败、整个构建停下 —— 所以要那个开关（**去掉它的错版本在验收里红**）。带 Maven 窗里勾的 profile 和离线，**不带跳过测试**（就是来跑测试的）。选择器按字符卡（`[A-Za-z0-9_.#$*+,]`），`$` 和 `#` 进命令时加引号。
- **结果只认这次的**：`target/surefire-reports/TEST-*.xml` 里改动时间不早于这次开始的那些。上次跑过、这次没跑到的类的报告还躺在那儿，算进来就是一份混着上次结果的东西。
- 报告不为它加 XML 依赖：只认 `testcase` 和底下的 `failure` / `error` / `skipped`，属性按「前面是空白」认名字（不然 `classname` 会被当成 `name`）、正文按实体解码、CDATA 原样。堆栈最多 1.6 万字、一次最多 5000 条，截了照实说。

**真 .app `scripts/accept/spring.sh` 29/29**（新加 5 条，真 mvn）：测试类和两个方法旁各一个 ▶；⌃⇧R 跑整个类，用到兄弟模块的那个过了（shop-common 没 install）、故意的那个没过；
命令带 profile、不带跳过测试；行号旁换成 ✓ / ✗；重跑没过的只跑那一个。
样例的根 pom 把 `junit-jupiter.version` 钉在 5.12.2：Spring Boot 4.0.1 管的是 JUnit 6.0.1，本地仓库里配套的 `junit-platform-launcher` 只有 1.x，用 6 的话 mvn 要去下载。

还没做：Gradle 项目（`gradle test --tests`，报告在 `build/test-results/`）、`@Nested` 内部类、跑的时候一条条地亮（现在是跑完一起出 —— 要接 surefire 的事件流）、按覆盖率着色。

## 18. 第 5 步：断点调试（2026-10-20）

照第 4 节搭的：`crates/dapsvc` 说调试适配器协议（DAP），适配器是 jdtls 的 java-debug 插件（VS Code 的 Java 调试用的同一个）。
插件 0.53.1 从 Maven Central 下（用户点头的，sha256 钉在 `scripts/java-bundle.sh`），作为 bundle 在 jdtls 起来时交给它 —— 不另起进程。

```text
Services 的虫子 / ⌃D
  → task_run(debug) 命令里 `exec java` 后面插 -agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0
  → JVM 起来先停在入口等调试器，输出里打 "Listening for transport dt_socket at address: 56050"
  → debug.rs 盯着任务的输出文件认出端口 → 等 jdtls 导入完 → vscode.java.startDebugSession（插件开一个 DAP 端口）
  → initialize / attach 56050 / setBreakpoints / configurationDone（JVM 接着跑）
  → stopped 事件：拿调用栈，整份推给前端（"debug-state"）；变量、单步、求值走命令
```

| IDEA 的 | 我们 |
|---|---|
| Debug 起一个运行配置 | Services 工具栏 / 右键的「调试」，⌃D 调试最近跑过的那个（终端里的 ⌃D 是输入结束，让给 shell，同 ⌃R）。只有认出来的 Spring Boot 应用：命令是我们拼的，知道 `java` 在哪 |
| 断点画在行号上（新 UI 的默认） | 同：点行号下 / 取消，⌘F8。跟着行走（上面插三行，断点跟着下移）、存进 localStorage（重启还在） |
| 条件断点：右键断点填 Condition | 同：右键行号出一个小框，填 Java 表达式、回车（Esc 不改、清空变回普通断点、点外面算确定）。条件跟着断点走，红点里多一个「?」；调试开着时改了当场下到 JVM 里 |
| 条件算不了（写错了）：停下、弹一句为什么 | 同：java-debug 照样停，先发一个它自己的 `usernotification` 事件 —— 我们把那句挂在这次停下上，调试器那行琥珀色写「条件 … 算不了，照样停下了：编译不过：…」 |
| 停下来：执行点整行标蓝、编辑区跳过去、Debug 窗跳到前面 | 同。jar 里的帧开成只读标签（⌘B 进 jar 用的同一条路），lambda 生成的类没有源码，灰着、点了说一句 |
| Debug 窗：Threads & Variables + Console 两页 | 同：运行那一格变成「调试器 / 控制台」两页，撞上断点自动切到调试器；控制台就是原来那份输出。两页只藏不卸（输出滚到哪、变量展开到哪都还在） |
| 变量的值后面直接是 `toString()` | 同：java-debug 对重写了 `toString` 的对象回「懒的」（值要再问一次），Rust 那边替前端先问好（一层最多 50 个） |
| 单步之后展开着的照样展开 | 同：按名字路径记，最多往下四层 |
| F7 / F8 / ⇧F8 / F9、Evaluate | 同键位；求值是变量那一栏顶上的输入框。多个线程停着可以换着看 |

几条为什么：

- **`suspend=y`**：不等调试器就跑的话，`main` 里、`@PostConstruct`、`CommandLineRunner` 的断点永远停不住 —— 挂上去之前它们早跑过了。IDEA 的 Debug 也是这么起的。
  代价是调试器挂不上的话 JVM 一直停在入口，所以**挂不上就把任务停掉、照实说为什么**（jdtls 起不来、开发构建没带插件、导入 10 分钟没完）。
- **端口 0**：系统挑一个空的，JVM 把真端口打在输出里（JDK 17 实测）。自己先找一个空端口再交给它，找到和用上之间会被别人占。JDK 8 没试过。
- **attach 时带 `projectName`**：不带的话求值报「please specify projectName」（探针实测）。项目名问 jdtls（`resolveMainClass`，m2e 的项目名，一般是 artifactId）。
- **事件不在 DAP 的读线程里处理**：`stopped` 之后要问调用栈，在读线程里问的话，等的那个回答要读线程自己去读，永远等不到。事件进通道，会话自己的线程一条条处理。
  适配器反过来问我们的（`runInTerminal`），另起一条线程答「不支持」—— 同 lspsvc 的理由。
- **调试期间攥着 jdtls**：插件跑在 jdtls 进程里，「没开着的 Java 文件、闲了十分钟」的回收不能把它收掉。
- **菜单里单步那几项没在调试时是灰的**：灰的菜单项不吃键，终端里的 htop、mc 照样收得到 F7 F8 F9。灰不灰由 Rust 的会话直接设，前端入口包一个字节不多。

实测（探针 `crates/dapsvc/examples/probe.rs`，三模块样例，热缓存）：jdtls 就绪之后起会话 21 ms、挂上 16 ms、下断点 170 ms，从 configurationDone 到停在兄弟模块的断点上 360 ms（Spring 启动那段）；
调用栈 14 ms（24 帧）、求值 31 ms。真 .app 上从点虫子到停在断点 3–6 秒，其中大头是 `mvn compile`。
带上插件之后 jdtls 起来要多久：热缓存三次里不带 1262 / 1326 ms、带 1293 / 1468 / 1399 ms —— 差在 0.1 秒上下，和同一种配置两次之间的抖动一个量级（插件在 jdtls 启动时登记、真调试时才激活）。
正式包多 2.8 MB（那个 jar）。被调试的 JVM 和 jdtls 的 JVM 版本不一样时（17 和 21）插件会说一句「source mapping 可能不对」，记进 app.log，不打扰人。

**真 .app `scripts/accept/spring.sh` 42/42**（加条件断点之后 47/47，见下）（新加 13 条，真 mvn / java / jdtls / java-debug）：断点画在行号上；点虫子之后停在 shop-common 的 `Money.java:4`（没 install），调用者是入口类；
Services 那一行写着「停在断点」、编辑区第 4 行标成执行点；`this = Money@21 "Money[cents=12345]"`（带着 toString）；求值 `cents / 100 + 1 = 124`；F8 停在第 5 行、`s = "CNY 123.45"` 出现；
F9 之后程序照常跑完、会话结束、执行点清掉。

**条件断点**（同日补上）：条件由 java-debug 在被调试的 JVM 里算，前端只管存、画、一路带进 `setBreakpoints` 的 `condition`。存的格式兼容第一版（普通断点还是一个行号数字）。
spring.sh 新加 5 条：带条件的画成另一个样子；`cents == 1` 跑过不停、照常跑完；`cents == 12345` 停在 `Money.java:4`；`nosuch == 1`（写错了）照样停、说清为什么；那句是琥珀色。
**写错了照样停**是插件的行为（IDEA 也这样：宁可多停，不漏停），没有那句提醒的话看着和「条件成立」一样 —— 第一次在真 JVM 上探出来的（JOURNAL 同日）。

**调试单测**（同日）：测试旁的 ▶ 点了出一个小菜单「运行 ⌃⇧R / 调试 ⌃⇧D」（同 IDEA：点了不直接跑），⌃⇧D 调试光标所在的那个。

| 问题 | 答案 |
|---|---|
| 跑测试的是哪个 JVM | surefire **另外 fork 的**，`mvn` 自己不是。JDWP 交给 surefire 的 `-Dmaven.surefire.debug=<同一段 JDWP>`（原样加在 fork 的 JVM 参数后面，不盖 pom 里的 `argLine`）。没走「自己起 JVM 跑 JUnit」：那样和「运行」、和 CI 不是同一条路 |
| 端口在哪 | 端口 0，但**不能读输出**：fork 的 JVM 确实打了那一行，可 surefire 要等那个 JVM 跑起来才把它的输出转给 mvn —— 而 `suspend=y` 的 JVM 在等调试器，调试器在等那一行，互相等（第一版就这么卡死了 300 秒；之前的实验用的是 `suspend=n`，正好绕开）。fork 的 JVM 和 mvn 在同一个进程组里：问系统这个组里谁在听（`lsof -g`），认命令行里**有一个参数本身**是 `-agentlib:jdwp=…` 的那个 —— mvn 自己的命令行里也带着这串（`-Dmaven.surefire.debug=-agentlib:jdwp=…`），只看「包不包含」会认成 mvn |
| `-am` 会不会让上游模块也起一个等调试器的 JVM | 不会：上游模块里的测试类不匹配 `-Dtest`，surefire 根本不 fork（实测：shop-common 加了一个测试类，输出里只有一个端口） |
| 一次运行不止一个 JVM（`forkCount > 1`、重跑没过的跨了几个类） | 每个都停在入口等调试器，没人挂就永远卡着。所以一个 JVM 结束、任务还在，就接着等**还没挂过的**下一个（按 pid 记）、再挂上去；挂上过之后任务退了算正常结束 |
| 求值按哪个项目编译 | 测试类没有 main，`resolveMainClass` 问不出来 —— 用模块的 artifactId（m2e 默认的项目名）。spring.sh 里求值 `new Money(7).cents() + 1` 用得到兄弟模块的类，证明对 |
| 调试器 / 控制台两页 | 控制台那页就是不调试时的样子（结果树 + 输出），同 IDEA 的 Debug 窗；调试结束自动回到这页 —— 跑完要看的是结果 |

还没做：日志断点（插件支持，缺的是界面）、命中次数、异常断点、热替换（改了代码不重启）、断点列表（⌘⇧F8）、「这一行停不住」的断点画成另一个样子、挂到别处起的 JVM（远程调试）、Gradle 项目。
多窗口时菜单的灰态是全应用一份：哪个窗口的会话停着都亮，按下去前台窗口找它自己的会话，别的窗口的不动。

## 19. 第 6 步：HTTP 请求（2026-10-20）

`.http` 文件照 IDEA 的 HTTP Client 格式：`###` 分开请求（后面的字是名字，`# @name` 也行）、`@变量 = 值`、`{{变量}}`、`< ./body.json`、`# @no-redirect`；
环境文件 `http-client.env.json` / `http-client.private.env.json` 从 `.http` 所在目录往上到项目根找，近的盖远的、私有的盖公开的、`$shared` 是底子，`{{a.b}}` 按路径取（IDEA 2024.2 起的写法）。

| IDEA 的 | 我们 |
|---|---|
| 请求行旁的绿三角 | 同 |
| 「运行光标这儿的」 | **⌃⇧R**（第 2 节表里写的「⌘↵」是我凭印象写的，不对：IDEA 里每个请求是一个临时运行配置，发它就是通用的 Run context，macOS 上 ⌃⇧R —— 和测试文件里跑光标所在的测试是同一个键） |
| 编辑器顶上的「Run with: 环境」 | 同：顶上一条「运行环境」，按项目记住选的哪个 |
| 响应出现在 Services 工具窗 | 出现在运行工具窗（一个请求一格，同名的原地换掉，同任务）：状态码按段上色（2xx 绿、4xx 琥珀、5xx 红）、耗时、大小、环境；响应体 / 响应头两页；4xx / 5xx 那一格的点是红的 |
| 响应存进 `.idea/httpRequests/` | **不往项目里写**：存在应用数据目录 `http/`，留最近 50 个 |
| 响应处理脚本（`> {% %}`） | **不跑**：那是在应用里执行文件里带来的 JavaScript。认出来、从请求体里摘掉、在那一格里说一句 |

**发请求交给系统的 curl**（`crates/httpsvc`，经 procutil 起），不自己带 HTTP 库 —— 第 2 节写的是「要加 HTTP 依赖，先问」，换成 curl 就不用加了，而且更对：
走系统的证书库和代理环境变量（公司内网的 CA、`HTTPS_PROXY`，和终端里的 curl 一样），计时、重定向、gzip 都现成，`-w '%{json}'` 一次给全统计。
交给 curl 的东西要安全，`.http` 是项目里带来的文件：

- `-q` 放第一个：不读 `~/.curlrc`（验红：去掉它，测试里放的 `.curlrc` 往请求里加了一个头）
- 只许 http / https：第一跳 `validate` 卡；`--proto` 和 `--proto-redir` 管重定向（两道是重复的：只去掉一个照样拦得住，两个都去掉，重定向去 `ftp://` 那条才红 —— 第一版拿 `file://` 测，curl 默认就不跟，是一条空断言）
- 方法、头名按 token 卡，头的值不许换行（一个头拆成两个），全走 `Command::arg`
- 请求体先写进我们自己的临时文件再 `--data-binary @文件`；没写 `Content-Type` 的不让 curl 自作主张加表单的那个
- **`< 文件` 只许是项目里的**（2026-10-20 code review 补的，IDEA 不拦）：`.http` 是项目带来的，一句 `< ../../.ssh/id_rsa` 加一个外面的地址，点一下 ▶ 私钥就发出去了。
  按解开软链之后的真路径比（项目里放一个指到外面的软链也不行）；项目根解析不出来时不发（加固出错往「不做」那边倒）。要发就把文件放进项目里
- 不设总超时：调试的时候请求停在断点上几分钟是常事（第 18 节）。要停就取消（运行窗的停止 / ⌘F2 / 关掉那一格 / 关窗口）：按 pid 发 SIGKILL，pid 在收尸之前摘掉 —— 没收尸之前它还是我们的僵尸，不会被系统分给别人
- 响应体最多收 32 MB，超了截断、照实说收到了多少；8 MB 以内的 JSON 重新缩进，**不经过解析成对象**（serde_json 默认按键名排序、会改写数字），按原样的次序和数字排

有没定义的变量就**不发**，把名字列出来、说清是没选环境还是环境里没有 —— 带着 `{{host}}` 发出去，回来的错只会让人往服务端去查。

着色另写了一个（`editor/http-lang.ts`）：原来用的 legacy-modes 的 `http` 认的是一条原样的报文，`@变量`、`###` 在它眼里全是错，整行标红（浏览器里看出来的）。

**真 .app `scripts/accept/http.sh` 16/16**（真 curl，对面是脚本自己写的 python 回显服务）：没选环境不发（服务端收到 0 个）；POST 的头和体逐字对上（私有环境的 token 盖掉公开的、`$shared` 里的对象按路径取、`{{$uuid}}` 换成真的）；
响应体按原样的次序排好、存在应用数据目录；`< 文件`、跟重定向、`@no-redirect`、418 标红、3 MB 照收；慢请求点取消 272 ms 停下、curl 进程没了；项目里一个文件都没多。

还没做：Cookie 罐（IDEA 跨请求记 Cookie）、multipart、响应处理脚本、请求历史（IDEA 的 `http-requests-log.http`）、按 traceId 联到日志视图（第 2 节表里写的，需要先约定 traceId 在哪个头）、响应体的 JSON 着色（现在是日志视图那套，有搜索和过滤，没有语法颜色；「在编辑区打开」有）。

## 20. 第 3 步补上的：参数提示（2026-10-20）

敲 `(` 或 `,` 时在光标上方列出这个方法的几个重载、当前参数加粗（IDEA 的 Parameter Info），出了括号收起；⇧⌘Space 手动弹。

- **jdtls 默认关着**：`java.signatureHelp.enabled` 不开，`signatureHelp` 一个签名都不回（探针实测 `{"signatures":[]}`），开了第一次 168 ms。
- **参数的位置自己找**：声明了 `labelOffsetSupport` 它照样给参数的文字，在签名里**从括号后面按顺序往后找** —— 不按顺序的话 `copy(int from, int to)` 两个 `int` 都亮第一个（错版本验红）；
  给偏移的服务器按 UTF-16 下标换成字符下标（前面有 emoji 时不换算会错位，错版本验红；拿中文测不出来，中文在 UTF-16 里也是一个单位）。
- **不自己数括号**判断「还在不在调用里」：开着时每次改动 / 挪光标都重新问（去抖 150ms），jdtls 回空就收 —— 字符串、注释、泛型的尖括号它本来就认得。
- **⌘P 不是它**：IDEA 在 macOS 上的 Parameter Info 是 ⌘P，这个应用的 ⌘P 是按文件名打开（Sublime / VS Code 的习惯）。要不要让出来是用户定的事，先用 VS Code 的 ⇧⌘Space。

真 .app `scripts/accept/java.sh` 41/41（新加 3 条）：敲 `String.format(` 弹出两个重载、当前参数 `String format`；打完 `"%d", ` 挪到 `Object... args`；打完 `1);` 收起。

## 21. 本地历史（2026-10-20，第 0 节排期里「穿插」的那一项）

IDEA 的 Local History：不提交也能回到十分钟前那一版。菜单「文件 › 本地历史…」或者编辑器右键「本地历史…」，左边版本、右边「那一版 → 现在」的差异，「恢复成这一版」。

- **什么时候存**：每次保存（⌘S、自动保存）存「存下去的那份」；存之前盘上那份和上一版不一样（第一次在这儿保存、git 切了分支、别的编辑器改过）就先补存一版 ——
  最早那条标「改之前」、夹在中间的标「外部改动」。不另外去盯文件变化：下一次保存时补上就够，而且不会在 git 切分支时一口气记几千个文件。
- **存盘上的原始字节**，读出来按文件的编码解（和打开文件同一条路）。存编辑器里的文字的话，GBK / CRLF 的文件每次都和盘上不一样，被误判成外部改动。
- **存在应用数据目录**（`history/`，一个文件一个子目录，同样的内容只存一份），不往项目里写；超过 4 MB 的不存，每个文件最多 200 版、30 天。
  记不成（盘满、没权限）只记 WARN，**保存照常成功**：附属功能不能堵主路。
- **哈希自己写 FNV-1a**：std 的 `DefaultHasher` 不保证跨 Rust 版本稳定（换工具链之后去重失效），也不为它加库；读回来核对字节数，撞了不拿错的内容出去。
- **恢复是编辑器里的一笔修改**（⌘Z 撤得回、还没保存），不直接写盘。为此加了一张很小的「路径 → 编辑器」登记表（`editor/live-views.ts`，跟着编辑器那个懒块走）。
- **差异复用差异视图**：`linediff.ts` 用已有的 Myers 生成一份 git 格式的 unified diff 交给 `DiffView`，双栏、着色、折叠都现成。
  测试是「生成的补丁打回旧的必须得到新的」，随机 300 组 + 一组手算块头。
- 面板挂在 body 上，不进 overlay 那张表（那张表在入口包里）。入口包 +33 B：`linediff` 被两个懒块共用，拆成了独立的块，入口里的依赖表多了它的名字。

**真 .app `scripts/accept/history.sh` 9/9**：GBK + CRLF 的文件，存下来的是原来那几个字节、读回来解得出中文；外面改过之后再保存，外部改动夹在中间；
面板里选最早那版恢复 —— 编辑器里换了、标签脏、盘上没动；项目里一个文件不多。去掉「恢复」那一笔的错版本在验收里红。

还没做：重构 / 替换 / git 丢弃这类不经过保存的改盘（它们各有自己的撤销）、按目录看历史、给某一版打标签（IDEA 的 Put Label）、只恢复一块。

