# 打包与发版

## 产物在哪

**是的，全在项目目录里。** 打包不往系统里写任何东西：

```
src-tauri/target/release/
├── lite-ide                                  可执行文件（app:build 产出）
└── bundle/
    ├── macos/lite-ide.app                    4.9 MB   ← 双击启动的就是它
    └── dmg/lite-ide_0.1.0_aarch64.dmg        2.7 MB   ← 发给别人的
```

`target/` 在 `.gitignore` 里，删掉整个项目目录就零残留（见 [UNINSTALL.md](../UNINSTALL.md)）。

## 三条命令的区别

这三个很容易搞混，而搞混的代价是**你以为在测新代码，其实跑的是旧的**：

| 命令 | 产出 | 什么时候用 |
|---|---|---|
| `pnpm app:build` | 只有 `target/release/lite-ide` 这个可执行文件 | 迭代时最快，改完 Rust 想跑一下 |
| `pnpm app:bundle` | `.app` + `.dmg` | 要更新那个能双击的 `.app`，或者要发给别人 |
| `pnpm app:bundle:devtools` | **同一个** `.app` + `.dmg`，但带 Web Inspector | 要用 Safari 的「开发」菜单连上这个 webview 查前端 |

> ⚠️ **调试版和正式版装在同一个路径上。** `app:bundle:devtools` 直接盖掉
> `app:bundle` 的产物 —— 这是有意的（「盘上只留一份 `.app`」），代价是
> 光看文件分辨不出来。**分辨的办法是让它自报家门**：悬停标题栏的项目挂件，
> tooltip 里带着 `⚠︎ 调试版：带 Web Inspector，别拿它当正式版用` 的就是它。
> `app.log` 里每次启动那行 `[budget]` 末尾的 `devtools=1` 也说同一件事。
>
> 为什么要在意：带着 inspector 的那份，**任何本机进程都能附加到这个
> webview 上读写页面**。那是调试能力，不是产品能力 —— 这正是它默认不开
> 的理由。量完记得 `pnpm app:bundle` 打回去。（issue #20）

> ⚠️ **`app:build` 不会更新 `.app`。** `bundle/macos/lite-ide.app` 里那份是上一次
> `app:bundle` 留下的，可能差好几天 —— 而你双击启动的正是它。
>
> 这坑踩过一次：一个已经修好的 bug 又被报上来，照着现象查了半天代码，
> 最后发现跑的是前一天的 `.app`。
>
> **怎么核对**：标题栏上把鼠标停在应用名（或面包屑的项目名）上，提示里有构建时间。
>
> 顺带一提，`cargo clean` 或删掉 `target/` 会把 `.app` 一起带走 —— 重新
> `pnpm app:bundle` 就有了。产物是可再生的，这也是不往系统里装的另一个理由。

## 用它

**不用装。** 打完包直接双击 `src-tauri/target/release/bundle/macos/lite-ide.app`，
Spotlight 也搜得到它。

> 曾经往 `~/Applications` 复制过一份，结果是 Spotlight 里出现**两个**同名
> `lite-ide.app`。两份只要有一次「打包了没重装」就分叉 —— 而你不会知道
> 自己点的是哪一个。盘上只留一份，就没有点错的可能。

要当命令用的话，给二进制做个软链：

```bash
ln -sf "$PWD/src-tauri/target/release/bundle/macos/lite-ide.app/Contents/MacOS/lite-ide" /usr/local/bin/lite
lite ~/some-project
lite huge.log
```

## 关于 Gatekeeper

**自己在本机构建的 `.app` 直接双击就能开** —— 本地产物没有 quarantine 属性
（实测只有 `com.apple.provenance`）。

从网上下载的 `.dmg` 就不一样了。这个项目**没有做代码签名和公证**（个人工具，
不值得为它买 99 美元/年的开发者账号），所以别人第一次打开会被拦：

```bash
# 别人下载后需要执行一次
xattr -dr com.apple.quarantine /Applications/lite-ide.app
```

或者右键 → 打开 → 再点「打开」。**Release 说明里必须写这一句**，否则大多数人
会以为下载的包坏了。

## 发一个版本

### 1. 改版本号（三处必须一致，外加一处自动生成的）

```bash
# package.json · src-tauri/Cargo.toml · src-tauri/tauri.conf.json
```

三处都得改，`.dmg` 的文件名取自 `tauri.conf.json`。

**改完必须在本地跑一次 cargo，把 `src-tauri/Cargo.lock` 一起提交。**
CI 用的是 `cargo test --workspace --locked`，它会拒绝更新 lock 文件：

```
error: cannot update the lock file ... because --locked was passed to prevent this
```

这条踩过一次 —— v0.3.0 第一次打标签，CI 和发版工作流双双在「Rust 测试」
那步红掉，而本地怎么跑都是绿的（本地没有 `--locked`）。
`pnpm app:bundle` 或任意一条 `cargo` 命令都会顺手把 lock 改好：

