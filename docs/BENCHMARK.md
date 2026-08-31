# M0 性能实测

复现：

```bash
# 造 1GB 测试日志（Java 应用形态，含异常堆栈）
cargo run -p logengine --release --example gen_log -- /tmp/big.log 1073741824
# 跑基准
cargo run -p logengine --release --example bench -- /tmp/big.log
```

## 引擎侧结果

机器：Apple Silicon / macOS 25.6 · rustc 1.98.0 release
样本：**1.00 GB / 9,141,707 行**（约 914 万行）

| 指标 | 实测 | 出口标准 | 余量 |
|---|---|---|---|
| ① 打开耗时 | **0.38 – 1.76 ms** | < 1000 ms | 500×+ |
| ② 首屏 50 行读取 | **0.008 – 0.02 ms** | 无感即可 | — |
| ③ 全量索引（后台，不阻塞交互） | 145 ms 热 / 557 ms 冷 | — | 6.87 / 1.79 GB/s |
| ④ 稀疏索引占用 | **69.8 KB** | 与文件大小无关 | 全量偏移需 69.7MB，省 **1024×** |
| ⑤ 随机定位单行 | **5.1 – 5.4 μs** | < 10 μs（设计值） | 达标 |
| ⑥ 顺序读 512 行/块 | **0.02 ms/块** | < 16 ms（60fps 单帧预算） | 800× |
| 顺序读吞吐 | 2983 – 3181 MB/s | — | — |

③ 的冷热差异说明：冷缓存（首次从磁盘读）约 557ms，热缓存（page cache 命中）约 145ms。
后者接近 `memchr` 的理论速度，说明加上 `MADV_SEQUENTIAL` 之后瓶颈已回到内存带宽。
两种情况下索引都在后台线程跑，不阻塞首屏与滚动。

## 应用侧结果（GUI 实测）

打开同一个 1GB 日志，稳定态：

| 指标 | 实测 | 出口标准 |
|---|---|---|
| 主进程 phys_footprint（含 mmap 引擎） | 36 MB | — |
| WebKit.GPU | 16 MB | — |
| WebKit.WebContent（渲染） | 46 MB | — |
| **合计常驻内存** | **98 MB** | < 200 MB ✅ |
| 进程数 | **4** | ≤ 5 ✅ |
| 二进制体积 | **3.8 MB** | 安装包约 10MB ✅ |

数字取自 `pnpm app:build` 产出的**生产二进制**，且确认没有任何 dev server 在跑
（原因见下面「坑四」）。M1 界面元素比 M0 多（chips、分段着色的 span），
WebContent 因此比 M0 时略高。

注：主进程 RSS 会显示约 1.1GB —— 那是 mmap 的 file-backed 页，内核可随时回收，
不是真实内存压力。`phys_footprint`（活动监视器口径）才是可比数字。
WebKit 的 GPU / Networking 子进程空闲后会被系统回收，不计入稳定态。

对照：VSCode 空载 650MB+，打开 1GB 日志直接卡死。

④ 的 1024× 正好等于 `DEFAULT_STRIDE` —— 稀疏索引按设计生效，没有隐藏开销。

前端产物：**62 KB JS / 21 KB gzip**（Svelte 5 + Vite，无 SvelteKit）。

---

## M1 过滤性能

同一个 1GB / 914 万行样本，过滤全程在后台线程跑，界面不阻塞：

| 过滤条件 | 耗时 | 命中 |
|---|---|---|
| 仅 ERROR（纯级别查表） | **86 ms** | 456,822 |
| 仅 INFO（大头，考验分配） | **93 ms** | 5,026,804 |
| 文本 `OrderService`（区分大小写） | **158 ms** | 1,143,465 |
| 文本 `orderservice`（不区分） | **284 ms** | 1,143,465 |
| ERROR + 文本 `Deadlock` | **98 ms** | 456,822 |
| 文本无命中（全扫不中） | **117 ms** | 0 |

纯级别过滤之所以能压到 86ms，是因为每行级别已经在扫描阶段存成 4bit 数组，
过滤只是遍历内存，不碰文件。

索引阶段的两个任务并行：

