# 按需资源验收与架构审计

2026-10-29。目标是保住「秒开、按需起独立进程、用完收掉、不打断」。**由 agent 根据改动判断是否测量，或由人要求；没有日历周期、调度服务或常驻采样器。**

编译器不能替代资源验收。安全 Rust 仍允许引用环等泄漏；它不能判断缓存是否留得太多、定时器是否空转、子进程是否退出、持久化是否写得过勤。前端 GC 同样不能回收仍被引用的无用对象。有限测量只能提供线索，不能证明永远无泄漏。

## 1. 触发与完成标准

1. 人提出测资源、性能、发热、磁盘增长或泄漏时，执行相关场景。
2. 完成大功能，或修改缓存/批量数据、轮询、后台任务、子进程、窗口生命周期、自动保存、历史/日志持久化时，交付前选择测量范围。通常先跑 quick；改生命周期或出现增长，再跑 full 和对应专项。
3. 小范围展示、文案、文档修改可以跳过，交付说明原因。采样方法修改后重新建基线，不能当成产品性能提升。

能在改动前跑就留 before，改后按相同条件比较。没有 before 的第一轮只叫基线。WARN 先复测定位；基线和预算由证据修订，不能为了通过顺手调大。完成标准是报告实际数字、构建身份、发现、未测范围和下一步，UNKNOWN 必须解释。

## 2. 执行

检查范围与历史状态先看 [RESOURCE-CHECKLIST.md](RESOURCE-CHECKLIST.md)。

```bash
scripts/resources.sh --self-test
scripts/resources.sh --build --profile quick
scripts/resources.sh --profile full --baseline /path/to/before/report.json
scripts/resources.sh --profile full --workload json-text
scripts/resources.sh --profile full --workload json
scripts/resources.sh --profile full --pace paced
scripts/resources.sh --profile full --pace paced --minimap off
scripts/resources.sh --profile full --pace paced --inspect-lifetimes
scripts/resources.sh --profile full --pace paced --inspect-lifetimes --control ipc
scripts/resources.sh --workload logs --profile quick --log-mib 1024 --native-diagnostics
scripts/resources.sh compare /path/to/before/report.json /path/to/after/report.json
```

首次或应用源码/构建变化后带 `--build`，走仓库测试构建、Cargo 离线。不安装依赖、不下载工具或数据；缺本机依赖直接失败，构建日志保留在 `target/resource-tools.noindex/test-build.log`。旧入口 `scripts/mem.sh` 转到这里，`mem.sh 8` 对应 full。

默认 `--workload mixed --pace burst` 保持原有 12 种小文件场景。调查 WebContent 时，`json` 和 `json-text` 各生成 12 份逐字相同的合法 JSON（每份 400 条），仅扩展名为 `.json` / `.txt`；文件数、内容及字节数一致，避免把大小差异误归因于语法处理。`--pace paced` 在每份文件成为当前标签且编辑器出现后再等 1.8 秒；它是日常节奏对照，不保证异步语言处理已完成，也不代表前台响应时间。文件场景与打开间隔写入 report 的 scenario，不同场景的 compare 返回 UNKNOWN，不能算作优化收益。`--minimap off` 通过测试应用原有菜单关闭代码缩略图，核对设置读回和编辑器没有缩略图 DOM；只影响本轮测试身份。该变量也写入 scenario，不能拿减少绘制工作的配置对照冒充修复收益。

打开阶段目前仍在 12 份全部打开后采样 2 秒；这个“采样峰值”不覆盖逐文件打开过程，也不是瞬时最高值。paced 保留相同的关闭等待与日志场景，用于拆分节奏变量，不冒充与历史 `mem.sh`（关后等 25 秒）的完全同条件复现。

| 场景 | quick | full | 用途 |
|---|---:|---:|---|
| 预热开关 12 个小文件 | 1 轮 | 2 轮 | 不进入增长拟合 |
| 测量开关 12 个小文件 | 4 轮 | 8 轮 | 关闭后的 footprint、FD、线程趋势 |
| 每轮关闭后等待 | 5 秒 | 15 秒 | 给清理与 GC 留时间，仍非保证全部回收 |
| 初始/恢复后空闲 | 各 15 秒 | 各 60 秒 | CPU、写入、唤醒 |
| 日志打开与过滤 | 16 MiB | 64 MiB | mmap/过滤及关闭恢复 |

这是**独立测试身份、单窗口、后台、无 JVM 的普通编辑场景**。测试身份共用，和其他验收串行执行。只操作临时项目和测试通道，不接管已运行的测试应用，不修改真实项目/正式应用数据。fixture 有限；原始采样超过 4 MiB 停止。fixture 验完清理，报告保留在被 Git 忽略的 `src-tauri/target/resource-reports.noindex/`；自选 `--output` 必须是新目录，避免覆盖证据。

每秒采样，PID 加进程起点识别身份；进程集合变化导致累计计数不可比时记 UNKNOWN。归属优先用本轮新建的 resource + jetsam coalition：两个编号均需匹配主应用，且这对编号不在启动前快照里。PPID 后代和 `vmmap` Responsible PID 也是直接证据，不把同期新出现的所有 WebKit 都算给应用。coalition 查询用了 XNU 的 PRIVATE ABI，仅限外部工具；返回尺寸不符/权限不足时降为 UNKNOWN，不进入正式应用、不提权。报告保存进程名与计数，不保存完整命令行、环境变量或用户文件正文。

构建前后校验源码指纹；报告记录源码/二进制 SHA-256、HEAD、dirty、OS、架构、硬件和内存压力。构建/采样时源码变化，本轮拒绝成为基线。dirty 构建可以测，但 HEAD 不能冒充完整源码身份。首次打包后的启动只是观察值，不当热启动 p95。schema 3 使用 `valid-syntax-v1`：JSON/XML/HTML/CSS 等各用合法内容，避免用普通文本冒充代码；schema/场景不同的旧报告拒绝混比。

脚本有观察者开销：探针、IPC、vmmap 和 Python 均消耗资源，检查点 IPC 会唤起应用。报告记 Python CPU 时间，但未扣除所有开销。采样峰值不等于瞬时峰值。后台 App Nap 影响计时和占用，不能据此推导前台延迟。

## 3. 指标与判定

