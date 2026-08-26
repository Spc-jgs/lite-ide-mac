# 开发日志

> 按时间顺序记录每一步做了什么、为什么这么定、踩了什么坑。
> 与另外两份文档的分工：
> - `ARCHITECTURE.md` —— 最终的架构决策长什么样（是什么）
> - `BENCHMARK.md` —— 性能数字与实现陷阱的细节（多快、什么坑）
> - `JOURNAL.md`（本文）—— 时间线上每一步的经过与取舍（怎么走到这儿的）

---

## 2026-08-25 · 立项

产出 `PLAN.md`：调研 VSCode / Sublime 的社区吐槽，确定"给自己用的 macOS 轻量工作台"
定位，敲定双模式架构（≤50MB 走 CodeMirror 6，>50MB 走自研 mmap 引擎）与技术栈
（Tauri 2 + Svelte 5 + CM6 + portable-pty + ripgrep）。

同时写了 `UNINSTALL.md` 与 `scripts/uninstall.sh` —— 先想好怎么卸载干净再动手，
避免个人项目在机器上留一堆残渣。三层心智模型：项目目录 / Rust 工具链 / 应用数据。

视觉方案出了三版 mockup，选定 `c-idea-dark`（IDEA 新 UI 式中性深灰 + 克制蓝 accent）。

---

## 2026-08-26 · 架构评审与工具链治理

### 对 PLAN 的七处修正

评审后落地 `docs/ARCHITECTURE.md`，对原方案做了七处修正，其中三处是关键的：

| # | 原方案 | 改为 | 为什么 |
|---|---|---|---|
| 02 | 全量行偏移索引 | 稀疏 checkpoint（每 1024 行） | 全量偏移在 1GB 时索引本身就吃 64MB，10GB 吃 640MB，"内存与文件大小无关"当场破功 |
| 03 | （未提 IPC 层） | 数据面强制走二进制 | Tauri 默认 invoke 走 JSON，传 1000 行约 15ms vs 二进制约 1ms；60fps 单帧预算才 16ms |
| 05 | M0 地基 → M1 日志引擎 | M0 直接做日志垂直切片 | 地基是确定能做成的已知工程，引擎是唯一未知数。先花三周做确定的事再去撞唯一可能撞不通的墙，是最坏的排序 |

另外四处：50MB 阈值改复合判据、日志模式不复用 CM6、内存目标 <100MB 放宽到 <200MB、
Java 不上 LSP（jdtls 启动 5–10s、常驻 1GB+，与轻量定位冲突）。

### Rust 工具链治理

诊断发现：Rust 本来就装在本机标准位置（`~/.cargo` + `~/.rustup`），
问题是**只装了最小组件** —— rustfmt / clippy / rust-analyzer / rust-src 全缺，
还往 `~/.profile` 写了一行 zsh 根本不读的垃圾。

按用户要求彻底卸载重装：
- `rustup self uninstall` 清空，确认 shell 配置无残留
- 用 `--no-modify-path --profile default` 重装，rustup 不再自作主张改任何 shell 文件
- PATH 由 `~/.zshenv` 中一段带 `RUST_TOOLCHAIN v1` 标记的配置独家负责，将来删那五行即可
- 补 `rust-analyzer` + `rust-src`
- 移除 rust-docs 离线文档，`~/.rustup` 从 1.4G 降到 571M

一处返工：`rust-toolchain.toml` 起初 pin 了具体版本 `1.98.0`，触发 rustup 把它当成
独立 toolchain 又下一份（约 500MB）。改回 `channel = "stable"` 并卸载多余的那份 ——
个人项目不值这个磁盘，真出现版本漂移再 pin 不迟。

---

## 2026-08-26 · M0 垂直切片

**目标**：用最小成本回答"mmap + 稀疏索引 + 二进制 IPC + 虚拟滚动这条链路到底通不通"。
丑没关系，只验性能。

**做了什么**
- `crates/logengine`：零 Tauri 依赖，可独立 `cargo bench`
  - `index.rs` 稀疏 checkpoint 索引
  - `block.rs` 紧凑二进制块编码
  - `lib.rs` mmap 管理 + 后台索引
- Tauri 命令层（只做解包与错误转换，业务全在 crate 里）
- 前端：Svelte 5 + Vite（不用 SvelteKit，桌面 app 不需要路由/SSR），虚拟滚动 + 块级 LRU

**过程中撞到的**

1. **后台索引线程把读者饿死**。首屏 50 行读取 1112ms，几乎正好等于全量索引耗时——
   窗口开了但一行读不出来。原以为 `RwLock` 每 16MB 放一次锁读者就能插进去，
   实际写者放锁后立刻重新申请，读者根本抢不到。改成后台无锁构建 + 快照发布
   （`Mutex<Arc<LineIndex>>`，读者只克隆 Arc），**1112ms → 0.008ms**。

