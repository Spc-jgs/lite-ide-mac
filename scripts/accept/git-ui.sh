#!/usr/bin/env bash
# Git 历史刷新 / 差异 UI。仅测试身份、临时仓库、测试通道；不发系统按键、不抢焦点。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = true ]; then PASS=$((PASS+1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL+1)); printf '  ✗ %s\n' "$2"; fi; }
W=$(mktemp -d /tmp/acGitUI.XXXXXX)
W=$(cd "${W}" && pwd -P)
P="${W}/project"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="${W}/gitconfig"
printf '[user]\n name = Git UI Test\n email = git-ui@example.invalid\n' > "${GIT_CONFIG_GLOBAL}"
trap 'lite_teardown; rm -rf "${W}"' EXIT
mkdir -p "${P}"
git init -q "${P}"
printf 'before\nkeep\n' > "${P}/a.txt"
git -C "${P}" add a.txt
git -C "${P}" commit -qm '测试初始提交'
lite_clean_data
lite_launch "${P}" || exit 1
lite_wait main "return __lite.git.repo === '${P}' && !!__lite.git.status" 15 || { echo "仓库尚未就绪"; lite_eval main "return {root:__lite.project.root,repo:__lite.git.repo,status:__lite.git.status,restricted:__lite.git.restricted}"; exit 1; }
lite menu git-log main >/dev/null
ok "$({ lite_wait main "return document.querySelector('.log .crow')?.textContent.includes('测试初始提交')" 15 && echo true; } 2>/dev/null)" '真实仓库历史载入'
ok "$(lite_eval main "return __lite.exists('刷新 Git','button','exact')")" '底部 Git 工具窗有刷新按钮'
INITIAL=$(git -C "${P}" rev-parse --short HEAD)
printf 'after\nkeep\n' > "${P}/a.txt"
git -C "${P}" add a.txt
git -C "${P}" commit -qm '终端里的新提交'
ok "$({ lite_wait main "return document.querySelector('.log .crow')?.textContent.includes('终端里的新提交')" 15 && echo true; } 2>/dev/null)" '应用保持打开，外部提交经文件监听进入历史'
ok "$(lite_eval main "return document.querySelector('.log .crow.on')?.textContent.includes('测试初始提交')")" '自动刷新保留原选中提交'
lite_eval main "__lite.click('跳到最新提交'); return true" >/dev/null
ok "$({ lite_wait main "return document.querySelector('.log .crow.on')?.textContent.includes('终端里的新提交')" 5 && echo true; } 2>/dev/null)" '跳到最新提交'
lite_eval main "document.querySelector('.log .drow')?.click(); return true" >/dev/null
ok "$({ lite_wait main "return !!document.querySelector('div.diff .grid')" 8 && echo true; } 2>/dev/null)" '真实历史差异默认双栏'
ok "$(lite_eval main "const s=document.querySelector('div.diff .sources'); return s.textContent.includes('提交前') && s.querySelectorAll('.source').length===2")" '比较版本有明确标题'
lite_eval main "[...document.querySelectorAll('div.diff .bar button')].find(b=>b.textContent==='统一').click(); return true" >/dev/null
ok "$({ lite_wait main "return !!document.querySelector('div.diff .uni')" 5 && echo true; } 2>/dev/null)" '真实差异统一视图可切换'
ok "$(lite_eval main "return document.querySelectorAll('div.diff .uni .row.add .no').length===2")" '统一视图保留旧新两列行号'
printf 'working\nkeep\n' > "${P}/a.txt"
ok "$({ lite_wait main "return __lite.git.status.entries.some(e=>e.path==='a.txt' && e.unstaged)" 8 && echo true; } 2>/dev/null)" '外部工作区修改进入更改列表'
lite menu git-changes main >/dev/null
lite_wait main "return !!document.querySelector('.frow[title=\"a.txt\"]')" 8 >/dev/null
lite_eval main "document.querySelector('.frow[title=\"a.txt\"]').dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:120,clientY:180})); return true" >/dev/null
ok "$({ lite_wait main "return !!document.querySelector('[role=menu][aria-label=\"文件的 Git 操作\"]')" 5 && echo true; } 2>/dev/null)" '更改列表的文件 Git 右键菜单'
lite_eval main "__lite.click('暂存文件','menuitem'); return true" >/dev/null
ok "$({ lite_wait main "return __lite.git.status.entries.some(e=>e.path==='a.txt' && e.staged && !e.unstaged)" 8 && echo true; } 2>/dev/null)" '菜单暂存作用到指定文件'
lite_eval main "return await __lite.git.commit('应用里的新提交',false)" >/dev/null
ok "$({ lite_wait main "return document.querySelector('.log .crow')?.textContent.includes('应用里的新提交')" 10 && echo true; } 2>/dev/null)" '应用内提交完成，打开的提交历史自动更新'
ok "$([ "$(git -C "${P}" log -1 --pretty=%s)" = '应用里的新提交' ] && echo true)" '提交确实写入临时仓库'
lite_eval main "__lite.click('刷新 Git'); return true" >/dev/null
ok "$({ lite_wait main "return !__lite.git.busy && document.querySelectorAll('.log .crow').length===3" 8 && echo true; } 2>/dev/null)" '手动刷新展示三次真实提交'
lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
