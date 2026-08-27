# 使用手册

从源码到 macOS 应用，以及日常怎么用。

---

## 一、日常开发

```bash
pnpm install          # 只需第一次
pnpm app              # 起 dev server 并打开窗口，改前端热更新
```

只改前端时不必重编译壳 —— 浏览器里打开 <http://localhost:1420> 就能调 UI，
会自动挂上一个 IPC 桩喂假数据（`src/lib/dev/mock-ipc.ts`），秒级热更新。
生产构建里这个桩会被 tree-shake 掉，一个字节都不会带进去。

---

## 二、打包成 App

```bash
pnpm app:bundle       # 生成 .app 与 .dmg
```

产物在：

```
src-tauri/target/release/bundle/
├── macos/lite-ide.app                    4.6 MB
└── dmg/lite-ide_0.1.0_aarch64.dmg        2.4 MB
```

只想要可执行文件、不打包（迭代时更快）：

```bash
pnpm app:build        # 只编译，产物 src-tauri/target/release/lite-ide
```

---

## 三、安装

```bash
cp -r src-tauri/target/release/bundle/macos/lite-ide.app ~/Applications/
```

放 `~/Applications`（当前用户）或 `/Applications`（全机器）都行 ——
`UNINSTALL.md` 两处都会清理。

### 关于 Gatekeeper

这个 app 用的是 **adhoc 临时签名**，没有开发者证书、没有公证
（自用工具，PLAN 里就定了不做签名分发）。

**自己本地构建的不受影响**：quarantine 标记是文件"从网络下载"时才会被打上的，
本地编译出来的没有，双击直接能开。实测确认过。

**如果把 .dmg 传到另一台机器**（AirDrop、网盘、U 盘都算），
那份文件会带上 quarantine，首次打开会弹"无法验证开发者"。两种处理：

```bash
# 办法一：去掉 quarantine 标记（推荐，一次就好）
xattr -dr com.apple.quarantine /Applications/lite-ide.app
```

办法二：在 Finder 里 **右键点图标 → 打开**，弹窗里再点一次"打开"。
之后双击就正常了。

---

## 四、命令行启动

装好之后可以直接从终端开：

```bash
open -a lite-ide                          # 空窗口
open -a lite-ide ~/some-project           # 打开一个项目目录
open -a lite-ide /var/log/system.log      # 打开单个文件
```

也可以给二进制做个软链，更顺手：

```bash
ln -sf ~/Applications/lite-ide.app/Contents/MacOS/lite-ide /usr/local/bin/lite
lite ~/some-project
lite huge.log
```

传目录 → 作为项目根打开；传文件 → 打开它并把父目录当项目根。

---

## 五、快捷键

### 全局

| 键 | 作用 |
|---|---|
| 双击 `⇧` | 随处搜索（文件 / 内容 / 操作，Tab 切范围） |
| `⌘P` | 按文件名找文件 |
| `⌘⇧F` | 在项目中搜内容 |
| `⌘⇧O` | 当前文件的结构大纲 |
| `⌘S` | 保存 |
| `⌘W` | 关闭当前标签 |
| `⌘1` | 收起 / 展开侧边栏 |
| `⌘J` | 收起 / 展开终端面板 |
| `⌃⇧\`` | 新建终端 |
| `Esc` | 关掉浮窗 |

搜索浮窗里：`↑` `↓` 选择、`↵` 打开、`Tab` 换范围、`Esc` 关闭。

### 编辑器

标准的 CodeMirror 键位：`⌘Z` / `⌘⇧Z` 撤销重做、`⌘F` 查找、`⌘/` 注释、
`⌥↑` `⌥↓` 移动行、`⌘D` 选中下一个相同词、`⌥` + 点击加多光标。

### 日志视图

`↑` `↓` 逐行、`PageUp` `PageDown` 翻页、`Home` `End` 首尾。

---

## 六、两种模式

打开文件时会自动判定：

| 条件（任一满足） | 模式 |
|---|---|
| 大于 32MB | 日志模式（只读） |
| 估算超过 30 万行 | 日志模式 |
| 存在超过 1 万字符的长行 | 日志模式 |
| 含 NUL 字节（二进制） | 日志模式 |
| 其余 | 编辑模式 |

**自动判定只是默认值，不是死判决** —— 状态栏左下角的「编辑模式 ⇄ / 日志模式 ⇄」
点一下就能切。切到编辑模式时如果文件超过 8MB 会先问一句，
因为编辑模式要把全文读进内存交给 CodeMirror。

日志模式里有级别 chips 过滤、文本搜索、跟随尾部（tail）；
编辑模式支持 67 种语言的语法高亮，Markdown 还带 live preview。

---

## 七、升级

重新打包覆盖即可：

```bash
pnpm app:bundle
rm -rf ~/Applications/lite-ide.app
cp -r src-tauri/target/release/bundle/macos/lite-ide.app ~/Applications/
```

没有自动更新器 —— 自用工具不值得为它引入一套更新基础设施，
也就没有"更新服务器挂了怎么办"这类问题。

---

## 八、卸载

见 [UNINSTALL.md](../UNINSTALL.md)。一句话版本：

```bash
./scripts/uninstall.sh            # 先 dry-run 看要删什么
./scripts/uninstall.sh --yes      # 确认后执行
```

本项目承诺零全局污染：不装全局 npm 包、无 LaunchAgent、无常驻进程
（终端子进程随窗口退出，有单测卡着），配置只写 `com.liteide.app` 标准目录。