2. **mmap 顺序扫描没给内核提示**。索引吞吐只有 0.77 GB/s，而 memchr 本身能跑 5–10 GB/s；
   瓶颈是 1GB 映射的约 26 万次 page fault。加 `MADV_SEQUENTIAL` 后 **→ 6.87 GB/s**。

3. **WebKit 元素高度上限**（设计阶段就绕开）。914 万行按 20px 是 1.6 亿 px，
   远超上限（量级 3000 万 px），直接设会让滚动条静默截断、行号全错。
   前端用比例映射把 scrollTop 换算成行号。

**结果**：打开 0.38ms、首屏读取 0.02ms、索引占用 69.8KB（省 1024×，正好等于 stride）、
进程数 4、二进制 3.7MB。技术路线确认可行。

**提交**：`05b28f4`（51 文件 +7543 行），merge 为 `6e0b0f4`。

---

## 2026-08-26 · M1 日志模式

**目标**：日常真能拿它替代 `less` 看线上日志。

**做了什么**
- `level.rs`：行首 64 字节内的快速级别探测；`LevelMap` 以 4bit/行 打包存储
- `filter.rs`：级别 + 文本组合过滤，只返回命中行号，可取消
- tail：`refresh()` 检测追加并重新 mmap，`unseal()` 让末尾半行能被续写；
  inode 变化或文件变短判定为 logrotate
- `read_block_at()`：按命中行号回表，相邻行号走顺序推进
- 前端：行分段解析（时间戳/级别/线程/logger/消息各自着色）、chips 过滤栏、
  搜索高亮、堆栈续行缩进压暗、过滤态显示物理行号

**三处偏离原架构**（理由已就地写进 ARCHITECTURE.md）

1. **级别统计不塞进索引**。顺路统计看似省一遍遍历，实测把索引从 143ms 拖到 870ms（6×）——
   级别探测要逐字节看行首，成本远高于 memchr 找换行。索引是关键路径，必须保持纯粹。
   改为并行的第二个后台任务，88ms 跑完。
2. **过滤不起 rg 子进程**。文件已 mmap 在内存，rg 会重新 IO 一遍 1GB；
   单文件搜索也用不上它的多文件遍历。改用 aho-corasick 进程内实现。rg 留给 M4 全局搜索。
3. **tail 不用 notify**。macOS FSEvents 对单文件有秒级合并延迟，500ms 轮询更快更可控。

**过程中撞到的**

4. **`cargo build` 产出的是 dev 模式二进制**（最严重的一个）。
   停掉 vite dev server 后 app 启动即白屏：窗口在、WebKit 子进程在、`run()` 也进了，
   但 `page_load` 从不触发。根因是 Tauri 的 dev/prod 之分**不看 cargo profile**，
   而由 `tauri build` 命令决定 —— `cargo build --release` 出来的二进制，
   窗口 URL 仍是 `http://localhost:1420/`。
   **代价**：此前几次"端到端验证通过"其实都是经由当时开着的 dev server 加载前端，
   并没有验证到真正的交付物，误判横跨 M0 到 M1 中段。
   换成 `pnpm app:build` 后立刻又逮到一个真 bug：标题栏拖不动窗口，
   缺 `core:window:allow-start-dragging` —— ACL 拒绝在前端只是一条静默 rejection。

5. **改了前端却毫无变化**。只改前端时 `cargo build` 看 Rust 代码没动就跳过编译，
   二进制里留着上一版 dist。`build.rs` 补 `rerun-if-changed=../dist` 解决。

6. **过滤时闪一帧空白**。过滤刚启动、命中数还没回来时就切到过滤视图，行数是 0。
   改成拿到第一批计数之前继续显示旧视图。

一处自我更正：排查白屏时一度怀疑顶层 await 并把结论写进了注释，
后来定位到真因是上面第 4 条，顶层 await 是无辜的 —— 已改掉那处错误归因（`7c7f71a`）。

**结果**：索引 143ms 与级别扫描 88ms 并行、过滤 86–284ms、常驻内存 98MB、
37 单测通过、二进制 3.8MB。

**范围缩减**：堆栈折叠缩为视觉区分（缩进 + 压暗）。折叠交互需要一套
"视图行→物理行"映射，与过滤是同一机制，叠加后复杂度陡增，留待后续。

**未实测**：tail 的前端轮询 + 吸底循环没有在真实追加写入场景下跑过（引擎侧有 4 个单测覆盖）。

**提交**：`3dfbe02` + `7c7f71a`（28 文件 +2126 行），merge 为 `e6ab1aa`。

---

## 2026-08-26 · M2 编辑模式（进行中）

**目标**：能舒服地改代码。CodeMirror 6 + 四语言高亮 + 文件树 + 多标签 + IDEA Dark 主题落地。

（进行中，完成后补记）
