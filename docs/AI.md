# AI 助手（MVP-6，#46）

一句话：lite-ide 不自己做 agent，是 ACP（Agent Client Protocol）的**宿主** —— 起一个 Claude Code / Codex 的 ACP 适配器当子进程，
它读代码、改文件、跑命令，我们管三件事：把它说的话画出来、回答它反过来问的（权限、文件内容、写文件）、让人审它的改动。
定这条路的理由在 [DIRECTION.md](DIRECTION.md)（「AI」那行和第 6 节）；设计稿是三张画布（主界面、第一次、审阅），
在 claude.ai 上：<https://claude.ai/artifact/PUPaLjieaH2nMXYWSNaH63>。

## 1. 落点

| 哪儿 | 管什么 |
|---|---|
| `src-tauri/crates/acpsvc/` | 不依赖 Tauri。`conn.rs`：按行的 JSON-RPC 2.0（请求配对、超时只给「问机器」的、单行 64 MB 上限、断线把等着的全叫醒）。`process.rs`：起进程（自己的进程组）、stderr 只留最后 32 KB、退了当场报、停的时候整组先软后硬；找 `npx` |
| `src-tauri/src/agent.rs` | 会话表（按窗口登记，关窗口整组停）；agent 发来的转成 `agent-event` 给开着它的那个窗口；三种反向请求；写文件只许写项目里的；每次授权往 app.log 记一行 |
| `src-tauri/src/commands/agent.rs` | 薄命令层；测试构建里 `LITE_AGENT_CMD` 换成假 agent |
| `src/lib/agent/` | 懒加载。`agent.svelte.ts` 对话、反向请求、审阅单；`AgentPane.svelte` 侧边栏里的那个视图 |
| `scripts/fake-acp-agent.py` / `src/lib/dev/mock/agent.ts` | 同一个剧本的两份：前者过真的 ACP 线（smoke ㊹），后者在浏览器里验界面 |

## 2. 第 0 步实测（2026-10-26）

| 问题 | 实测 | 所以 |
|---|---|---|
| 适配器从哪来 | npm 上的 `@agentclientprotocol/claude-agent-acp`、`@agentclientprotocol/codex-acp`，经 `npx -y` 起。第一次下载 Claude 那个 18 秒、Codex 24 秒，之后秒起 | **钉版本**（`acpsvc::CLAUDE_ADAPTER` / `CODEX_ADAPTER`）：0.89.1 / 2.2.2，都是 10-10 发的、选的时候已经过了两周。`npx -y 包名` 不带版本等于每次可能跑一份新代码；升级是改一行、过一遍 smoke |
| 读到 stdin EOF 退不退 | Claude 0.04 秒退、Codex 2.05 秒退 | 正常停是关管道；我们崩了 / 被 kill -9 的路靠**进程组整组杀**兜着：`npx` → node → Claude Code 三层，只杀直接子进程会留下孙子（`process.rs` 那条测试，改成只 `kill` 直接子进程就红） |
| 一个新会话多少钱 | Claude 新会话先写约 3.6 万 token 的缓存，说一句「pong」约 $0.29 | 第一次的页面上照实说；会话尽量接着用，不每轮新起 |
| 有哪些模式 | Claude 的 `session/new` 回 default / acceptEdits / plan / auto / bypassPermissions | 界面上不写死：agent 给什么列什么，点一下换下一个（Claude Code 自己就是 ⇧Tab 轮换） |
| 从访达起的 .app 找得到 node 吗 | 找不到（PATH 只有系统那几个）；nvm / fnm / Homebrew 的 node 都靠 rc 文件 | 起一次交互式登录 shell 把 `command -v npx` 和 `$PATH` 打出来，**按标记取最后两行**（rc 自己也会往 stdout 打欢迎语） |

## 3. 三个决定