| 阶段 | 耗时 | 说明 |
|---|---|---|
| 打开 | 0.49 ms | mmap 是 O(1) |
| 行索引 | 143 ms | 6.97 GB/s，关键路径 |
| 级别扫描 | 88 ms | 与索引并行，不阻塞 |
| 级别表内存 | 4.4 MB | 每行 4 bit |

---

## 实现中撞到的两个坑

两个都不是选型问题，是实现细节 —— 但任何一个不修，「秒开」都是假的。

### 坑一：后台索引线程把读者饿死了

**症状**：首屏 50 行读取耗时 **1112 ms**，几乎正好等于全量索引耗时。
「打开只要 1.9ms」在体验上完全没有兑现 —— 窗口是开了，但一行都读不出来。

**原因**：最初用 `RwLock<LineIndex>`，后台线程分 16MB 块推进，每块结束释放写锁。
以为「每 3ms 让一次锁」读者就能插进去 —— 实际上写者放锁后立刻重新申请，
读者在竞争中根本抢不到。分块让锁是无效的缓解手段。

**修法**：改成**后台无锁构建 + 快照发布**。
后台线程在自己的 `LineIndex` 上扫描，全程不持锁；每块结束才把一份克隆塞进
`Mutex<Arc<LineIndex>>`。读者只需克隆 Arc（O(1)）就脱离锁去查。

**结果**：1112 ms → **0.008 ms**。

代价是每块一次约 70KB 的 memcpy（64 次共约 2MB），完全可以忽略。

### 坑二：mmap 顺序扫描不给内核提示，吞吐掉一半

**症状**：全量索引 1294 ms，吞吐仅 0.77 GB/s —— 而 `memchr` 本身能跑 5–10 GB/s。

**原因**：1GB 映射按 4KB 页算是约 26 万次 minor page fault，
即使数据已在 page cache 里，逐页建立映射的开销也吃掉了大部分时间。
瓶颈不在扫描算法，在缺页。

**修法**：`mmap.advise(Advice::Sequential)`，告诉内核这是一次从头到尾的顺序访问，
让它按需预读并及时回收。

**结果**：1294 ms → **557 ms**，吞吐 0.77 → **1.79 GB/s**。

---

## 结论

M0 的技术路线**通过验证**：mmap + 稀疏索引 + 二进制 IPC 这条链路，
在 1GB 日志上把「打开」压到 2ms 量级，索引内存恒定在 KB 级，
读取延迟比 60fps 单帧预算低两到三个数量级。

引擎不是瓶颈，后续帧率取决于前端渲染。


### 坑三：改了前端却毫无变化 —— `cargo build` 不知道 dist 变了

**症状**：M1 期间反复出现「界面改了、重新编译了、跑起来还是旧的」。
一度误判成日志文件的 fd 竞争，实际根因完全不同。

**原因**：只改前端时，`pnpm build` 更新了 `dist/`，但 `cargo build` 看 Rust 代码
没动就整个跳过编译 —— 二进制里嵌的还是上一版前端。时间戳一比就露馅：
二进制 10:45:09，dist 10:47:08。

**修法**：`build.rs` 里加一行 `println!("cargo:rerun-if-changed=../dist");`，
并把完整构建路径固化成 npm script（`pnpm app` / `pnpm app:build`），
不再手工分两步跑。

**教训**：这个坑之所以难查，是因为它的表现是「代码没生效」而不是「报错」——
所有信号都指向前端逻辑有问题，实际上前端代码根本没被装进去。
遇到「改了没反应」，先比对产物时间戳，再怀疑代码。


### 坑四：`cargo build` 产出的是「dev 模式」二进制，一直在偷偷连 dev server

**症状**：停掉 vite dev server 之后，app 启动即白屏 —— 窗口在、WebKit 子进程在、
`run()` 也进了，但 `page_load` 事件从不触发，前端一行代码都没跑。

**原因**：Tauri 的 dev / prod 之分**不看 cargo profile**，而由 `tauri build` 命令决定。
`cargo build --release` 编译出来的二进制里，窗口 URL 仍是 `http://localhost:1420/`
（配置里的 `devUrl`），而不是嵌入资源的 `tauri://localhost`。

