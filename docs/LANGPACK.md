# 语言包：语言服务器用 JSON 声明（docs/TAKEOVER.md R4 + D8，MVP-7）

> 状态：**第一版做完，2026-10-27**（第 5 节）。D8 定的方向：插件只做**声明式集成包** —— 语言服务器怎么起、认哪些文件用 JSON 写，
> 真跑代码的仍然是独立进程（语言服务器），和跑任务同一个信任模型。这一份是它的第一步：把「非 Java 的语言」接上一个通用的语言服务器客户端。

## 1. 要什么、先不要什么

| 第一版要 | 先不要（Java 那边有、通用版以后再接） |
|---|---|
| 实时报错（波浪线、悬停看原因、问题工具窗） | 改名、快速修复、格式化、重构 |
| 补全（服务器给的候选） | 参数提示、行内类型提示、调用层级 |
| 悬停文档（F1 同款） | 运行配置类型、构建工具（JSON 声明的那一半，下一步） |
| ⌘B / ⌘Click 跳到定义（含跳进别的文件） | 下载语言服务器（Python / TS 那一步：要 Node，先量、先核来源） |
| ⌥F7 查找用法 | 包市场（D8：不做）、包带网页界面（不做） |

判据还是 DIRECTION 那句：**秒开不等、独立进程可关、正式包不变大**。不打开那种文件，语言服务器一个字节不加载；入口包不涨（客户端跟着语言包懒加载）。

**Java 先不搬**。`java-lsp.ts` 有一千多行、四十几个命令，全是 jdtls 特有的（生成、重构、Spring、MyBatis）。通用客户端照它的形状写，
等通用这边的五样在两三种语言上都稳了，再把 Java 里通用的那部分换过来 —— 先抽、后搬，不是凭想象抽。

## 2. 包长什么样

```json
{
  "schema": 1,
  "id": "rust",
  "name": "Rust",
  "files": ["*.rs"],
  "languageId": "rust",
  "roots": ["Cargo.toml"],
  "server": {
    "command": ["rust-analyzer"],
    "initializationOptions": {}
  }
}
```

- `files`：文件名的通配（只认文件名，不认路径）。`languageId` 是 LSP 的语言标识（didOpen 要）
- `roots`：往上找到第一个有这些文件的目录当这个服务器的根（一个仓库里几个 Cargo 工程各起一个）；都找不到就用项目根
- `server.command`：第一个是可执行文件。**只在 PATH 里找**（登录 shell 的 PATH，同任务；找不到就说「没装 rust-analyzer」、给装的办法，不替你装）
- 内置的包编进应用（`src-tauri/packs/*.json`，`include_str!`）；你自己的放 `~/Library/Application Support/<id>/packs/*.json`，同 id 的盖过内置的
- 带 `schema` 版本号：以后加字段不破坏老的包；认不得的版本整份不认、说一句

## 3. 怎么接（照 Java 那条路的形状）

- **Rust**：`lspsvc` 本来就是通用的（JSON-RPC、分帧、进程组、空闲收掉、单条消息设闸）。新加 `langsvc`（按 `窗口 × 根 × 包` 一个 Server，`Owned` 登记、关窗口收），
  通用命令 `lsp_open / change / close / complete / hover / definition / references`，诊断照 Java 那样推事件 `lsp-diagnostics`
- **前端**：`editor/lsp-generic.ts`，跟着语言包懒加载（`Editor.svelte` 的 `applyLang`：文件名对上某个包就拉它）。诊断的画法、⌘Click 那套「先缓存、回头补下划线」、
  查找用法的浮层直接用 Java 那边已有的（`UsagesPopup`）
- 文档同步用全文、去抖 250 ms（同 Java，理由在 java.rs 头上）

## 4. 第 0 步实测（2026-10-27）：rust-analyzer，小工程

本机已有 rust-analyzer（不用下载），拿它当第一个包。scratchpad 里一个十几行的小 crate（不拿本仓库量：别把机器跑满）：

| | 量到 |
|---|---|
| 起进程 → initialize 回来 | 0.37 s |
| 第一次非空诊断（类型错一处） | 3.25 s |
| 补全 | 第一次 99 ms，之后 1 ms |
| 悬停 / 跳定义 / 查用法 | 0–1 ms |
| 常驻 | 558 MB（大头是标准库的索引） |
| shutdown + exit | 干净退出，退出码 0 |

558 MB 是语言服务器自己的事，它是独立进程：不开 `.rs` 不起，空闲收掉（`lspsvc` 现成的），关窗口收。IDEA 的 Rust 插件、VS Code 都是同一个 rust-analyzer。

## 5. 第一版做成什么样（2026-10-27）

