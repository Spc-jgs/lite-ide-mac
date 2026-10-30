#!/usr/bin/env bash
# 真 .app 的 canvas 生命周期：持有画布引用验尺寸归零，不能让碰巧 GC 掩盖未释放的位图。
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
source "$(dirname "$0")/../lib/bridge.sh"
cd "${LITE_ROOT}"
if pgrep -f "${LITE_APP}/Contents/MacOS" >/dev/null; then
  echo '测试身份正在使用中，不能接管' >&2
  exit 1
fi
export LITE_WORK
trap 'lite_teardown' EXIT
mkdir "${LITE_WORK}/fixture"
python3 - <<'PY'
import os
from pathlib import Path
p = Path(os.environ['LITE_WORK']) / 'fixture/sample.txt'
p.write_text('\n'.join(f'sample row {i} value 123' for i in range(400)))
PY
lite_clean_data
lite_launch "${LITE_WORK}/fixture"
python3 - <<'PY'
import os
from pathlib import Path
import sys
sys.path.insert(0, str(Path.cwd() / 'scripts/lib'))
from resource_guard import js, wait_js
from bridge import call

passed = failed = 0
def check(condition, label):
    global passed, failed
    if condition:
        passed += 1
        print(f'  ✓ {label}', flush=True)
    else:
        failed += 1
        print(f'  ✗ {label}', flush=True)

def toggle(enabled):
    call({'cmd': 'menu', 'id': 'toggle-minimap', 'window': 'main'})
    wait_js(f'return __lite.settings.v.minimap === {str(enabled).lower()}')

def painted(minimum):
    try:
        wait_js(f'return window.__minimapCanvases.length >= {minimum} && !!document.querySelector(".cm-minimap canvas") && window.__minimapCanvases.slice(-2).every(c => c.width > 0 && c.height > 0)')
    except RuntimeError:
        print('画布现场：', js("return {canvases:window.__minimapCanvases.map(c=>({width:c.width,height:c.height,connected:c.isConnected})),minimap:__lite.settings.v.minimap,editors:document.querySelectorAll('.cm-content').length}"), flush=True)
        raise
    return js('return window.__minimapCanvases.length')

wait_js('return __lite.settings.v.minimap === true')
toggle(False)
js('''window.__minimapCanvases = [];
window.__minimapCreate = document.createElement;
document.createElement = function(...args) {
  const el = Reflect.apply(window.__minimapCreate, this, args);
  if (String(args[0]).toLowerCase() === 'canvas') window.__minimapCanvases.push(el);
  return el;
}; return true''')
toggle(True)
path = str((Path(os.environ['LITE_WORK']) / 'fixture/sample.txt').resolve())
import json
def open_file():
    js(f'await __lite.tabflow.openPath({json.dumps(path)}, {{preview:false}}); return true')

open_file()
first = painted(2)
check(first == 2, f'首次挂载只建一组画布（2 张），实际 {first} 张')
check(js("return !!document.querySelector('.cm-minimap canvas')"), '显示画布和离屏瓦片都已实际绘制')
toggle(False)
wait_js("return !document.querySelector('.cm-minimap')")
check(js('return window.__minimapCanvases.every(c => c.width === 0 && c.height === 0)'), '关缩略图：两张画布位图归零（仍持有引用）')
check(js("return !!document.querySelector('.cm-content') && __lite.text().includes('sample row 399')"), '关缩略图后正文仍在')
toggle(True)
second = painted(first + 2)
check(second == first + 2, '重新打开：只创建两张新画布')
check(js(f'return window.__minimapCanvases.slice(0,{first}).every(c => c.width === 0 && c.height === 0)'), '重新打开：旧画布不复活')
call({'cmd': 'menu', 'id': 'close-all-tabs', 'window': 'main'})
wait_js("return __lite.tabs.list.length === 0 && !document.querySelector('.cm-content')")
check(js('return window.__minimapCanvases.every(c => c.width === 0 && c.height === 0)'), '关标签：当前两张位图也归零')
open_file()
third = painted(second + 2)
check(third == second + 2 and js("return __lite.text().includes('sample row 399') && !!document.querySelector('.cm-minimap canvas')"), '再开文件：正文正常，缩略图只建一组')
call({'cmd': 'menu', 'id': 'close-all-tabs', 'window': 'main'})
wait_js("return !document.querySelector('.cm-content')")
check(js('return window.__minimapCanvases.every(c => c.width === 0 && c.height === 0)'), '再次关闭：全部旧画布保持零尺寸')
js('document.createElement = window.__minimapCreate; delete window.__minimapCreate; delete window.__minimapCanvases; return true')
call({'cmd': 'menu', 'id': 'quit', 'window': 'main'})
print(f'通过 {passed} 条，失败 {failed} 条', flush=True)
sys.exit(1 if failed else 0)
PY