```bash
cd src-tauri && cargo metadata --format-version 1 > /dev/null
git add src-tauri/Cargo.lock
```

改完再跑一次 `pnpm app:bundle` 确认 `.dmg` 的文件名对得上。

### 1.5 跑一遍 smoke

```bash
./scripts/smoke.sh
```

**用真的 `.app` 点一遍验收清单**（issue #11）：提交（带话多的 pre-commit 钩子）、
保存脚本与软链、切分支、开关大日志。断言尽量落在盘上（`git log` / `stat`），
只有「日志正文有没有乱码」这一条必须读界面 —— 那正是它当初漏掉的地方。

**为什么它值得在发版前跑**：有一整类 bug 只在「真 .app + 真 IPC + 真文件系统」
这条完整链路上才现形。2026-09-07 就靠手工点这一遍逮到一个 ——
一个 26MB 全中文的 UTF-8 日志在界面上整份是乱码，而行数和级别统计全对、
每一层的单测也全绿。那次是手点的，现在固化成脚本了。

要它跑得动，终端需要「辅助功能」权限（第一次会弹窗）。

### 1.6 看一眼预算有没有悄悄涨

```bash
./scripts/budget.sh
```

每次启动都会往 `app.log` 写一行 `[budget]`（见
[ARCHITECTURE.md §7.5](ARCHITECTURE.md)）。这个脚本把它们按
「版本 + 标签数」分组取中位数 —— 发版前扫一眼，**同样开 3 个标签，
这个版本比上个版本的 `self` / `nodes` 涨了多少**。

它不会拦住发版（没有红线，也不该有：单次读数噪声太大）。
它回答的是「发出去之后要不要盯着」。

### 2. 打标签推上去

```bash
git tag -a v0.2.0 -m "v0.2.0：日志命中跳转、崩溃兜底、界面打磨"
git push origin v0.2.0
```

推标签会触发 `.github/workflows/release.yml`：在 GitHub 的 macOS runner 上
构建 **universal（Intel + Apple Silicon 通吃）** 的 `.dmg`，建一个草稿 Release
并把包传上去。

> `release.yml` 里那三个 action 升到 Node 24 那一代
> （`actions/checkout@v7`、`actions/setup-node@v7`、`pnpm/action-setup@v6`，
> [issue #3](https://github.com/Spc-jgs/lite-ide-mac/issues/3)）**已经在
> v0.4.0 上真跑过一次了**（2026-09-02，全绿，universal `.dmg` 5.4MB 正常上传）。
> 这里原来有一段「还没被验证过」的警告，现在可以不用管了。

### 3. 补发布说明，然后发布

草稿建好后去 GitHub 上补说明，确认无误再点 Publish。模板：

```markdown
## 变化

- …

## 安装

下载 `.dmg` → 拖进「应用程序」。

**首次打开会被 Gatekeeper 拦住**（这个包没有签名）。执行一次：

    xattr -dr com.apple.quarantine /Applications/lite-ide.app

或者右键点图标 → 打开 → 再点「打开」。

仅支持 macOS（Intel 与 Apple Silicon 都能跑）。
```

### 本地也能出 universal 包

CI 不可用时可以自己出：

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin
pnpm tauri build --target universal-apple-darwin
# 产物：src-tauri/target/universal-apple-darwin/release/bundle/dmg/
```

注意 universal 包大约是单架构的两倍大。日常自用没必要，`pnpm app:bundle` 就够。

## CI 在做什么

`.github/workflows/ci.yml`，每次 push 和 PR 都跑：

| 步骤 | 为什么 |
|---|---|
| `cargo test --workspace` | 162 条 Rust 测试（含 IPC 两侧 DTO 的一致性检查、菜单与键位表的一致性检查）。另有 1 条 `#[ignore]` 的：它会往跑测试的人的废纸篓里扔文件，得手动跑 |
| `pnpm check` | Svelte + TS 类型检查 |
| 前端纯函数测试 | 324 条断言（diff 解析、双栏对照、泳道布局、冲突解析、改动行标记、行缓存预算、会话快照、模糊匹配排序、键位表） |
| `pnpm build` | 确认前端能构建 |
| 入口包体积门禁 | 超过 **150 KB** 就失败，超过 138 KB 先告警 —— 见下 |
| 开发桩不许进产物 | 浏览器里那份 IPC 桩（`mock-ipc.ts`）真的漏进过生产入口包 1,195 字节，见 .claude/rules/frontend.md |

**为什么要卡入口包体积**：「重的东西不进入口包」是这个项目的一条红线
（CM6 340KB、xterm 250KB、67 个语言包全是按需加载的）。这条红线很容易在
「顺手加个 import」时破掉，而破了之后没有任何症状 —— 只是启动慢了一点，
下次注意到时已经回不去了。所以让 CI 盯着。

跑在 `macos-latest` 上，因为 Tauri 的构建依赖在 Linux 上要另外装一堆 GTK/WebKit，
而这个项目本来就只支持 macOS，没必要为 CI 维护一套用不到的环境。