- Rust：`src-tauri/src/langs.rs`（包的解析和合并、找服务器、按 `窗口 × 根 × 包` 一个、空闲收、关窗口收），命令在 `commands/lsp.rs`；DTO 和 Java 那边同一套
- 前端：`editor/lsp-generic.ts`，`Editor.svelte` 的 `applyLang` 里文件名对上某个包才拉（包的列表问一次）；跳转那一层的口子（`lspResolve` / `lspSettle`）和 Java 共用一个
- 内置三个包：Rust（rust-analyzer）、Python（basedpyright）、TypeScript / JavaScript（typescript-language-server）。后两个这台机器上没装：打开那种文件说一次「未找到 …」、带怎么装，不替你装
- 你自己的包：应用数据目录 `packs/*.json`，同 id 的**在原地**换掉内置的（`pack_for` 按次序找第一个认这个文件的：只是把你的加在后面的话，排在前面的内置那份照样先被找到）

**找服务器**：交互式登录 shell 打一次 `command -v` 和 `$PATH`（按标记取最后两行，rc 文件自己打的字不算），服务器本身直接起、带这个 PATH（rust-analyzer 要找到 cargo）。
命令名拼进 shell 之前先卡字符（只许字母数字和 `-_.+`）。

验收 `scripts/accept/lsp.sh`（10 条，真 rust-analyzer、临时目录里两个文件的 crate）：打开到第一条波浪线 2.9 秒；F1 签名 + 文档注释；`m.yu` 补全出 `yuan()`；
⌘B 跳到另一个文件的 `struct Money`；⌥F7 列出另一个文件里的调用；打开 `.py` 说一次「未找到 basedpyright」带怎么装；子进程是测试应用起的、退出后没了；你自己的包盖过内置的。

**2026-10-28 补上**：⇧F6 重命名（prepareRename 先问能不能改；改动照跨文件替换落盘 —— 撤销日志、开着的标签跟上，同 Java 的改名）、
⌥⌘L 格式化（缩进照编辑器对这个文件的判断给；改动是编辑器里的一笔，⌘Z 撤得回）、⌥↵ 快速修复（把和这几行重叠的诊断原样带回去当上下文；
没带改动的先 `codeAction/resolve` —— rust-analyzer 的「插入显式类型」这类全走这一步，验收把这一步去掉只红这一条；只给命令、要服务器再做一步的说清楚暂不支持）。
lsp.sh 15 条。顺手：重命名框、修复列表的字原来是 Java 专用的口语（「把 X 改名为」「不是合法的 Java 名字」「↵ 做」），改成通用的产品话。

**2026-10-28 再补**：参数提示（敲 `(` / `,` 弹、⌘P 手动弹；画法和解析都是 Java 那份）。lsp.sh 20 条。

**还没做**：跳进项目外的文件（标准库源码）；把 Java 搬到通用客户端上。

## 6. 一键安装 Python / TS 的语言服务器（2026-10-28）

两个都是 Node 程序（basedpyright 在 npm 上也有，同 pyright 的发法），所以走 npm 自己的完整性校验，不自己下 tarball：

- **锁文件编进应用**（`src-tauri/packs/npm/<套>/package.json + package-lock.json`）：每个包钉版本、带 sha512、`resolved` 是 `registry.npmjs.org`。
  单元测试卡着这三样 —— 国内常配镜像，在镜像下生成的锁文件 `resolved` 是 `registry.npmmirror.com`，改一处那条就红
- **装**：`npm ci --ignore-scripts --no-audit --no-fund --prefer-offline`，装进应用数据目录 `lsp/<套>/`。`npm ci` 照锁文件装、核每个包的 sha512（亲手验过：改一个 integrity，`EINTEGRITY`、一个包都不装）；
  `--ignore-scripts`：哪个包的安装脚本都不跑（basedpyright 带的可选依赖 `fsevents` 有安装脚本）。先装进临时目录、成了再改名过去，失败了临时目录清掉、原来那份不动
- **找**：应用装的那份优先（它是 Node 脚本，`#!/usr/bin/env node`：PATH 里要有 node 所在的目录，从登录 shell 找），没有再找你自己全局装的；都没有，那句「未安装」上带「安装（约 N MB）」
- 装好了把「起不来」的记录清掉，开着的那种文件当场接上；npm 失败给错误码和原话（第一版取了最后一行，那是「完整日志在 …」，等于没说）

**版本**（2026-10-28 挑的，都是发布满两周的）：

| | 版本 | 为什么 |
|---|---|---|
| typescript-language-server | 6.0.2（10-09） | 最新；要 Node ≥ 22.22.2 |
| typescript | **6.0.3**，不是最新的 7.0.2 | 7 是原生重写：包里只有 `tsc`、没有 `tsserver`，而 typescript-language-server 要的就是 tsserver。项目自己的 `node_modules/typescript` 在的话它优先用项目的 |
| basedpyright | 1.40.2（10-04） | 发布满两周的里面最新的 |

第 0 步实测（scratchpad 里的小工程）：TS 装 1 秒 26 MB、起 0.32 秒、第一条诊断 0.69 秒、补全 20 → 1 ms、常驻 63 MB；
Python 装 2 秒 42 MB、起 0.23 秒、第一条诊断 0.41 秒、补全 259 → 3 ms、常驻 354 MB。lsp.sh 在真 .app 上装 Python 那一套：1.8 秒（npm 缓存里已有）。
