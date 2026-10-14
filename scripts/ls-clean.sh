#!/usr/bin/env bash
# 从 LaunchServices 注销 lite-ide 的**失效**登记：路径已经不在盘上的那些（只碰 lite-ide.app / lite-ide-mwtest.app，别的应用一概不碰）。
#
# 为什么有这一步（2026-10-13）：用户问「app 的名你是不是改了」—— 查下来名字没改，是系统的登记里攒了一堆 lite-ide：
# `pnpm app:bundle` 打 .dmg 时会临时挂载一个 /Volumes/dmg.XXXX，系统顺手把里面那个 lite-ide.app 记下来，卸载之后记录还在，
# 一次一条，攒到 114 条；「打开方式」里就是一串重复的 lite-ide。外加测试身份的 lite-ide-mwtest 被 Spotlight 收录了（见 build-test-app.sh）。
#
# 还在盘上的不动：你双击的那份、测试 .app 都留着登记。`app:bundle` 末尾自动跑；单独跑也行
set -uo pipefail
LSR=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
gone=0
kept=0
while IFS= read -r p; do
  [ -n "$p" ] || continue
  if [ -e "$p" ]; then
    kept=$((kept + 1))
    continue
  fi
  "$LSR" -u "$p" >/dev/null 2>&1 && gone=$((gone + 1))
done < <("$LSR" -dump 2>/dev/null | sed -nE 's/^[[:space:]]*path:[[:space:]]+(.*\/lite-ide(-mwtest)?\.app) \(0x[0-9a-f]+\)$/\1/p' | sort -u)
echo "LaunchServices：注销了 ${gone} 条已经不在盘上的 lite-ide 登记（还在盘上的 ${kept} 条留着）"
