#!/usr/bin/env bash
# 推送到一个要账号密码的 HTTP(S) 远程、钥匙串里还没有它的凭据（#62，gitsvc::askpass）：git 要凭据时问到界面上。走测试通道（scripts/lib/bridge.sh）。
# 先 scripts/build-test-app.sh。对面是脚本自己起的 git 服务器：Python 只做 Basic 认证，认过了交给 git 自带的 `git http-backend`（只听 127.0.0.1）。
#
# **不碰你的钥匙串**：macOS 的 git 默认 credential.helper=osxkeychain（Xcode 那份系统级配置里），认证成功后 git 会把账号密码存进去。
# 这里 GIT_CONFIG_NOSYSTEM=1、GIT_CONFIG_GLOBAL 指到一个临时文件 —— `open` 会把调用者的环境带给测试应用（bridge.sh 里写着），
# 它起的 git 一个凭据助手都没有（实测 `git config --get-all credential.helper` 在这个环境下一条都没有）。账号密码是现生成的测试值。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS_N=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS_N=$((PASS_N + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

W=$(cd "$(mktemp -d /tmp/acGa.XXXX)" && pwd -P)
P="${W}/shop"; SRV="${W}/srv"
SRV_PID=""
trap 'lite_teardown; [ -n "${SRV_PID}" ] && kill "${SRV_PID}" 2>/dev/null; rm -rf "${W}"' EXIT
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="${W}/gitconfig"
printf '[user]\n\tname = acc\n\temail = acc@example.com\n[init]\n\tdefaultBranch = main\n' > "${GIT_CONFIG_GLOBAL}"
GU=acc; GP="T$(python3 -c 'import secrets; print(secrets.token_hex(8))')"

mkdir -p "${SRV}"
git init -q --bare "${SRV}/shop.git"
cat > "${W}/srv.py" <<'EOF'
import base64, os, subprocess, sys
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
ROOT, USER, PASS, PORTF, LOG = sys.argv[1:6]
WANT = "Basic " + base64.b64encode(f"{USER}:{PASS}".encode()).decode()
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def go(self):
        auth = self.headers.get("Authorization", "")
        good = auth == WANT
        open(LOG, "a").write(f"{self.command} {self.path.split('?')[0]} {'ok' if good else ('none' if not auth else 'bad')}\n")
        if not good:
            self.send_response(401); self.send_header("WWW-Authenticate", 'Basic realm="acc"'); self.send_header("Content-Length", "0"); self.end_headers(); return
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n) if n else b""
        path, _, qs = self.path.partition("?")
        env = dict(os.environ, GIT_PROJECT_ROOT=ROOT, GIT_HTTP_EXPORT_ALL="1", PATH_INFO=path, QUERY_STRING=qs, REQUEST_METHOD=self.command,
                   CONTENT_TYPE=self.headers.get("Content-Type", ""), CONTENT_LENGTH=str(len(body)), REMOTE_USER=USER, REMOTE_ADDR="127.0.0.1")
        if self.headers.get("Content-Encoding"):
            env["HTTP_CONTENT_ENCODING"] = self.headers["Content-Encoding"]
        out = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True).stdout
        sep = b"\r\n\r\n" if b"\r\n\r\n" in out else b"\n\n"
        head, _, rest = out.partition(sep)
        status, hdrs = 200, []
        for line in head.decode().splitlines():
            k, _, v = line.partition(":")
            if k.lower() == "status":
                status = int(v.split()[0])
            elif k:
                hdrs.append((k, v.strip()))
        self.send_response(status)
        for k, v in hdrs:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(rest))); self.end_headers(); self.wfile.write(rest)
    do_GET = do_POST = go
s = ThreadingHTTPServer(("127.0.0.1", 0), H)
open(PORTF, "w").write(str(s.server_address[1]))
s.serve_forever()
EOF
python3 -I "${W}/srv.py" "${SRV}" "${GU}" "${GP}" "${W}/port" "${W}/req.log" & SRV_PID=$!
wait_for 10 "[ -s '${W}/port' ]" || { echo "git 服务器起不来"; exit 1; }
PORT=$(cat "${W}/port")

git init -q "${P}"
printf 'hello\n' > "${P}/a.txt"
git -C "${P}" add a.txt && git -C "${P}" commit -qm "第一次"
git -C "${P}" remote add origin "http://127.0.0.1:${PORT}/shop.git"
ok "$([ -z "$(git -C "${P}" config --get-all credential.helper)" ] && echo 1)" "前提：这个环境下 git 一个凭据助手都没有（测试账号密码不会进你的钥匙串）"