| 数据 | 能说明什么 | 边界 |
|---|---|---|
| phys_footprint：主进程 + 确认归属的 WebKit/后代 | 同场景资源保留和增长 | mmap 文件大小、虚拟地址空间、RSS 不等于泄漏 |
| CPU 时间差 / 实际秒数 | 单核 100% 口径，可超过 100% | 不是整机利用率或温度/能耗 |
| logical writes / disk I/O bytes | 写入活动及放大线索 | 不等于 NAND 物理写入、SSD 寿命或 TBW |
| 主进程 FD、线程、主进程/后代退出 | 重复操作资源累积、退出清理 | 不证明前端所有对象释放或系统托管进程全部回收 |
| 数据/日志/WebKit/cache 的文件与分配字节 | 测试目录磁盘占用变化 | 不覆盖真实项目长期历史增长 |
| package idle wakeups | 唤醒线索 | 不等于所有定时器次数或整机能耗 |

调查阈值：空闲 CPU > 单核 2%、空闲 logical writes > 1 MiB/分钟；预热后关闭序列增长 > 8 MiB、斜率 > 1 MiB/轮且 R² > 0.7；FD/线程持续增长。无 JVM 初始测试壳 footprint > 200 MiB、快速打开 12 文件的采样峰值 > 300 MiB 也提示检查。它们是调查阈值；`ARCHITECTURE.md` §7 的原场景预算另行验收，快速批量打开不能冒充日常单个文件打开的工作量。

报告仅在相同场景、机器、OS 下比较；系统内存压力变化时拒绝混比。增长 > 20% 且超过绝对噪声底（内存 16 MiB、CPU 1 个百分点、区间 logical writes 1 MiB）提示复测。阶段缺失或数据不全报 UNKNOWN。WebKit 归属不完整时，已知进程合计只是下界，整应用内存/CPU/I/O 对比不成立。

- `NO_SIGNAL`：本场景/时长没有触发调查阈值，不能写「无泄漏」「不伤磁盘」。
- `REVIEW`：有 WARN 或 UNKNOWN；脚本退出 0，agent 仍必须读 findings 再判断。
- `FAIL`：关闭后编辑视图还在，或退出后本轮确认的主进程/后代仍活着。
- `INVALID`：采样期间源码/只读 fixture 改变，不作基线。FAIL/INVALID 返回非零。

保留 `report.md`、`report.json`、`checkpoints.json` 和原始计数。中断后的 checkpoints 只供排查，不算完整测量。

### 3.1 门槛的性质与依据

**这些数值属于项目自定预算和经验调查线，不是行业统一标准，也不是 macOS 的安全/硬件损伤边界。** 目标是及时发现偏移，再定位原因；缓存可以用内存换响应速度，不能为了一个更小的数字牺牲交互或数据完整性。

| 层级 | 当前机制 | 怎么处置 |
|---|---|---|
| 正确性与生命周期硬约束 | 关闭后视图清空、退出后自有进程收回、正常写入时输出有界；源码/构建身份可追溯 | 功能不成立报 FAIL；条件变化报 INVALID，脚本返回非零。容量还有 Rust 回归测试约束 |
| 产品性能预算 | 历史热启动 <0.5s、起来 <200 MiB、12 文件 <300 MiB、入口包 CI 红线 160 KiB | 来源于本项目定位和历史样本，不是跨应用标准。按约定场景验证；其中入口包已有 CI 阻断，其余并未全部自动阻断 |
| 经验调查线 | 空闲单核 CPU 2%、logical writes 1 MiB/分钟、关闭后增长拟合、同比 20% 加绝对噪声底 | 触发 REVIEW，当前退出码仍为 0，agent 必须解释/复测；不能直接判泄漏、SSD 受损或禁止交付 |

200/300 MiB 来自既有架构预算；2%、1 MiB/分钟、拟合条件及对比噪声底是本次工具选的初始启发式，尚未经过多机/多工作量校准。8 轮可能漏掉小幅或非线性泄漏，R² 也不是泄漏的统计证明。

**原场景与新压力场景有差别**：旧 `mem.sh` 每开一个文件等 1.8 秒、关完等 25 秒；新脚本连续打开 12 个文件，full 关完等 15 秒，文件种类也变了。因此 584 MiB 是该压力场景的真实采样结果，超过的是沿用的参考线；目前不能据此断言日常使用违反原预算、发生泄漏或实现一定有 bug。先补与日常节奏一致的对照、同条件重复样本和前台延迟，再决定优化与预算修订，保留压力场景用于查峰值。

目前执行方式是 AGENTS 触发规则 + 手动/agent 按需运行。资源脚本没有加入定时任务，也没有作为每次提交都必须通过的 CI 全量基准。

## 4. 架构审计与待办

基于 2026-10-29 工作树源码；性能结论另看实测。历史设计文字不能替代实现。

轮询的完整用途、成熟编辑器的做法及相邻异步/文件监听缺口，见 [轮询与后台设计评审](POLLING-AND-BACKGROUND-REVIEW.md)。其中外部重载清掉等待期间的新草稿、Git 刷新不合并重复工作已有受控状态层复现；其余源码风险与待量项分别标注，不把轮询存在本身判作缺陷。