```
[boot] main 窗口 url = http://localhost:1420/     ← cargo build --release
[boot] main 窗口 url = tauri://localhost          ← pnpm tauri build
```

**代价**：此前几次「端到端验证通过」其实都是经由当时还开着的 dev server 加载前端的，
并没有验证到真正的生产二进制。这个误判持续了整个 M0 到 M1 中段。

**修法**：构建路径固化成 `pnpm app:build`（`tauri build --no-bundle`），
不再手工跑 `cargo build`。验证前先确认 `curl localhost:1420` 不通，排除 dev server 干扰。

**顺带逮到一个真 bug**：换成生产二进制后立刻冒出
`Command plugin:window|start_dragging not allowed by ACL` ——
标题栏的 `data-tauri-drag-region` 缺 `core:window:allow-start-dragging` 权限，
也就是拖标题栏挪不动窗口。这个缺陷在 dev 模式下没暴露。

**教训**：验证环境与交付环境的差异，会让一整串「验证通过」变得没有意义。
凡是靠外部进程（dev server）才能跑起来的验证，都要先确认那个进程在最终形态里也存在。


---

# ⌘P 模糊匹配（2026-08-31）

日志引擎之外唯一进过基准的热点。它跑在**每一次击键**上，预算是 60fps 的 16ms 单帧。

## 基准

5 万条路径（`MAX_FILES` 上限，形态接近真实 Java 单体仓库）：

```ts
import { rank } from "../src/lib/search/fuzzy.ts";
const segs = ["src", "lib", "components", "utils", "internal", "api", "model", "test"];
const files: string[] = [];
for (let i = 0; i < 50_000; i++)
  files.push(`${segs[i % 8]}/${segs[(i >> 3) % 8]}/${segs[(i >> 6) % 8]}/File${i}Handler.ts`);
for (const q of ["a", "ha", "handler", "sluhandler"]) {
  rank(files, q, (f) => f, 40); // 预热
  const t = performance.now();
  for (let k = 0; k < 5; k++) rank(files, q, (f) => f, 40);
  console.log(`${q}: ${((performance.now() - t) / 5).toFixed(1)} ms`);
}
```

## 结果

| query | 改前 | 改后 |
|---|---|---|
| `a` | 13.2 ms | 4.8 ms |
| `ha` | 12.7 ms | 5.2 ms |
| `handler` | **20.7 ms** | **8.2 ms** |
| `sluhandler`（命中少） | 8.9 ms | 3.7 ms |

query 越长越慢，因为内层扫描的候选字符数是「文本长度 × query 长度」量级。

## 时间花在哪

改前拆段（5 万条）：

| 段 | 耗时 |
|---|---|
| 只做 `toLowerCase` | 1.3 ms |
| 完整 `fuzzyMatch` | **13.4 ms** |
| 全量排序 5 万条 | 5.7 ms |
| （对照）5 万次 `indexOf` | 0.9 ms |

一开始猜是 `toLowerCase()` 的分配，**猜错了** —— 它只占 1.3ms。热点在
`isBoundary`：它对每一个候选字符做 `prev.toLowerCase()` 和 `s[i].toLowerCase()`，
每次分配一个单字符串。

## 三处改动，各值多少

| 改动 | `handler` |
|---|---|
| 起点 | 20.7 ms |
| `isBoundary` 改成码点比较，不再分配 | 9.9 ms |
| `SEP.has(prev)` 展开成比较链（它在最内层） | 7.3 ms |
| 全量 `sort` → 有界插入（排序 5.7ms → 0.3ms） | 8.2 ms 附近波动 |

最后一行看着变慢是测量噪声 —— 分段量的时候 `rank` 与 `fuzzyMatch` 的差
（也就是排序开销）从 5.7ms 降到了 0.3ms，这一项是实的。

## 两条不能丢的性质

- **同分保持输入顺序。** 候选按目录序来，同分打乱表现为「同一个 query
  两次结果不一样」，而人是靠肌肉记忆按第几条的。有界插入必须插在同分项**之后**。
- **对原来就能匹配的候选，选出的位置和分数一个字节都不能变。**
  完备性修复（见 JOURNAL M21 / issue #1）靠的正是这条：新加的上界只在
  原来会失败的候选上起作用。