> **真适配器实测推翻了第一条的前提**（2026-10-26，`claude-agent-acp@0.89.1`，临时项目、三轮）：它**不走** `fs/write_text_file` ——
> Edit 工具、Bash 的 `printf >` 都是它自己直接写盘，审阅单一条都没有；开着的标签靠文件监听重新加载。它在工具调用的 `content` 里报
> `{type: "diff", path, oldText, newText}`（权限请求里也带着，点「Yes」之前就有）—— 想要审阅 / 退回，要改成按这份差异做（已经在盘上；退回 = 把旧内容写回去）。
> 用户 10-26 定了 AI 不是重点（重点是取代 IDEA），这一步没做，记在第 4 节。另外两条实测：它起来是你 Claude Code 设置里的模式（这台机器是 Auto，不问就改）；
> 权限的选项是它给的英文（Yes / Yes, and don't ask again… / No）。下面这条留着，是给真走 `fs/write_text_file` 的 agent（和 smoke 里的假 agent）的。

**改动先进编辑器，不直接落盘。** agent 的 `fs/write_text_file` 交给前端，按行最小改动写进编辑器（同本地历史的「恢复」），不存盘；
审阅单上「留下」= 存盘，「退回」= 换回 agent 第一次写之前那份。理由：人看得见、⌘Z 撤得回、和自己没保存的改动不打架 —— 直接写盘的话，
开着的标签要走「外面改过」那套冲突裁决，人没保存的那几个字要么被盖、要么得选。agent 读文件（`fs/read_text_file`）给的也是编辑器里那份，
它看到的和人眼前的一样（smoke ㊹ 验：盘上第一行是 `first line`、编辑器里改成没保存的 `UNSAVED_MARK`，agent 说「第一行是：UNSAVED_MARK」）。
**新文件例外**：没有编辑器可进，直接落盘，退回 = 扔进废纸篓。

**问人的不超时，问机器的才超时**（DIRECTION 第 6 节）。权限卡片挂着等人点 —— 超时等于替人拒绝；停这一轮时还挂着的回 `cancelled`（协议要求，不回 agent 一直等）。
「编辑器里有没有没保存的那份」是问机器的：前端 2 秒没回就读盘。它在另起的线程里等 —— 在读线程上等的话，等的这 2 秒里 agent 的别的消息全堵着。

**写只许写项目里的，按词法判。** 要写的文件可能还不存在，不能 `canonicalize`；按路径组件比（`Path::starts_with`，不是字符串前缀 —— `/p/project2` 不在 `/p/proj` 里），
带 `..` 的一律不认。在 Rust 这一层拒，前端连看都看不到（smoke ㊹ 让假 agent 写 `<根>/../lite-agent-outside.txt`，被拒的原话回给了它、盘上没有那个文件）。
**这条管不到 agent 自己的工具**：Claude Code 的 Bash、Codex 的 apply_patch 是它们进程里直接碰盘的，那一层靠它们自己的权限请求（走我们的卡片）。

## 4. 没做的，连同理由

- **AI 写提交信息、把报错送给 agent**（MVP-6 表里的另两样）：先让对话这条路被真用起来。送选区做了（编辑器右键「发给 AI 助手」）
- **`terminal/*`**（宿主替 agent 开终端）：`clientCapabilities.terminal = false`，agent 用自己的。接上要过 tasksvc 那套（进程组、输出设闸）
- **对话历史**：不存。agent 自己存（Claude Code 的会话文件），重启之后接着上次那个会话要 `session/load`，等真用起来再看要不要
- **webview 重载后重放没回的权限请求**：重载会丢掉那张卡片，agent 那边一直等 —— 现在只能「新对话」。真撞上再做
- **Codex 改文件走不走 `fs/write_text_file`**：没实测。不走的话它直接写盘，开着的标签走「外面改过」那条，审阅单上不会有它
- **按工具调用里的差异审阅 / 退回**（上面那段实测）：Claude Code 现在的改动不进审阅单。权限卡片上也还没画那份差异（数据就在请求里）
- **审阅时看全文差异**：审阅单里是统一格式的前 400 行；要看全的打开文件
- **侧边栏宽度**：对话要 400 宽，侧边栏和文件树共用一个宽度 —— 进 AI 视图时拉宽、离开时还回去（只在懒块里记，入口包一个字节不多）。重启时停在 AI 视图上的话，记下的是 AI 的宽度
