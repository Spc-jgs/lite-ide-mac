# 按需资源验收与架构审计

2026-10-29。目标是保住「秒开、按需起独立进程、用完收掉、不打断」。**由 agent 根据改动判断是否测量，或由人要求；没有日历周期、调度服务或常驻采样器。**

编译器不能替代资源验收。安全 Rust 仍允许引用环等泄漏；它不能判断缓存是否留得太多、定时器是否空转、子进程是否退出、持久化是否写得过勤。前端 GC 同样不能回收仍被引用的无用对象。有限测量只能提供线索，不能证明永远无泄漏。

## 1. 触发与完成标准

1. 人提出测资源、性能、发热、磁盘增长或泄漏时，执行相关场景。
2. 完成大功能，或修改缓存/批量数据、轮询、后台任务、子进程、窗口生命周期、自动保存、历史/日志持久化时，交付前选择测量范围。通常先跑 quick；改生命周期或出现增长，再跑 full 和对应专项。
3. 小范围展示、文案、文档修改可以跳过，交付说明原因。采样方法修改后重新建基线，不能当成产品性能提升。

能在改动前跑就留 before，改后按相同条件比较。没有 before 的第一轮只叫基线。WARN 先复测定位；基线和预算由证据修订，不能为了通过顺手调大。完成标准是报告实际数字、构建身份、发现、未测范围和下一步，UNKNOWN 必须解释。

## 2. 执行

```bash
scripts/resources.sh --self-test
scripts/resources.sh --build --profile quick
scripts/resources.sh --profile full --baseline /path/to/before/report.json
scripts/resources.sh compare /path/to/before/report.json /path/to/after/report.json
```

首次或应用源码/构建变化后带 `--build`，走仓库测试构建、Cargo 离线。不安装依赖、不下载工具或数据；缺本机依赖直接失败，构建日志保留在 `target/resource-tools.noindex/test-build.log`。旧入口 `scripts/mem.sh` 转到这里，`mem.sh 8` 对应 full。

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
| 已修 P1（本地） | JSON 缩进原来只卡输入，现于每次追加前卡输出 8 MiB，超限保留完整原文并说明未格式化 | `crates/httpsvc/src/json.rs::reindent` / `send_capped`、HttpPane；[#85](https://github.com/Spc-jgs/lite-ide-mac/issues/85) | 旧实现 2001 B → 2,004,001 B；6001 B 放大样例验红后修复。httpsvc 22 项通过，含真实 curl 回退原文及输出边界 |
| 已修 P1（本地） | 外部重载曾清掉等待期间的新草稿；现用重载代次、标签身份/路径/模式/指纹阻止过期写回 | `docs.svelte.ts::reloadGuard/checkExternalChanges`、docs-ops；[#84](https://github.com/Spc-jgs/lite-ide-mac/issues/84) | 旧实现在草稿保护断言验红；11 场景覆盖编辑/撤销/保存/改名/关闭重开/模式/编码/逆序及显式重载 |
| 已修 P2（本地） | Git 刷新合并；日志/堆串行采样与回包失效；监听 Err/rescan 显式校准和受限降级 | [#86](https://github.com/Spc-jgs/lite-ide-mac/issues/86)、[#87](https://github.com/Spc-jgs/lite-ide-mac/issues/87)、[#88](https://github.com/Spc-jgs/lite-ide-mac/issues/88)；细节见轮询评审 §7 | 10 次 Git 触发首轮前查询 20→2、整批共 4；采样错误副本验红；watch 17 项。Java 通知不全时提示手动重启，不冒充自动修复所有索引 |
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

## 6. 专项与后续顺序

1. P1 任务日志容量边界已修；新复现的 JSON 格式化输出放大优先补输出上限。性能侧优先对齐日常打开节奏与压力场景、重复确认峰值；同时补键入/切标签/补全/搜索/保存/取消的前台反馈及完成延迟分布（p95/p99），避免只优化内存数字而忽略卡顿。后台自动化不能代替前台交互测量。
2. JVM/LSP、数据库边车、构建工具、任务分别测：启动前 → 使用中 → 停止/关闭 → 等回收；列出本体和工具进程。沿用 `accept/java.sh`、`spring.sh`、`gradle.sh`、`db.sh`、`lsp.sh` 隔离 fixture，不自动下载缺失依赖。普通文件场景不能替这些通过。
3. 多窗口/大项目改动增加反复切项目/开关窗口，以及 30–60 分钟稳定工作量 soak；比较末尾稳定窗口。quick/full 都不是长时泄漏证明。
4. 内存增长用 Instruments Allocations、Leaks、Memory Graph 查归属；CPU 用 Time Profiler；磁盘用 File Activity 查路径/频率。Leaks 覆盖不了仍被引用的无用对象。前台手动测量由人选择时机。
5. 1 GiB 日志、前台 60fps、热启动 <0.5s、每窗口预算逐项验；16/64 MiB 后台场景不替它们背书。默认不启用 sudo 系统追踪、不安装依赖/常驻服务。
6. 补真实规模的 Java 项目下启动/导入/补全/关闭资源采样，分别列 JVM 与壳成本、冷热状态。已有专项验收覆盖部分功能，缺的是与资源数字对齐，并非所有功能都没测。

上面的专项不是每次改动的必跑清单：只在相关路径改动、人要求或实测出现信号时执行。历史/HTTP 全局容量、缓存与轮询项按 §4 的影响证据决定是否排期；当前不扩建成通用监控/缓存/配额平台。后续还需在相关功能改动时关注取消后的残留、磁盘满/断网的恢复、休眠唤醒与长时间运行，优先复用现有验收。

## 依据

- [Rust Book：引用环可能造成内存泄漏](https://doc.rust-lang.org/book/ch15-06-reference-cycles.html)：内存安全不等于无泄漏。
- [Apple：Timers](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/Timers.html)：减少无用唤醒。
- [Apple：Minimize I/O](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/MinimizingIO.html)：减少/合并 I/O，同时满足数据完整性。
- [Apple：App Nap](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/AppNap.html)：后台与前台测量条件不同。
- [Apple XNU：fill_task_rusage](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c) 与本机 SDK `sys/resource.h` / `libproc.h`：计数字段依据。CPU Mach ticks 经 `mach_timebase_info` 转纳秒，自检用真实进程 CPU 时间核验单位。
- [Apple XNU：proc_info.h](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h)、[coalition.h](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/coalition.h)：PRIVATE 查询结构、resource/jetsam 两种编号；只做外部诊断，不能承诺跨系统版本可用。
- [Apple：Improving app responsiveness](https://developer.apple.com/documentation/xcode/improving-app-responsiveness)、[TN2434：Minimizing your app's Memory Footprint](https://developer.apple.com/library/archive/technotes/tn2434/_index.html)：应以交互响应、代表性工作量、分配来源和实测反馈建立改进循环。这里的 200/300 MiB、2%、1 MiB/分钟等具体数字是项目决策，不是这些文档给出的通用标准。