lite_clean_data
lite_launch "${P}" || exit 1
lite_wait main "return __lite.project.root === '${P}'" 15 >/dev/null
sleep 3   # 等 Git 认出仓库（信任检查、status 走完）

DLG='document.querySelector(".dialog.askpass")'
push_now() {
  lite menu git-push main >/dev/null
  lite_wait main "return [...document.querySelectorAll('button')].some((b) => b.textContent.trim() === '推送并跟踪' || b.textContent.trim() === '推送')" 8 >/dev/null
  lite_eval main "const b = [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === '推送并跟踪' || b.textContent.trim() === '推送'); b?.click(); return !!b" >/dev/null
}
answer() { # $1 = 要填的字（空 = 点取消）
  lite_wait main "return !!${DLG}" 15 >/dev/null || return 1
  if [ -n "$1" ]; then
    lite_eval main "__lite.fill($(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"), '.dialog.askpass input'); [...${DLG}.querySelectorAll('button')].find((b) => b.textContent.trim() === '确定').click(); return true" >/dev/null
  else
    lite_eval main "[...${DLG}.querySelectorAll('button')].find((b) => b.textContent.trim() === '取消').click(); return true" >/dev/null
  fi
  wait_for 3 "[ \"\$(lite_eval main 'return !!document.querySelector(\".dialog.askpass\")')\" = false ]"
}

echo "== 推送到要账号密码的远程：先问用户名、再问密码（不回显），答完推上去"
push_now
lite_wait main "return !!${DLG}" 15 >/dev/null
Q1=$(lite_eval main "return ${DLG}?.querySelector('.msg')?.textContent + '|' + ${DLG}?.querySelector('input')?.type")
ok "$(echo "${Q1}" | grep -q "^Username for 'http://127.0.0.1:${PORT}'.*|text$" && echo 1)" "问用户名：${Q1}（git 的原话，说清是哪个地址）"
answer "${GU}"
lite_wait main "return !!${DLG}" 15 >/dev/null
Q2=$(lite_eval main "return ${DLG}?.querySelector('.msg')?.textContent + '|' + ${DLG}?.querySelector('input')?.type")
ok "$(echo "${Q2}" | grep -q "^Password for 'http://${GU}@127.0.0.1:${PORT}'.*|password$" && echo 1)" "再问密码、输入框不回显：${Q2}"
answer "${GP}"
ok "$(wait_for 20 '[ "$(git --git-dir="${SRV}/shop.git" rev-parse -q --verify main 2>/dev/null)" = "$(git -C "${P}" rev-parse HEAD)" ]' && echo 1)" "推上去了：远程的 main 就是本地的 HEAD（$(git -C "${P}" rev-parse --short HEAD)）"
ok "$(grep -q ' none$' "${W}/req.log" && grep -q ' ok$' "${W}/req.log" && echo 1)" "服务端先回 401、带上凭据之后才放行：$(tr '\n' ',' < "${W}/req.log" | cut -c1-160)"
ok "$(! grep -rqF "${GP}" "${LITE_DATA}" "${LITE_LOG}" 2>/dev/null && echo 1)" "密码没落进应用数据目录和应用日志"

echo "== 再推一次、问用户名时点取消：报认证失败，不挂着；远程不动"
printf 'again\n' >> "${P}/a.txt"
git -C "${P}" commit -qam "第二次"
BEFORE=$(git --git-dir="${SRV}/shop.git" rev-parse main)
push_now
answer ""
ok "$(lite_wait main "return (document.querySelector('.confirm.bad')?.textContent ?? '').includes('认证失败')" 15 && echo 1)" "取消之后那张卡片报认证失败、说清下一步：$(lite_eval main "return (document.querySelector('.confirm.bad')?.textContent ?? '').replace(/\\s+/g, ' ').slice(0, 90)")"
ok "$([ "$(git --git-dir="${SRV}/shop.git" rev-parse main)" = "${BEFORE}" ] && [ -z "$(pgrep -f "git.*push.*127.0.0.1:${PORT}")" ] && echo 1)" "远程没动、git 进程没挂着"

lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS_N}" "${FAIL}"
[ "${FAIL}" = 0 ]
