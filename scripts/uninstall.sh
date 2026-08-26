#!/usr/bin/env bash
# lite-ide 完全卸载脚本
# 用法:
#   ./scripts/uninstall.sh                 # dry-run 预览，不删除任何东西
#   ./scripts/uninstall.sh --yes           # 真正执行（保留项目目录）
#   ./scripts/uninstall.sh --yes --project # 真正执行并删除项目目录本身
set -euo pipefail

YES=0; PROJECT=0
for a in "$@"; do
  case "$a" in
    --yes) YES=1 ;;
    --project) PROJECT=1 ;;
    *) echo "未知参数: $a (支持 --yes / --project)"; exit 1 ;;
  esac
done

APP_ID="com.liteide.app"
PROJ_DIR="$HOME/playground/lite-ide"

say(){ printf '%s\n' "$*"; }
del(){
  local p="$1"
  if [ -e "$p" ]; then
    if [ "$YES" = "1" ]; then rm -rf "$p"; say "  已删除   $p"
    else say "  [dry] 将删除   $p"; fi
  else
    say "  跳过     $p （不存在）"
  fi
}

[ "$YES" = "1" ] || say "== DRY-RUN 预览模式：确认无误后加 --yes 执行 =="

say ""
say "[1/4] 应用数据与缓存 ($APP_ID)"
del "$HOME/Library/Application Support/$APP_ID"
del "$HOME/Library/Caches/$APP_ID"
del "$HOME/Library/WebKit/$APP_ID"
del "$HOME/Library/Preferences/$APP_ID.plist"
del "$HOME/Library/Saved Application State/$APP_ID.savedState"
del "$HOME/Library/Logs/$APP_ID"
for p in "$HOME/Library/HTTPStorages/"$APP_ID*; do
  [ -e "$p" ] && del "$p"
done

say ""
say "[2/4] 已构建的 .app"
del "$HOME/Applications/lite-ide.app"
del "/Applications/lite-ide.app"

say ""
say "[3/4] 项目目录"
if [ "$PROJECT" = "1" ]; then
  del "$PROJ_DIR"
else
  say "  保留项目目录（加 --project 连项目一起删）：$PROJ_DIR"
fi

say ""
say "[4/4] Rust 工具链"
if command -v rustup >/dev/null 2>&1; then
  if [ "$YES" = "1" ]; then rustup self uninstall -y || true
  else say "  [dry] 将执行: rustup self uninstall -y"; fi
else
  say "  未检测到 rustup，跳过"
fi
del "$HOME/.rustup"
del "$HOME/.cargo"
[ -e "$HOME/.crates.toml" ] && del "$HOME/.crates.toml"
[ -e "$HOME/.crates2.json" ] && del "$HOME/.crates2.json"

ZRC="$HOME/.zshrc"
if [ -f "$ZRC" ] && grep -q '\.cargo/env' "$ZRC" 2>/dev/null; then
  if [ "$YES" = "1" ]; then
    cp "$ZRC" "$ZRC.bak-liteide"
    sed -i '' '/\.cargo\/env/d' "$ZRC"
    say "  已从 ~/.zshrc 移除 cargo env 行（备份: ~/.zshrc.bak-liteide）"
  else
    say "  [dry] 将从 ~/.zshrc 删除 cargo env 行"
  fi
fi

say ""
say "完成。验证：新开终端执行 'command -v cargo' 应无输出；'ls ~/Library | grep -i liteide' 应无结果。"