| 优先级 | 已确认行为 / 风险 | 代码证据 | 下一步与完成标准 |
|---|---|---|---|
| 保留 | Owned 按窗口登记；关闭先摘表、锁外析构；语言服务器/调试/数据库/agent/任务按 owner 释放 | `src-tauri/src/state.rs::release_window`、`owned.rs`，已有漏收表源码检查 | 新资源进入同一生命周期；专项关闭后句柄/子进程恢复 |
| 保留 | 普通子进程并发排空管道、设输出上限；日志引擎取消；应用日志轮转 | `crates/procutil`、`crates/logengine`、`crates/applog` | 不另开无上限输出/后台进程通道 |
| 已修 P1 | 任务日志无换行时突破 cap；note 可无限排队 | `crates/tasksvc/src/sink.rs`；相同 4096 B 输出、cap=64 B，修复后当前 26 B + 上份 60 B（此前当前 4096 B） | 按块内换行轮转；容量到线明确标记分段；中文字符跨块也验过；note 最多排队 64 KiB、超长部分标明省略字节数，EOF 补写队列 |
| 已修 P1 | 任务历史原来只在启动清理 | `crates/tasksvc/src/prune.rs`、`Task::start`、输出线程 | 启动/输出 EOF/轮转均触发清理，无定时器；写入登记和清理共用锁，保护当前与 .1。5 GiB 是清理目标，全部被活跃文件占满时允许超出，不为达标删活跃文件；返回真实 remaining_bytes |
| 已补测量 | vmmap 缺 Responsible PID，原来只有主进程下界 | `resource-probe.c` / `resource_guard.py` 的 fresh coalition 判断 | 当前机器能确认主进程及 3 个 WebKit；测试拦住误认领其他应用。schema 从 1 升为 2，旧下界报告不作新方法的对比基线 |
| 已测现象，先校准场景 | 快速打开 12 文件的全进程采样峰值超过 300 MiB 参考线 | schema 3 full 报告 §5.2；旧日常节奏不同，见 §3.1 | 峰值是真的，是否影响日常体验/应降至多少仍待验证；不直接判泄漏或实现缺陷 |
| 已确认策略缺口，影响未测 | 历史每文件 4 MiB×200 版、30 天；只清刚记录的目录，无全局容量/过期目录扫除 | `crates/histsvc/src/lib.rs::record/prune` | 800 MiB/文件是极端理论值，有内容去重，非实测异常。先量代表性工作量的容量增长；需要总容量承诺时再补策略 |
| 已确认策略缺口，影响未测 | HTTP 按项目保留约 50 份响应，原始响应接收上限 32 MiB，无全局字节预算 | `src/commands/http.rs`、`crates/httpsvc/src/lib.rs` | 50×32 MiB 只反映原始大响应的保留量级，不能当格式化文件的硬上限；长期真实占用未测，按使用场景排期 |
| 已修 P1（4ccae8e） | JSON 缩进原来只卡输入，现于每次追加前卡输出 8 MiB，超限保留完整原文并说明未格式化 | `crates/httpsvc/src/json.rs::reindent` / `send_capped`、HttpPane；[#85](https://github.com/Spc-jgs/lite-ide-mac/issues/85) | 旧实现 2001 B → 2,004,001 B；6001 B 放大样例验红后修复。httpsvc 22 项通过，含真实 curl 回退原文及输出边界 |
| 已修 P1（4ccae8e） | 外部重载曾清掉等待期间的新草稿；现用重载代次、标签身份/路径/模式/指纹阻止过期写回 | `docs.svelte.ts::reloadGuard/checkExternalChanges`、docs-ops；[#84](https://github.com/Spc-jgs/lite-ide-mac/issues/84) | 旧实现在草稿保护断言验红；11 场景覆盖编辑/撤销/保存/改名/关闭重开/模式/编码/逆序及显式重载 |
| 已修 P2（4ccae8e） | Git 刷新合并；日志/堆串行采样与回包失效；监听 Err/rescan 显式校准和受限降级 | [#86](https://github.com/Spc-jgs/lite-ide-mac/issues/86)、[#87](https://github.com/Spc-jgs/lite-ide-mac/issues/87)、[#88](https://github.com/Spc-jgs/lite-ide-mac/issues/88)；细节见轮询评审 §7 | 10 次 Git 触发首轮前查询 20→2、整批共 4；采样错误副本验红；watch 17 项。Java 通知不全时提示手动重启，不冒充自动修复所有索引 |
| 观察项，暂不重构 | 两种 LSP 客户端 wire 后每秒醒一次；空文档集合也醒后返回 | `src/lib/editor/java-lsp.ts`、`lsp-generic.ts` | 行为已确认，但未测出它造成空闲 CPU/能耗问题；有信号再改启停，不仅凭 setInterval 存在就报 bug |
| 小型保留项，严重度未量 | 两种 LSP 的 `viewsOf` 在 destroy 时只删 Set 内视图、不删空 Set 的路径键；诊断缓存只在 opened 关闭分支删路径 | 同上 `Sync.destroy` / `diagsOf` | 空键保留是真的，未证明 EditorView 泄漏；正常项目下可能很小。相关代码改动时可顺手清空键，当前不建立通用缓存治理框架 |
| 保留采样/兜底机制 | Structure 400ms、Services 端口/堆、日志索引/过滤/跟随 | `StructureView.svelte`、`ServicesView.svelte`、日志组件 | Structure 未变不重算；日志/堆已补串行与失效保护，周期为本轮完成后的等待时间；不全面改事件驱动。其余观察见 [#89](https://github.com/Spc-jgs/lite-ide-mac/issues/89) |
| 待量，保留现状 | 自动保存/会话/历史可能放大写入；已有 dirty、防抖、历史内容去重 | autosave、persist 路径、`histsvc` | 尚无异常写入量证据；连续编辑/保存测 bytes/操作。保留原子性、恢复能力与 fsync，不能为省写丢可靠性 |

旧 `mem.sh` 操作正式应用、用 AX/前台切换、按名字批量终止；WebKit 猜归属并把缺失采样合成 0。现已换安全入口。旧结果只保留历史参考身份，不作新方法的可比 before。`budget.sh` 可只读观察启动/主进程历史，但不是全进程资源验收。

## 5. 实测记录与证据边界

历史报告保留在本地 `src-tauri/target/resource-reports.noindex/`：schema 1 只有主进程下界；schema 2 补了 coalition 归属，但早期普通文本夹具不代表合法源码；schema 3 换成合法语法。三种口径不混比，不把新数字减旧数字当优化收益。方法纠偏的原因见 JOURNAL.md「资源验收」。

### 5.1 容量修复与验证

运行期回收以输出管道 EOF 为界，不跟着卡片是否还显示走，也不只看组长退出。相同路径的写入保护有引用计数；正常输出、强制停止和并发任务共用该保护。只回收应用自己生成的命名范围，用户文件和 live.json 不碰。

单个新输出文件受硬 cap 约束，普通行尽量完整，单行持续输出或跨块到线时标记「日志分段 / 上行续接」。已有历史超限文件不会被原地截断；它可能暂留在 .1，直到下一次轮转或符合回收条件，避免改坏仍被 mmap 的旧文件。活跃输出优先于 5 GiB 的总量目标。

原工作树验证：735 项 Rust 测试通过、4 项已有 ignored 未运行，clippy 通过、任务真应用验收 44/44。工具 14 项自检覆盖缺失值/PID 身份/进程归属/场景可比性/合法语法与峰值告警。旧无换行写法、漏保护活跃文件、漏接 EOF finish、误认领其他应用、放宽峰值阈值的错误副本均被对应测试抓到；副本在临时目录，不修改正在构建/测量的源码。

仅本轮资源改动的独立暂存快照也已验证：前端构建、14 项工具自检、734 项 Rust 测试（4 ignored）和 clippy 通过。两组 Rust 测试数不同来自未纳入本次提交的其他修改，不将工作树结果冒充精确提交结果。

### 5.2 本轮工作树的合法语法复测（非独立资源提交基线）

报告：`src-tauri/target/resource-reports.noindex/20261029T074356Z-lite-bridge.4lKpWs/report.json`，schema 3 / full / valid-syntax-v1。源码 SHA-256 `3833a10c8b76405ed85025837d2def7fa0776731cccf7fefffadab84c8b90bd4`，测试二进制 SHA-256 `bcfa62cedea9a080ab6be1b5badb86de37259467d9f6f46c09c7f6fa6e882461`，dirty 工作树；采样中源码与 fixture 未改变。压力字段前后均为 1，不能据此断言区间内所有瞬时系统条件完全不变。

这组数据来自当时整个 dirty 工作树，包含并行的 UI 修改。资源修复与其他 UI 修改分开提交；这些数字是该工作树的本机记录，不能冒充仅资源提交的干净版本基线。原始报告留在本地 target，复现时以脚本记录的新构建指纹为准。

| 全进程数据（主进程 + 3 个 WebKit） | full 实测 |
|---|---:|
| 初始空闲 footprint / 单核 CPU | 101.22 MiB / 0.366% |
| 快速开 12 文件各轮采样峰值 | 583.82、564.11、555.68、558.39、557.21、557.44、553.55、555.04 MiB |
| 各轮关闭后 footprint 中位 | 220.39、217.13、220.44、226.49、224.27、227.11、230.74、155.97 MiB |
| 主进程 FD | 8 轮均为 11 |
| 恢复空闲 footprint / 单核 CPU | 203.18 MiB / 0.330% |
| 两段 60 秒空闲 logical writes | 各 96 KiB，含 WebKit，非 NAND 写入量 |
| 退出残留 | 报告内主进程/后代 0；另外只读复查 4 个已确认 PID 均已消失 |
| 启动观察值 | 472 ms；仅本次观察，非热启动 p95 |

结论 **REVIEW**，唯一自动告警是 `opened_memory_budget`。本时长内关闭序列未触发持续增长阈值，不能推导无泄漏；合法样本仍有约 584 MiB 峰值，证明旧夹具不是足以消除该告警的解释。本轮修复了容量边界与测量缺口，尚未针对打开峰值优化；该峰值是否违反日常场景预算、是否影响体验，需要按 §3.1 对齐场景后判断。

下一步先把相同操作拆成只开纯文本、只开 JSON、交替开关一种语言，保留按进程样本；再用分配生命周期定位 WebContent 的瞬时对象和 GPU 的绘制资源。schema 2 的当前源码样本中，WebContent 打开窗口的单进程峰值约 398 MiB、GPU 约 151 MiB，关闭后分别约 157–190 MiB / 20–21 MiB；它用于定位方向，不能与 schema 3 做优化差值。保持 300 MiB 参考线，不主动触发 GC 来凑通过数字。

最终 `LITE_BUILD_OFFLINE=1 CARGO_NET_OFFLINE=true pnpm app:bundle` 成功，正式 `.app` / `.dmg` 已更新；完成后源码指纹仍为上述值，测试身份剩余进程为 0。本轮未加依赖、未调整性能门槛。当前工作树正式前端入口为 162,328 B（158.52 KiB），处于现有 CI 的 >152 KiB 告警区、尚未越过 160 KiB 红线；后续功能需要继续控制入口体积。

### 5.3 异步/轮询修复后的 full（第一轮）

报告：`src-tauri/target/resource-reports.noindex/20261029T092756Z-lite-bridge.VqHmOW/report.json`，schema 3 / full。源代码 SHA-256 `99f16f5d49e7747068de16a88653e2296865dedeb3727b099aef0c10000cbddc`，测试二进制 SHA-256 `c229e97f3a4d08055a4d77c2b476096c9454edbe33ac76353f31016edd64b735`；HEAD `e5b5281` 上 dirty 工作树。源码/fixture 未变，压力字段前后为 1，确认主进程及 3 个 WebKit。

初始空闲 98.11 MiB / 单核 CPU 0.067%；快速打开采样峰值 439.22 MiB。关闭后中位从 194.32 到 259.22 MiB，斜率 7.87 MiB/轮、R²=0.766；恢复空闲 272.05 MiB / CPU 0.33%。两段 60 秒空闲 logical writes 分别 24 KiB / 0；主进程 FD 每轮 11，关闭后编辑器每轮 0，退出残留 0。

结果 **REVIEW**：同时触发 opened_memory_budget 和 retained_memory。拆进程后，主进程关闭后约 27.8 MiB、GPU 约 16.1–16.6 MiB，增长主要在 WebContent（各自中位约 145.83 → 210.28 MiB）；这些单进程中位数不能直接相加冒充整体中位数。不能仅凭本轮判泄漏，也不能用 FD/编辑器数量稳定抵消内存告警。已建 [#90](https://github.com/Spc-jgs/lite-ide-mac/issues/90) 并安排同构建复测，之后按文件类型与分配生命周期定位。

本轮未取得启动观察数字，原因尚未定位，启动速度记 UNKNOWN。与 §5.2 的不同工作树不能作归因优化差值，尤其包含中间 UI 修改。没有放宽门槛、没有主动 GC 凑通过；本轮修复的正确性证据与未解决的内存调查分别记录。

**同构建复测**：`20261029T093912Z-lite-bridge.8Afdgm/report.json` 的源码/二进制 SHA-256 均与上轮一致，source/fixture 未变、压力字段前后均 1。本轮关闭后中位 201.58 → 191.18 MiB，斜率 -3.75 MiB/轮、R²=0.422，未重现持续增长告警；恢复空闲 196.07 MiB / 单核 CPU 0.257%。主进程 FD 仍逐轮 11、退出残留 0；两段 60 秒空闲 logical writes 为 196 / 100 KiB。打开峰值 469.27 MiB，仍为 REVIEW（opened_memory_budget）。

两轮同一构建的变化不能算优化收益，也不能以第二轮没触发增长抹掉第一轮。已向 #90 补充重复性结果：继续区分 WebContent 的活对象保留、GC/分配器缓存和异步积压，根因尚未确定。当前不为达到内存数字主动 GC 或调整预算。正式 app/dmg 打包成功；本轮前端 115 文件、Rust 740 项（4 ignored）、clippy、HTTP 36、Git UI 15、Spring/Services 111、任务 44 均通过，相关检查没有替代这两条资源调查。

### 5.4 同构建拆分内容与打开节奏（2026-10-30）

以下配置对照保持应用源码、测试二进制与 §5.3 完全相同，新增脚本场景；不是应用优化的 before/after。源码 SHA-256 `99f16f5d49e7747068de16a88653e2296865dedeb3727b099aef0c10000cbddc`，二进制 SHA-256 `c229e97f3a4d08055a4d77c2b476096c9454edbe33ac76353f31016edd64b735`。HEAD 为 `4ccae8e`，dirty 来自脚本、测试和文档修改。各组独立启动并清空测试身份数据，不能把不同进程的回收时机当作固定差值。

前两组都用 12 份相同的 JSON 内容，仅扩展名不同。full 仍为预热 2 轮、测量 8 轮、关后等 15 秒、初始/恢复空闲各 60 秒、最后走 64 MiB 日志打开/过滤。第三组沿用原 mixed 文件，只改变每文件打开后的等待。各项均未主动 GC、修改预算或配置定时任务。

| 场景 | 报告目录（`target/resource-reports.noindex/`） | 打开后采样峰值 MiB | 首/末关闭中位 MiB | 关闭斜率 MiB/轮；R² | 恢复空闲 MiB |
|---|---|---:|---:|---:|---:|
| `json-text` / burst | `20261030T012403Z-lite-bridge.AGvGMC` | 478.61 | 175.08 / 220.08 | +5.65；0.433 | 183.80 |
| `json` / burst | `20261030T012950Z-lite-bridge.B2LGyN` | 481.25 | 214.43 / 164.50 | -9.61；0.724 | 171.22 |
| `mixed` / paced | `20261030T013819Z-lite-bridge.lhrwwt` | 551.22 | 218.13 / 417.00 | +28.53；0.978 | 419.79 |
| `mixed` / paced / minimap off | `20261030T014908Z-lite-bridge.GREkVJ` | 309.57 | 192.36 / 196.94 | +1.35；0.046 | 206.38 |

前两组（json-text/json）均为 REVIEW，仅触发 `opened_memory_budget`；未触发持续增长条件。`json-text` 首末上升不能因 R² 未达线而忽略，其序列为 175.08、189.61、207.16、225.41、242.80、209.32、216.77、220.08 MiB，末尾恢复空闲又降到 183.80 MiB。JSON 组序列为 214.43、239.18、185.13、181.86、184.77、160.33、161.97、164.50 MiB。仍只能描述观测，不能用某一轮下降证明无泄漏。

两轮主进程 FD 每轮 11、关闭时标签/编辑器均 0、退出确认残留 0、4 个进程归属完整、源码/fixture 未变、压力字段前后均 1。恢复空闲单核 CPU 分别 0.212% / 0.197%，60 秒 logical writes 为 108 / 100 KiB；它们不代表物理 SSD 写入或整机发热。启动观察 455 / 454 ms，不是热启动分布。

两轮首/末关闭时 WebContent 中位分别为 125.55 / 171.11、165.24 / 116.56 MiB，主进程约 27.5 / 27.9 MiB。纯文本也出现峰值，说明“必须有语法解析才出现峰值”不成立；各一轮且回收有波动，不能据此证明语言处理没有额外成本。GC/分配器缓存、共享编辑器/绘制路径或仍活着的对象尚未区分；没有可归因的修复证据，不改缓存或回收策略来凑预算。

慢速完整组再次触发 `retained_memory`，关闭序列为 218.13、224.75、284.72、295.72、327.36、340.74、385.77、417.00 MiB，恢复空闲也没有明显回落。首/末关闭 WebContent 为 164.09 / 368.24 MiB、GPU 为 17.41 / 17.34 MiB，增长仍主要在 WebContent；主进程 FD 每轮 11，关闭标签/编辑器 0、退出残留 0、归属完整、源码/fixture 未变、压力前后均 1。恢复空闲单核 CPU 0.507%、logical writes 0；本轮未得到数值启动行，启动观察 UNKNOWN。

这一轮的恢复空闲阶段额外只读取一次 vmmap（耗时约 0.6 秒），结果保存在同报告目录的 `webcontent-vmmap-summary.txt`。该时点 WebContent footprint 约 360.5 MiB；WebKit Malloc dirty 约 255.8 MiB，owned unmapped graphics dirty 约 183.7 MiB。VM 类别不能相加当 footprint，更不能说明是哪段应用代码保留；这次额外探针可能扰动恢复段 CPU/I/O，不影响调用前已完成的关闭序列。它提示要追加相同 paced/full、仅关闭缩略图的对照，不能直接断言 canvas 泄漏。

缩略图关闭组的完整序列为 192.36、202.30、217.18、231.69、228.80、204.63、226.71、196.94 MiB，没有触发持续增长；打开峰值仍约 309.57 MiB，结论 REVIEW。WebContent 首/末关闭中位 138.30 / 142.89 MiB；主进程 FD 每轮 11，关闭标签/编辑器和各阶段缩略图 DOM 均 0，退出残留 0。恢复空闲单核 CPU 0.672%、60 秒 logical writes 0，启动观察 458 ms；归属完整、源码/fixture 未变、压力前后均 1。此组未额外跑 vmmap。

配置对照各一轮只能支持继续检查缩略图，不能把关闭功能后的数字算成代码收益。后续真应用画布探针确认首次打开会建 4 张画布（正常一组为可见画布 + 离屏瓦片 2 张），开关 effect 用新的 ViewPlugin 声明替换 build 中刚装好的声明；destroy 只摘 DOM，位图尺寸仍非零。`accept/minimap.sh` 保持画布引用，对原实现有 3 通过 / 6 断言失败，分别指向重复创建与关闭后清理。已新建 [#91](https://github.com/Spc-jgs/lite-ide-mac/issues/91) 做局部修复；#90 的整体增长仍待同场景复测。

慢速组首轮在预热阶段因脚本将 `/tmp` 输入路径直接和应用规范化后的 `/private/tmp` 标签路径比较而超时；只有 baseline_idle 的 `20261030T013538Z-lite-bridge.f8UwLB` 不算完整报告。已修正就绪检查并补父目录软链接回归：旧比较断言验红，工具自检现为 18 项通过。没有将这次脚本失败归因于应用或计入资源对照。

### 5.5 缩略图局部修复后的同场景 full（2026-10-30）

修复 [#91](https://github.com/Spc-jgs/lite-ide-mac/issues/91)：每个 Editor 的 build/开关 effect 共用同一份 minimap 扩展声明；销毁时把可见画布及离屏瓦片都清为 0×0，并清 context 引用。未关闭默认功能、未改预算。插件复用依据为已读的本机 CodeMirror `updatePlugins` 和 [官方 ViewPlugin 文档](https://codemirror.net/docs/ref/#view.ViewPlugin)；canvas 位图尺寸规则参照 [HTML Standard](https://html.spec.whatwg.org/multipage/canvas.html#the-canvas-element)。不据规范推导具体释放量，实际占用另量。

工具自检 18 项通过；`scripts/accept/minimap.sh` 从旧实现 3 通过 / 6 失败变为新构建 9/9。该验收主动保留画布引用，验证首次只建两张、功能开关/关标签后位图清零、重新打开正常绘制；不是依靠 GC 碰巧发生。前端 115 文件通过，svelte-check 0 错误/0 警告；真应用 smoke 142/142。smoke 仍按既有行为向废纸篓留下 1 个本轮临时文件，CLI 清理被系统 PermissionError 拒绝，未改权限或碰其他条目；这是既有 [#54](https://github.com/Spc-jgs/lite-ide-mac/issues/54) 的验收收尾缺口，不冒充本轮已清理。

报告 `20261030T020620Z-lite-bridge.YbRKuj/report.json`：应用源码 SHA-256 `1a3e7396ada5bf249347da108380ea0f904bb3438f9464abb2389300e748262a`，测试二进制 SHA-256 `994c5d21ab80fa84452fdeaeb06cf8bfdbeeb487fa0133e333bd57cec12b5822`。同机同 OS、full / mixed / 每文件等 1.8 秒，**缩略图保持开启**；与修复前 paced 场景比较。源码/fixture 未变、归属完整、前后压力字段均 1，主进程 FD 每轮 11、关闭标签/编辑器/缩略图 DOM 均 0，退出残留 0；本轮未调用额外 vmmap。

| 指标 | 修复前 | 修复后第一轮 |
|---|---:|---:|
| 打开后采样峰值 MiB | 551.22 | 290.72 |
| 首/末关闭中位 MiB | 218.13 / 417.00 | 157.36 / 275.05 |
| 关闭斜率 MiB/轮；R² | +28.53；0.978 | +15.86；0.647 |
| 恢复空闲 MiB | 419.79 | 294.99 |

第一轮恢复空闲单核 CPU 0.382%、60 秒 logical writes 0；启动观察 393 ms（非热启动 p95）。峰值已落到 300 MiB 参考线内，但关闭序列仍为 157.36、164.19、177.32、193.49、135.91、224.50、249.16、275.05 MiB，首末增长约 117.69 MiB。未满足自动 retained_memory 的 R² 条件不能抹掉这段增长；#90 继续打开，不声称根因全部解决或无泄漏。

该报告自身 findings 为空，但与修复前比较时 opened-5 的 2 秒窗口单核 CPU 从 0.571% 到 2.855%，自动 comparison 提示复测，因此最终 verdict **REVIEW**。这不是空闲 CPU 门槛，也不足以认定持续性能回退；按同构建/同场景再跑一轮，不豁免告警或调预算。

同源码/二进制复测报告 `20261030T021911Z-lite-bridge.yrs8hL/report.json`：打开后采样峰值 321.30 MiB，关闭序列 204.13、219.05、243.97、263.27、186.69、210.57、226.72、245.94 MiB，斜率 +1.84 MiB/轮、R²=0.032，恢复空闲 239.02 MiB。FD 每轮 11，关闭标签/编辑器/缩略图 DOM 为 0，退出残留 0，源码/fixture 未变、归属完整；启动观察 426 ms。恢复空闲单核 CPU 0.412%、60 秒 logical writes 0；opened-5 观察值为 0.164%，本轮未额外调用 vmmap。

**本次前后系统压力从 1 变成 2，比较器返回 UNKNOWN，不能把下降当作优化或据此排除 CPU 回退。** 该轮自身也触发 opened_memory_budget（321.30 > 300 MiB），最终仍 REVIEW。复测记录支持继续调查，不支持宣称全部场景达标。#91 的重复创建/位图清理有独立回归证据可交付；#90 继续跟踪关闭后保留与峰值，下一步需要压力稳定时的重复样本和分配/对象归属，不能只看是否碰巧满足 R² 门槛。#89 的其他观察项仍不自动转成必做重构。

### 5.6 总清单建立与进一步内存定位（2026-10-30）

本轮建立 [RESOURCE-CHECKLIST.md](RESOURCE-CHECKLIST.md) 的 R01–R22，总表包括历史正常项、已修复后的回归和未测专项。三轮均沿用 §5.5 的应用源码/测试二进制（SHA-256 分别为 `1a3e7396ada5bf249347da108380ea0f904bb3438f9464abb2389300e748262a` / `994c5d21ab80fa84452fdeaeb06cf8bfdbeeb487fa0133e333bd57cec12b5822`），没有改应用实现；新增诊断改变了测量场景，不能把新数字当优化收益。

三轮均 full、每文件等待 1.8 秒、压力前后 1、4 个进程归属完整、源码/fixture/诊断脚本未变，主进程 FD 每轮 11、关闭 DOM 清空、退出残留 0。每轮输出原判定均 NO_SIGNAL；这不替代下面的人工判断及总清单的专项覆盖。日志仍为约 64 MiB、在编辑/IPC 循环之后打开/过滤；下表全部为整应用数据，不是某个功能的增量。

| 诊断场景 | 外部 eval 数 | opened 阶段采样峰值 MiB | 日志打开 / 过滤采样峰值 MiB | 恢复空闲 MiB | 最终 view alive/observed | 最终 canvas alive/observed |
|---|---:|---:|---:|---:|---:|---:|
| 编辑器 · 分阶段解引用 | 409 | 204.68 | 279.05 / 184.94 | 180.47 | 96/120 | 0/240 |
| IPC 对照 · 分阶段解引用 | 408 | 97.05 | 202.24 / 305.97 | 110.38 | 0/0 | 0/0 |
| 编辑器 · 仅末尾解引用 | 409 | 293.52 | 466.60 / 405.10 | 211.33 | 24/120 | 0/240 |

报告目录均在 `src-tauri/target/resource-reports.noindex/`：

- 分阶段编辑器：`20261030T030340Z-lite-bridge.f6tJhL/report.json`。
- IPC 对照：`20261030T031430Z-lite-bridge.7orW9s/report.json`。
- 仅末尾编辑器：`20261030T032406Z-lite-bridge.EewKmj/report.json`。

原探针源码已另存各目录的 `lifetime-probe.js`，与该报告记录的探针 SHA-256 一致。分阶段模式会在检查点解引用；后加的默认 final 模式仅在恢复空闲末尾解引用一次，两种方法分别记录 scenario。WeakRef 仍 alive 不等于有无用的强引用，解引用本身会短暂保活目标；GC 时点和观察方式均影响计数。

本轮能缩小的范围：

1. IPC 对照关闭中位 96.50 → 96.77 MiB，增长约 0.27 MiB；409/408 次外部 eval 的规模接近，但业务 IPC 并不相同。单靠测试 eval 的数量不足以解释编辑器场景的大幅保留，不能据此排除所有观察者影响。
2. 分阶段模式恢复空闲时 240 张 canvas 全部回收；仅末尾模式也全部回收，非零位图为 0。没有发现 #91 修复后的旧画布持续留活或非零位图回归，不能据此排除所有图形/native 缓存。
3. 仅末尾模式观察过的 120 个 view/doc 中各有 96 个已回收、24 个仍 alive；分阶段模式各有 96 个仍 alive。不能按某次 alive 数认定泄漏，也不能把差异全归因于探针，因为 GC 时点没有被控制；尚缺真实引用链/分配来源。
4. 仅末尾模式的关闭序列为 182.47、191.53、202.18、216.89、197.21、187.82、210.63、222.94 MiB，斜率 +3.76 MiB/轮、R²=0.411。未触发自动增长线不抹掉约 40.47 MiB 的首末变化。
5. 日志打开/过滤有独立的瞬时分配峰值；即使 IPC 对照没有编辑器使用历史，日志过滤也采到约 305.97 MiB。另一轮混合历史下日志打开采到 466.60 MiB。它们不等于日志常驻占用，也不等于完成原 1 GiB 日志预算验收。R09 继续单列。

因此 #90 继续调查：下一步是 R03 的引用链/分配定位、R09 的独立日志规模与瞬时分配、R22 的长时稳定工作量；没有足够证据做新的应用重构。资源报告补充了每阶段采样峰值和已知进程全流程峰值，避免只看 opened 的 300 MiB 调查线漏掉日志阶段；保持原门槛不变。

本轮检查范围：R01、R02、R03、R04、R05、R06 的普通测试身份范围；R09 的约 64 MiB 日志局部场景；R20 的构建与测试身份收尾；R21 的对象存活诊断。R04 的连续编辑/读放大、R06 的重工具、R09 的 1 GiB 部分仍未执行。R07/R08 前台分布、R10–R12 工具专项、R13/R14 长期容量/故障注入、R15 事件风暴、R16 多窗口、R17 恢复、R18 其他协议、R19 硬件指标、R22 长时工作量，本轮未执行：当前改动限于诊断和清单，相关项目保留待测/原有证据，不以本轮普通场景冒充通过。

### 5.7 独立日志：图形记账、结果表与分配器保留（2026-10-30）

本轮选择 R01–R06 的隔离单窗口范围、R09 独立日志、R20 测试收尾，以及 R21 的编辑器保留代码核对。应用源码/测试二进制仍为 §5.6 的 `1a3e7396…` / `994c5d21…`，没有改应用实现。三次新进程都没有编辑器使用历史，均后台、无 JVM、4 轮打开→全命中过滤→关闭，不请求 GC、不调用 allocator purge。内存压力前后均 1，fresh coalition 归属完整，源码/fixture 未改，主进程 FD 各阶段均 11、退出残留 0。整机已使用约 7 GiB swap，不能把压力等级 1 写成整机无内存负担。

先行定位使用有限临时脚本：初始/恢复空闲各 30 秒、关闭等 5 秒、打开阶段并发采样 6 秒、过滤阶段 8 秒，完成后各采样 5 秒；在第一轮及初始/恢复阶段另取 vmmap。随后把方法整理成正式的 `--workload logs` 入口，以 16 MiB/quick 走通真应用。quick 空闲 15 秒、full 60 秒；full 关闭等 15 秒、8 轮，quick 等 5 秒、4 轮。均无预热，首轮单列；不能将 quick 的首轮初始化混入后当成稳定增长的证明。

| 文件 / 方法 | 过滤完成后整应用中位 MiB（4 轮） | 全流程采样峰 MiB | 关闭后整应用中位 MiB（4 轮） | 恢复空闲 MiB | 原始判定 |
|---|---|---:|---|---:|---|
| 约 64 MiB · 先行定位 | 170.98 / 174.29 / 179.28 / 153.25 | 352.22 | 162.80 / 127.39 / 169.92 / 145.35 | 132.94 | NO_SIGNAL |
| 约 1 GiB · 先行定位 | 244.68 / 370.49 / 230.36 / 290.62 | 474.94 | 231.93 / 233.76 / 236.61 / 228.21 | 223.91 | NO_SIGNAL |
| 约 16 MiB · 新入口 quick | 169.20 / 192.07 / 193.99 / 198.48 | 336.39 | 119.13 / 144.96 / 144.09 / 149.71 | 145.16 | REVIEW（关闭序列增长） |

报告目录均在 `src-tauri/target/resource-reports.noindex/`：

- 64 MiB：`20261030T063937Z-lite-bridge.DGNhcY/report.json`。
- 1 GiB：`20261030T064326Z-lite-bridge.1Q8HUd/report.json`。
- 16 MiB：`20261030T064715Z-lite-bridge.8nzxZK/report.json`。

先行脚本及原 `resource_guard.py`、方法 SHA 清单另存各报告目录，原始报告未改写。64 MiB 先行脚本的 DOM 行数 selector 写成不存在的 `.log-view`，其 `rows:0` **无效**，不拿它证明前端没渲染；native 索引/级别/命中数量均核对完成。1 GiB 和新入口已改为实际 `.lv`，每轮记录 46 个 DOM 行。1 GiB 的 SHA 使用流式 1 MiB 缓冲；64 MiB 的早期诊断在准备/收尾读过整份 fixture，两种方法有观察者差异，不直接算缩放倍率或优化收益。

本轮可确认的事实与解释：

1. **图形内存不能只看 GPU 进程，也不能只看 canvas 对象。** 64 MiB 第一轮 WebContent 的 vmmap `owned unmapped (graphics)` 打开时 resident 约 89.0 MiB、过滤后约 105.5 MiB；同阶段 GPU 有约 135 MiB 的 footprint 采样峰值。图形记账能解释峰值的一部分，尚未定位到具体合成层/表面，不把整块 WebContent 当 JS 堆。没有因此修改透明窗口、CSS 或应用配置。
2. **全命中行号表有真实容量成本。** 1 GiB fixture 实际 1,073,709,056 B / 17,317,888 行，过滤每轮全部命中。`logengine/src/filter.rs` 的结果为 `Vec<u64>`，元素 payload 138,543,104 B（132.125 MiB）；第一轮过滤前后主进程中位 38.31 → 170.58 MiB，vmmap `Malloc Large` 约 140.4 MiB。结果在 Rust，前端只画 46 行，不能误诊为全文传入浏览器。
3. **关闭后的高 footprint 不等于同量活跃对象。** 1 GiB 恢复空闲主进程约 160.19 MiB；原生快照 `Malloc Large (empty)` resident/dirty 约 132.4 MiB，malloc zones 的活跃分配总量约 15.2 MiB。支持“已释放区域仍被分配器保留”的解释，不足以给每块内存建立历史引用链，也不能解释此前全部编辑器 WebContent 保留。未加定期 purge/强制 GC。
4. 64 MiB / 1 GiB 关闭序列没有持续线性增长；16 MiB quick 则触发约 30.58 MiB 首末增长的调查线（斜率约 9.09 MiB/轮、R² 约 0.724）。原始自动理由当时写“预热后”，但本场景没有预热；现已修正文案。保留 REVIEW，不用另两轮 NO_SIGNAL 抹掉，也不把它直接定为泄漏。
5. 1 GiB 常驻预算尚不能标通过：第一次未过滤阶段中位约 100.79 MiB，后续带使用历史的未过滤阶段中位最高约 233.65 MiB；过滤后也高于历史 200 MB 目标。MB 与 MiB 未悄悄互换。未测前台首屏/滚动，刚生成文件缓存热，不能声称冷启动、60fps 或 <1s 验收已完成。

恢复空闲 CPU 分别约 0.054% / 0.070% / 0.217%，对应 30 / 30 / 15 秒 logical writes 均 4 KiB；主进程恢复线程 16 / 17 / 17。只是本场景结果，不涉及温度或 NAND 寿命。

**后续决策**：[#90](https://github.com/Spc-jgs/lite-ide-mac/issues/90) 保留编辑器引用/分配及长时调查；新增 [#92](https://github.com/Spc-jgs/lite-ide-mac/issues/92) 跟踪高命中结果表容量，先测不同命中率并比较简单表示方案，再改应用。图形合成层峰值尚未定根因，不能按猜测去掉界面效果。#89 仍是独立观察项。

可复用命令：

```bash
scripts/resources.sh --workload logs --profile quick --log-mib 64
scripts/resources.sh --workload logs --profile quick --log-mib 1024 --native-diagnostics
# 持续增长或生命周期变更时再选 full；相同方法/场景匹配后 compare
```

新入口只生成日志，不先循环编辑器；fixture 规模限制为 16/64/1024 MiB、轮数按 profile 有限。并发采样覆盖 open/filter 的操作窗口，核对索引/扫描/命中数，再记录完成窗口；每秒采样仍会遗漏更短峰值。`--native-diagnostics` 在采样窗外读取 vmmap，仍会扰动进程，开关/方法文件 SHA 都进入 scenario；档案保存方法源码，运行中改方法判 INVALID。它不把日志套进“12 文件 300 MiB”线，也不靠已有 NO_SIGNAL 宣称 1 GiB 全面达标。

验证：资源自检 25 项通过，前端测试 116 个文件通过；三项新测试按真实错法确认断言红。新日志入口在隔离真应用跑完 quick 4 轮并保存 REVIEW；`pnpm app:bundle` 离线成功，应用源码/测试构建凭据未变，入口仍 163548 B。未修改 Rust/Svelte 实现，本轮不重复 Rust 全量测试和 smoke，历史通过记录不冒充本轮结果。

其余范围：R03 编辑器真实引用链仍缺失；R04 连续编辑/读放大、R06 重工具、R09 前台首屏/滚动未覆盖。R07/R08 前台分布暂缓，不打断前台；R10–R12 无 JVM 场景不适用；R13/R14 长期容量/故障、R15 风暴、R16 多窗口、R17 恢复、R18 其他协议、R19 热/能耗暂缓，优先缩小内存来源；R22 30–60 分钟组合场景仍待测。历史正常项保持复测条件。

原生解释的外部依据：[WebKit 的内存分类说明](https://docs.webkit.org/Infrastructure/MemoryInspection.html)、[Apple 的 vmmap/heap 分析讲解](https://developer.apple.com/videos/play/wwdc2022/10106/)、[Apple libmalloc 的 large cache 实现](https://github.com/apple-oss-distributions/libmalloc/blob/main/src/magazine_large.c)。公开源码说明存在缓存/延迟回收机制，不代表已核对当前 OS 每项实现；本轮“分配器保留”仍是与实测分类相符的解释。

## 6. 按清单选专项并更新结果

长期检查项、历史正常项的复测条件、当前缺口和每轮记录模板，统一维护在 [资源与性能检查清单](RESOURCE-CHECKLIST.md)。每次资源验收先逐项选覆盖范围，再使用本文的方法与调查线；正常项仍保留回归入口，功能验收不能替代资源测量。人可指定 Rxx，agent 也依据相关改动选择并记录覆盖与跳过理由。

当前优先调查 R03 的内存保留与峰值（#90）；R07/R08 前台延迟/启动分布、R09 1 GiB 日志、R10–R12 工具进程资源、R13 长期磁盘增长、R16/R22 多窗口/长时工作量等各自保留待测状态。R19 的热/能耗/物理写入不由 CPU 或 logical writes 代替。完整定义与执行条件以检查清单为准。

## 依据

- [Rust Book：引用环可能造成内存泄漏](https://doc.rust-lang.org/book/ch15-06-reference-cycles.html)：内存安全不等于无泄漏。
- [Apple：Timers](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/Timers.html)：减少无用唤醒。
- [Apple：Minimize I/O](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/MinimizingIO.html)：减少/合并 I/O，同时满足数据完整性。
- [Apple：App Nap](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/AppNap.html)：后台与前台测量条件不同。
- [Apple XNU：fill_task_rusage](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c) 与本机 SDK `sys/resource.h` / `libproc.h`：计数字段依据。CPU Mach ticks 经 `mach_timebase_info` 转纳秒，自检用真实进程 CPU 时间核验单位。
- [Apple XNU：proc_info.h](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h)、[coalition.h](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/coalition.h)：PRIVATE 查询结构、resource/jetsam 两种编号；只做外部诊断，不能承诺跨系统版本可用。
- [Apple：Improving app responsiveness](https://developer.apple.com/documentation/xcode/improving-app-responsiveness)、[TN2434：Minimizing your app's Memory Footprint](https://developer.apple.com/library/archive/technotes/tn2434/_index.html)：应以交互响应、代表性工作量、分配来源和实测反馈建立改进循环。这里的 200/300 MiB、2%、1 MiB/分钟等具体数字是项目决策，不是这些文档给出的通用标准。
