#!/bin/bash
# 自动更新（docs/TAKEOVER.md MVP-8）在真 .app 上从头走到尾：读 latest.json、比版本、下载、验签、状态栏出「重启更新」、
# 点了换 .app、存好现场退出、起新的、新的起来后把旧的清走。
# 走测试通道：不发全局按键、不抢焦点（scripts/lib/bridge.sh）。不上网：「发布」是本机的文件（file://），
# 密钥是这次现生成的测试密钥（应用里编的是正式公钥；测试构建认 LITE_UPDATE_PUBKEY），装的是测试应用的一份拷贝（原来那份不动），
# 换下来的旧版本挪进临时目录（LITE_UPDATE_TRASH），不往你的废纸篓里扔
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
done_() { printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"; [ "${FAIL}" = 0 ]; exit $?; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

[ -d "${LITE_APP}" ] || { echo "没有测试 .app，先跑 scripts/build-test-app.sh" >&2; exit 2; }
stale=$(lite_stale)
[ -n "${stale}" ] && { echo "测试 .app 比源码旧（${stale}）：先跑 scripts/build-test-app.sh" >&2; exit 2; }

W="${LITE_WORK}/upd"
NAME=$(basename "${LITE_APP}")
mkdir -p "${W}/inst" "${W}/new" "${W}/srv" "${W}/trash"
cleanup() {
  lite_teardown
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "${W}/inst/${NAME}" >/dev/null 2>&1
}
trap cleanup EXIT

# 「装着的」：测试应用的一份拷贝；「新版本」：再一份，多一个记号文件，重新做一遍 ad-hoc 签名（改了包里的东西，原来的签名封不住了）
cp -R "${LITE_APP}" "${W}/inst/"
cp -R "${LITE_APP}" "${W}/new/"
echo "new" > "${W}/new/${NAME}/Contents/Resources/upd-marker"
codesign --force --deep -s - "${W}/new/${NAME}" 2>/dev/null

# 测试密钥（没密码），用发版的同一个脚本打包、签名、写 latest.json
(cd "${LITE_ROOT}" && pnpm -s tauri signer generate --ci -p "" -w "${W}/k.key" </dev/null >/dev/null 2>&1)
TAURI_SIGNING_PRIVATE_KEY_PATH="${W}/k.key" "${LITE_ROOT}/scripts/update-pack.sh" "${W}/new/${NAME}" 99.0.0 "file://${W}/srv/lite-ide.app.tar.gz" "${W}/srv" >/dev/null 2>&1
[ -f "${W}/srv/latest.json" ] || { echo "打包 / 签名没成（scripts/update-pack.sh）" >&2; exit 1; }
cp "${W}/srv/latest.json" "${W}/good.json"
# 被换过的包：签名还是原来那份，包里改了一个字节（下载地址被劫持、镜像被换）
cp "${W}/srv/lite-ide.app.tar.gz" "${W}/srv/bad.tar.gz"
python3 -I - "${W}/srv/bad.tar.gz" <<'PY'
import sys
p = sys.argv[1]; b = bytearray(open(p, "rb").read()); b[len(b) // 2] ^= 1; open(p, "wb").write(b)
PY
manifest() { python3 -I - "${W}/good.json" "${W}/srv/latest.json" "$1" "$2" <<'PY'
import json, sys
src, out, ver, pkg = sys.argv[1:]
d = json.load(open(src)); d["version"] = ver
for p in d["platforms"].values(): p["url"] = p["url"].rsplit("/", 1)[0] + "/" + pkg
json.dump(d, open(out, "w"))
PY
}
manifest 0.0.1 lite-ide.app.tar.gz   # 先摆一份「比现在旧」的：启动后那次自动查看到的是它

# 这几个环境变量 `open` 会带给应用（bridge.sh 里那段注释），应用重启自己（open -n）时又带给新的
export LITE_UPDATE_URL="file://${W}/srv/latest.json"
LITE_UPDATE_PUBKEY=$(cat "${W}/k.key.pub"); export LITE_UPDATE_PUBKEY
export LITE_UPDATE_DELAY_MS=1500 LITE_UPDATE_TRASH="${W}/trash"
CACHE="${HOME}/Library/Caches/${LITE_ID}/updates"
INST="${W}/inst/${NAME}"
LITE_APP="${INST}" lite_launch || exit 1
st() { lite_eval main "const u = await window.__TAURI_INTERNALS__.invoke('$1'); return [u.phase, u.version, u.error].join('|')"; }

echo "▸ 查：比现在旧的不下"
wait_for 10 '[ "$(st update_status | cut -d"|" -f1)" = latest ]'
ok "$([ "$(st update_status | cut -d'|' -f1)" = latest ] && echo 1)" "启动后自己查了一次（设置默认开着）：发布的是 0.0.1，说已是最新"
ok "$([ -z "$(ls "${CACHE}" 2>/dev/null | grep '\.tar\.gz$')" ] && echo 1)" "没下东西"

echo "▸ 包被换过：不认"
manifest 99.0.0 bad.tar.gz
R=$(st update_check)
ok "$([[ "${R}" == failed* && "${R}" == *签名校验失败* ]] && echo 1)" "签名对不上就不认（${R%%|*}：${R##*|}）"
ok "$([ ! -f "${CACHE}/lite-ide-99.0.0.app.tar.gz" ] && echo 1)" "没留下一个「下好了」的包"
ok "$(lite_eval main "return __lite.has('重启以更新到')" | grep -q false && echo 1)" "状态栏上什么都没出"

echo "▸ 真的新版：下好、验过，状态栏出「重启更新」"
manifest 99.0.0 lite-ide.app.tar.gz
R=$(st update_check)
ok "$([[ "${R}" == ready\|99.0.0* ]] && echo 1)" "下好了、验过签名（${R}）"
ok "$([ -f "${CACHE}/lite-ide-99.0.0.app.tar.gz" ] && [ ! -f "${CACHE}/lite-ide-99.0.0.app.tar.gz.part" ] && echo 1)" "包在应用缓存目录里，下了一半的那份收掉了"
wait_for 5 'lite_eval main "return __lite.has(\"重启以更新到 99.0.0\")" | grep -q true'
ok "$(lite_eval main "return __lite.has('重启以更新到 99.0.0')" | grep -q true && echo 1)" "状态栏：「重启以更新到 99.0.0」"
ok "$([ ! -f "${INST}/Contents/Resources/upd-marker" ] && echo 1)" "点之前一个字节都没换"

echo "▸ 点「重启更新」：换、退、起新的"
OLD=$(pgrep -f "${INST}/Contents/MacOS" | head -1)
lite_eval main "__lite.click('重启以更新到 99.0.0'); return true" >/dev/null
wait_for 15 '! kill -0 '"${OLD}"
ok "$(! kill -0 "${OLD}" 2>/dev/null && echo 1)" "旧的进程退了（照 ⌘Q 的路：存好现场）"
wait_for 20 '[ -n "$(pgrep -f "${INST}/Contents/MacOS")" ]'
NEW=$(pgrep -f "${INST}/Contents/MacOS" | head -1)
ok "$([ -n "${NEW}" ] && [ "${NEW}" != "${OLD}" ] && echo 1)" "原来的位置上起了一个新进程（pid ${OLD} → ${NEW:-没有}）"
ok "$([ "$(cat "${INST}/Contents/Resources/upd-marker" 2>/dev/null)" = new ] && echo 1)" "原来的位置上是新版本（记号文件在）"
OLDAPP="${W}/inst/.lite-ide-old-${OLD}.app"
lite_wait main "return true" 20 >/dev/null 2>&1
ok "$(lite_eval main "return 1" 2>/dev/null | grep -q 1 && echo 1)" "新的那个应用起得来、窗口在（测试通道答话）"
echo "▸ 新的起来之后：旧的清走（测试构建里挪进临时目录，正式包是废纸篓）"
wait_for 10 '[ -d "${W}/trash/.lite-ide-old-${OLD}.app" ]'
ok "$([ -d "${W}/trash/.lite-ide-old-${OLD}.app" ] && [ ! -e "${OLDAPP}" ] && echo 1)" "换下来的旧版本挪走了，应用旁边不剩东西"
ok "$([ ! -f "${W}/trash/.lite-ide-old-${OLD}.app/Contents/Resources/upd-marker" ] && echo 1)" "挪走的是旧的那份"

done_
