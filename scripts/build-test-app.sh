#!/usr/bin/env bash
# 打一份带测试通道的临时身份 .app（com.liteide.mwtest）：Rust 开 test-bridge 特性，前端带 VITE_TEST_BRIDGE=1 挂测试钩子。
# 和你双击的那份 lite-ide.app 是两个身份、两套数据目录（WebKit / Application Support / Logs），测试碰不到你的真实数据。
# 产物：src-tauri/target/test-app.noindex/lite-ide-mwtest.app。用完 `scripts/lib/bridge.sh` 的 lite_teardown 会从 LaunchServices 注销它、删掉它的数据目录；.app 本身留在盘上，下次不用重打。
#
# **为什么挪进 `*.noindex`**（2026-10-13）：原来留在 bundle/macos/ 里、挨着你双击的那份。lite_teardown 每次都注销它，可 Spotlight 照样把它收录、
# 系统又把它登记回去 —— Spotlight 一搜、「打开方式」里都冒出一个 lite-ide-mwtest，用户以为应用改名了。名字以 `.noindex` 结尾的目录 Spotlight 不进，
# 它就只在跑测试那几分钟（`open -a` 时临时登记、退出时注销）存在于系统的登记里
#
# **正式包永远不走这里**：pnpm app:bundle 不带这两样，CI 有哨兵盯着测试钩子不许进正式产物。
set -euo pipefail
cd "$(dirname "$0")/.."
# 覆盖配置：换身份；主窗口不拿焦点（`focus: false`）—— 测试在后台跑（open -g），默认拿焦点的主窗口一出来就会把
# 用户正在用的应用顶下去（2026-10-09 实测）。`--config` 是 JSON Merge Patch，数组整个替换，所以从 tauri.conf.json
# 读出完整的窗口配置、只改 focus 这一项，不在这儿手抄一份（手抄的迟早和正式配置分叉）
CONFIG=$(python3 - <<'PY'
import json
c = json.load(open("src-tauri/tauri.conf.json"))
wins = [dict(w, focus=False) for w in c["app"]["windows"]]
print(json.dumps({"identifier": "com.liteide.mwtest", "productName": "lite-ide-mwtest", "app": {"windows": wins}}))
PY
)
VITE_TEST_BRIDGE=1 pnpm tauri build --features test-bridge --bundles app --config "${CONFIG}"
BUILT="src-tauri/target/release/bundle/macos/lite-ide-mwtest.app"
DEST="src-tauri/target/test-app.noindex"
mkdir -p "${DEST}"
# 删的是上一次打的测试包（我们自己生成的、名字卡死），不是用户的文件
rm -rf "${DEST}/lite-ide-mwtest.app"
mv "${BUILT}" "${DEST}/"
# 原地方的登记跟着注销（tauri 打包时可能已经登记过它）
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "$(pwd)/${BUILT}" >/dev/null 2>&1 || true
echo "测试 .app：$(pwd)/${DEST}/lite-ide-mwtest.app"
