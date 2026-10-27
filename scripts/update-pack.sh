#!/bin/bash
# 打自动更新的包（docs/TAKEOVER.md MVP-8）：.app → lite-ide.app.tar.gz + 签名 + latest.json。
# release.yml 发版时用它；scripts/accept/update.sh 拿测试密钥签一份本机的、验同一条路。
#
#   scripts/update-pack.sh <.app> <版本号> <包的下载地址> <输出目录>
#
# 私钥从环境变量来（tauri signer 认的那几个）：TAURI_SIGNING_PRIVATE_KEY（内容）或 TAURI_SIGNING_PRIVATE_KEY_PATH（文件），
# 有密码的话 TAURI_SIGNING_PRIVATE_KEY_PASSWORD。应用里编着对应的公钥（src-tauri/src/update.rs 的 PUBKEY），对不上的包应用不装。
#
# latest.json 三个平台键都写同一个 universal 包：应用按本机架构找、没有再找 darwin-universal（updsvc::parse_manifest），
# 写全了 Tauri 自己的更新器也认得（万一以后换回去）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
app="$1" ver="${2#v}" url="$3" out="$4"
[ -d "${app}/Contents" ] || { echo "不是 .app：${app}" >&2; exit 2; }
mkdir -p "${out}"
pkg="${out}/lite-ide.app.tar.gz"
# COPYFILE_DISABLE：不往包里塞 ._ 开头的扩展属性文件（macOS 的 tar 默认会带）
COPYFILE_DISABLE=1 tar -czf "${pkg}" -C "$(dirname "${app}")" "$(basename "${app}")"
# stdin 接空：私钥要密码而没给时，签名工具会停下来问 —— 在 CI 里就是挂到超时
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD-}"
(cd "${ROOT}" && pnpm -s tauri signer sign "${pkg}" </dev/null >/dev/null)
python3 -I - "${ver}" "${url}" "${pkg}.sig" "${out}/latest.json" <<'PY'
import json, sys, datetime
ver, url, sig_file, out = sys.argv[1:]
sig = open(sig_file).read().strip()
p = {"url": url, "signature": sig}
doc = {"version": ver, "notes": "", "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
       "platforms": {"darwin-universal": p, "darwin-aarch64": p, "darwin-x86_64": p}}
json.dump(doc, open(out, "w"), ensure_ascii=False, indent=2)
PY
echo "${pkg} ($(du -h "${pkg}" | cut -f1))、${pkg}.sig、${out}/latest.json"
