#!/usr/bin/env bash
# lite-ide 完全卸载脚本
# 用法:
#   ./scripts/uninstall.sh                 # dry-run 预览，不删除任何东西
#   ./scripts/uninstall.sh --yes           # 真正执行（保留项目目录）
#   ./scripts/uninstall.sh --yes --project # 真正执行并删除项目目录本身
#   ./scripts/uninstall.sh --yes --delete-scratches  # 连草稿一起删（默认先把草稿挪到桌面）
#   ./scripts/uninstall.sh --yes --rust    # 连 Rust 工具链一起卸（默认不动：机器上别的东西可能也在用）
#
# 草稿（随手记的笔记）在应用数据目录里（Application Support/<id>/scratches/）。删那个目录之前**默认先把草稿挪到桌面**，
# 原来是一起删、没有任何提醒（issue #61，2026-10-10 整理文档时发现）
set -euo pipefail

YES=0; PROJECT=0; DEL_SCRATCH=0; RUST=0
for a in "$@"; do
  case "$a" in
    --yes) YES=1 ;;
    --project) PROJECT=1 ;;
    --delete-scratches) DEL_SCRATCH=1 ;;
    --rust) RUST=1 ;;
    *) echo "未知参数: $a (支持 --yes / --project / --delete-scratches / --rust)"; exit 1 ;;
  esac
done

APP_ID="com.liteide.app"
# 项目目录从脚本自己的位置推（scripts/ 的上一层）：原来写死 $HOME/playground/lite-ide，换个地方克隆就指错了（#61 顺带发现）
PROJ_DIR="$(cd "$(dirname "$0")/.." && pwd -P)"

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

# 草稿：有就先挪到桌面（不覆盖任何已有的东西），再让下面删整个应用数据目录。空的照常删，不在桌面上留空文件夹
DATA="$HOME/Library/Application Support/$APP_ID"
keep_scratches(){
  local src="$DATA/scratches" n dest
  [ -d "$src" ] || return 0
  n=$(find "$src" -type f | wc -l | tr -d ' ')
  [ "$n" -gt 0 ] || return 0
  if [ "$DEL_SCRATCH" = "1" ]; then
    say "  草稿 $n 份：加了 --delete-scratches，跟着应用数据一起删"
    return 0
  fi
  dest="$HOME/Desktop/lite-ide 草稿（卸载时留下的 $(date +%Y%m%d-%H%M%S)）"
  if [ "$YES" = "1" ]; then
    if [ -e "$dest" ]; then say "  $dest 已经存在，不覆盖 —— 停下，什么都没删"; exit 1; fi
    mkdir -p "$HOME/Desktop"
    mv "$src" "$dest"
    say "  草稿 $n 份挪到了：$dest"
  else
    say "  [dry] 草稿 $n 份将先挪到：$dest（不想留加 --delete-scratches）"
  fi
}

[ "$YES" = "1" ] || say "== DRY-RUN 预览模式：确认无误后加 --yes 执行 =="

say ""
say "[1/4] 应用数据与缓存 ($APP_ID)"
keep_scratches
del "$DATA"
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
# 默认不动：立项时 Rust 是为这个项目装的，现在机器上别的东西可能也在用 —— 卸掉整台机器的工具链不该是卸载一个应用的默认动作（#61 顺带发现）
if [ "$RUST" != "1" ]; then
  say "  保留 Rust 工具链（~/.rustup、~/.cargo）。确定只有这个项目在用，加 --rust 连它一起卸"
else
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
fi

say ""
say "完成。验证：'ls ~/Library | grep -i liteide' 应无结果。"
[ "$RUST" = "1" ] && say "      Rust 那一项：新开终端执行 'command -v cargo' 应无输出。"
true
