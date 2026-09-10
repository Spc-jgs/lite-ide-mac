# Lite IDE 完全卸载方案

> 目标：任何时刻都能把这台机器恢复到「没搞过这个项目」的状态，不残留、不搞崩。
> 本项目承诺：**零全局污染** —— 不装全局 npm 包、无 launchd 后台进程、无内核扩展、配置只写标准目录。

## 心智模型：三层结构

```
① 项目目录        ~/playground/lite-ide     （源码 + node_modules + target 构建产物）
② 全局工具链      Rust(rustup/cargo)         （唯一新增的全局东西，可一键自卸载）
③ 应用与其数据    lite-ide.app + 数据目录    （bundle id: com.liteide.app，定位即清除）
```

node/pnpm/Xcode CLT 是机器上本来就有的，**不属于本项目的卸载范围**。

---

## 第 ① 层：项目本体（含全部构建产物）

```bash
cd ~/playground && rm -rf lite-ide
```

- `node_modules/`、`src-tauri/target/`、`dist/` 全部在项目目录内，随目录一起消失。
- 验证：`ls ~/playground/lite-ide` → No such file or directory

## 第 ② 层：Rust 工具链

```bash
rustup self uninstall        # 官方自卸载，清掉 ~/.rustup 和 ~/.cargo，输入 y 确认
```

手动兜底（效果等同）：

```bash
rm -rf ~/.rustup ~/.cargo ~/.crates.toml ~/.crates2.json
# 再编辑 ~/.zshrc 删掉这一行（如有）：
#   . "$HOME/.cargo/env"
cp ~/.zshrc ~/.zshrc.bak && sed -i '' '/\.cargo\/env/d' ~/.zshrc   # 自动方式（自带备份）
```

验证：新开终端 `command -v cargo` → 无输出。

## 第 ③ 层：应用与数据（bundle id 统一为 `com.liteide.app`）

```bash
rm -rf ~/Applications/lite-ide.app            # 默认不装到这儿，但你自己拷过去的话删这里
rm -rf /Applications/lite-ide.app             # 同上

rm -rf ~/Library/"Application Support"/com.liteide.app   # 设置/工作区记录
rm -rf ~/Library/Caches/com.liteide.app                  # 缓存
rm -rf ~/Library/WebKit/com.liteide.app                  # WKWebView 本地存储
rm -f  ~/Library/Preferences/com.liteide.app.plist       # 偏好设置
rm -rf ~/Library/"Saved Application State"/com.liteide.app.savedState
rm -rf ~/Library/HTTPStorages/com.liteide.app*           # 网络存储
rm -rf ~/Library/Logs/com.liteide.app                    # 运行日志，见下
```

验证：`ls ~/Library | grep -i liteide` → 无结果。

### 运行日志：写什么、留多久、怎么删

`~/Library/Logs/com.liteide.app/` 下有两个文件：`app.log` 和轮转出去的
`app.log.1`。实现在 `src-tauri/crates/applog`。

| | |
|---|---|
| 写什么 | **只有异常**：前端未捕获错误与 Promise 拒绝、CSP 违规、Rust panic、启动那一行 |
| 不写什么 | 执行轨迹、文件内容、你打的字。那些归 `diag`（stderr，`LITE_IDE_DEBUG=1` 才开，不落盘） |
| 多大 | 每份 **2MB** 封顶，**合计 4MB，永远不会再长** |
| 留多久 | 不按时间删。满了轮转，只留两代 —— 「上个月崩过一次」是这份文件最值钱的用途 |
| 出得了这台机器吗 | **不**。没有网络代码、没有上报、没有遥测（[PLAN.md](PLAN.md) 立项时就排除了） |
| 怎么清 | 应用里「帮助 → 清空应用日志」，或者直接 `rm` 掉这个目录 |

**直接删是安全的**：下次启动会重新建；应用正开着的时候删掉，写入会静默失败，
下次启动恢复正常 —— 不会崩，也不会连累别的东西。

**里面会有你的路径。** 错误消息和调用栈里带着文件路径（也就带着项目名和用户名）。
这也是「清空应用日志」这条菜单存在的理由 —— 保留策略保证的是「不会长大」，
不是「会自己消失」，要它现在就没，得你自己按一下。

## 一键卸载

```bash
~/playground/lite-ide/scripts/uninstall.sh           # 默认 dry-run，只预览不动手
~/playground/lite-ide/scripts/uninstall.sh --yes     # 真正执行（保留项目目录）
~/playground/lite-ide/scripts/uninstall.sh --yes --project   # 连项目目录一起删
```

脚本会依次清理第③②层并处理 `~/.zshrc`，每步打印做了什么。

---

## 可选收尾

```bash
pnpm store prune        # 清 pnpm 全局下载缓存（不影响任何现有项目，纯回收磁盘）
```

## 开发期「防搞崩」守则（实现时必须遵守）

| 规则 | 原因 |
|---|---|
| 一切 npm 依赖装进项目 `node_modules`，禁止 `-g` | 卸载=删目录，零残留 |
| 不创建 LaunchAgent/LaunchDaemon/登录项 | 无常驻进程，删了就干净 |
| 终端/pty 子进程必须在主窗口退出时一并 kill | 防止孤儿 zsh 占资源 |
| 配置与缓存只写 `com.liteide.app` 标准目录 | 卸载路径确定，一键清除 |
| bundle id 固定为 `com.liteide.app`，不许中途改改 | 保证上面所有路径始终有效 |
