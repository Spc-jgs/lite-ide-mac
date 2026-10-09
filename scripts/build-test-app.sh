#!/usr/bin/env bash
# 打一份带测试通道的临时身份 .app（com.liteide.mwtest）：Rust 开 test-bridge 特性，前端带 VITE_TEST_BRIDGE=1 挂测试钩子。
# 和你双击的那份 lite-ide.app 是两个身份、两套数据目录（WebKit / Application Support / Logs），测试碰不到你的真实数据。
# 产物：src-tauri/target/release/bundle/macos/lite-ide-mwtest.app。用完 `scripts/lib/bridge.sh` 的 lite_teardown 会从 LaunchServices 注销它、删掉它的数据目录；.app 本身留在盘上，下次不用重打。
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
echo "测试 .app：$(pwd)/src-tauri/target/release/bundle/macos/lite-ide-mwtest.app"
