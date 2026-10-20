#!/usr/bin/env bash
# `.http` 文件发请求（docs/IDEA.md 第 19 节）的真 .app 验收，走测试通道（scripts/lib/bridge.sh）：不发全局按键、不读 AX、不截图、不抢焦点。
# 先 scripts/build-test-app.sh 打一份测试 .app。发请求的是系统的 /usr/bin/curl；对面是脚本自己写的一个回显服务（python3，只听 127.0.0.1），
# 把收到的方法、路径、头、体原样回成 JSON —— 验的就是「发出去的和文件里写的一样」。
set -uo pipefail
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
source "$(dirname "$0")/../lib/bridge.sh"
PASS=0; FAIL=0
ok() { if [ "$1" = 1 ]; then PASS=$((PASS + 1)); printf '  ✓ %s\n' "$2"; else FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$2"; fi; }
wait_for() { local s=$1 i; shift; for ((i = 0; i < s * 10; i++)); do eval "$@" >/dev/null 2>&1 && return 0; sleep 0.1; done; return 1; }

P=$(cd "$(mktemp -d /tmp/acH.XXXX)" && pwd -P)
SRV=$(cd "$(mktemp -d /tmp/acHs.XXXX)" && pwd -P)
SRV_PID=""
trap 'lite_teardown; [ -n "${SRV_PID}" ] && kill "${SRV_PID}" 2>/dev/null; rm -rf "${P}" "${SRV}"' EXIT

# ── 回显服务：/redirect 302 到 /echo，/slow 不回话（像停在断点上的服务），/big 回 3 MB，/status/N 回 N；别的回收到的东西 ──
cat > "${SRV}/echo.py" <<'EOF'
import json, sys, time
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
log = open(sys.argv[2], "a", buffering=1)
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def handle_one(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n).decode("utf-8", "replace") if n else None
        log.write(f"{self.command} {self.path}\n")
        if self.path.startswith("/slow"):
            time.sleep(60); return
        if self.path.startswith("/redirect"):
            self.send_response(302); self.send_header("Location", "/echo?from=redirect"); self.send_header("Content-Length", "0"); self.end_headers(); return
        if self.path.startswith("/big"):
            data = b"x" * (3 << 20)
            self.send_response(200); self.send_header("Content-Type", "text/plain"); self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data); return
        code = int(self.path.split("/status/")[1]) if "/status/" in self.path else 200
        out = json.dumps({"method": self.command, "path": self.path, "headers": dict(self.headers), "body": body}, ensure_ascii=False).encode()
        self.send_response(code); self.send_header("Content-Type", "application/json"); self.send_header("Content-Length", str(len(out))); self.end_headers(); self.wfile.write(out)
    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = handle_one
s = ThreadingHTTPServer(("127.0.0.1", 0), H)
open(sys.argv[1], "w").write(str(s.server_address[1]))
s.serve_forever()
EOF
python3 -I "${SRV}/echo.py" "${SRV}/port" "${SRV}/requests.log" & SRV_PID=$!
wait_for 10 "[ -s '${SRV}/port' ]" || { echo "回显服务起不来"; exit 1; }
PORT=$(cat "${SRV}/port")
hits() { wc -l < "${SRV}/requests.log" 2>/dev/null | tr -d ' '; }

mkdir -p "${P}/api"
cat > "${P}/api/orders.http" <<'EOF'
@base = http://{{host}}

### 查订单
GET {{base}}/echo?status=PAID
Accept: application/json

### 下单
POST {{base}}/orders
Content-Type: application/json
Authorization: Bearer {{token}}

{ "sku": "A-1", "qty": 2, "trace": "{{$uuid}}", "client": {{client.id}} }

### 从文件读请求体
PUT {{base}}/orders/1
Content-Type: application/json

< ./body.json

### 跟着重定向
GET {{base}}/redirect

### 不跟
# @no-redirect
GET {{base}}/redirect

### 茶壶
GET {{base}}/status/418

### 大的
GET {{base}}/big

### 慢的
GET {{base}}/slow

### 没定义
GET {{base}}/echo?x={{nope}}
EOF
printf '{ "sku": "B-2", "qty": 1 }\n' > "${P}/api/body.json"
printf '{ "$shared": { "client": { "id": 7 } }, "local": { "host": "127.0.0.1:%s", "token": "public-token" } }\n' "${PORT}" > "${P}/http-client.env.json"
printf '{ "local": { "token": "secret-private" } }\n' > "${P}/http-client.private.env.json"
BEFORE=$(cd "${P}" && find . -type f | sort | md5)
F="${P}/api/orders.http"

lite_clean_data
lite_launch "${P}" || exit 1
lite_eval main "await __lite.tabflow.openPath('${F}', { preview: false }); return true" >/dev/null
lite_wait main "return document.querySelectorAll('.cm-http-send').length === 9" 15 >/dev/null

# 发光标所在的那个：光标放到第 N 行，⌃⇧R（测试通道造的按键事件，发给编辑器）
send() { lite_eval main "__lite.caret($1, 1); __lite.key('Ctrl-Shift-r'); return true" >/dev/null; }
R='const r = (n) => __lite.runs.list.find((x) => x.name === n);'
done_() { lite_wait main "${R} return !!r('$1') && r('$1').status !== 'running'" "${2:-15}" >/dev/null; }
body() { lite_eval main "${R} return r('$1')?.log ?? ''"; }

echo "== 行号旁的 ▶、顶上的运行环境"
ok "$([ "$(lite_eval main "return document.querySelectorAll('.cm-http-send').length")" = 9 ] && echo 1)" "9 个请求各一个 ▶（「没定义」那个也有）"
ENVS=$(lite_eval main "return [...document.querySelectorAll('.cm-http-env .btn')].map((b) => b.textContent).join(',')")
ok "$([ "${ENVS}" = "无,local" ] && echo 1)" "环境条：${ENVS}（\$shared 不算一个环境）"

echo "== 没选环境：{{host}} 没定义，不发"
send 4
sleep 1.5
ok "$([ "$(hits)" = 0 ] && [ "$(lite_eval main "return __lite.runs.list.length")" = 0 ] && echo 1)" "服务端一个请求都没收到、运行窗没开格子（收到 $(hits) 个）"

echo "== 选 local，发 POST：头、体逐字对；私有环境盖公开的；\$shared 里的对象按路径取；{{\$uuid}} 换成真的"
lite_eval main "[...document.querySelectorAll('.cm-http-env .btn')].find((b) => b.textContent === 'local').click(); return true" >/dev/null
send 8
done_ 下单
B=$(body 下单)
ok "$([ "$(lite_eval main "${R} return r('下单')?.status + ':' + r('下单')?.code")" = "done:200" ] && echo 1)" "回来了：$(lite_eval main "${R} return r('下单')?.status + ':' + r('下单')?.code")"
ok "$(python3 -c '
import json,sys,re
d=json.load(open(sys.argv[1]))
b=json.loads(d["body"]); h={k.lower():v for k,v in d["headers"].items()}
ok = d["method"]=="POST" and d["path"]=="/orders" and h.get("authorization")=="Bearer secret-private" and h.get("content-type")=="application/json" \
  and b["sku"]=="A-1" and b["client"]==7 and re.fullmatch(r"[0-9a-f-]{36}", b["trace"])
print(1 if ok else 0)' "${B}" 2>/dev/null)" "服务端收到的：POST /orders、Bearer secret-private（私有盖公开）、client=7（\$shared 按路径取）、trace 是 UUID：$(head -c 300 "${B}" 2>/dev/null)"
ok "$(head -c 14 "${B}" | grep -q '^{$' && sed -n 2p "${B}" | grep -q '^  "method": "POST",$' && echo 1)" "响应体按原样的次序排好了（method 在第一个）：$(head -2 "${B}" | tr '\n' ' ')"
# 别在 $(…) 里写 case：bash 3.2 把模式后面那个 `)` 当成 $( 的结尾（第一次跑就红在这儿，路径其实是对的）
ok "$([[ "${B}" == "${LITE_DATA}/http/"* ]] && echo 1)" "响应体存在应用数据目录里：${B/#$HOME/~}"

echo "== < ./body.json、跟重定向、@no-redirect、4xx 标红"
send 15; done_ 从文件读请求体
ok "$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(1 if d["method"]=="PUT" and json.loads(d["body"])=={"sku":"B-2","qty":1} else 0)' "$(body 从文件读请求体)" 2>/dev/null)" "PUT 的体是 body.json 的内容"
send 21; done_ 跟着重定向
ok "$([ "$(lite_eval main "${R} const h = r('跟着重定向')?.http?.resp; return h?.status + ':' + h?.redirects")" = "200:1" ] && grep -q '"path": "/echo?from=redirect"' "$(body 跟着重定向)" && echo 1)" "跟了一次重定向，落在 /echo?from=redirect"
send 25; done_ 不跟
ok "$([ "$(lite_eval main "${R} return r('不跟')?.code")" = 302 ] && echo 1)" "# @no-redirect：停在 302"
send 28; done_ 茶壶
ok "$([ "$(lite_eval main "${R} return r('茶壶')?.status + ':' + r('茶壶')?.code")" = "failed:418" ] && echo 1)" "418 那一格是红点（failed）"

echo "== 3 MB 的响应：照收、照显示"
send 31; done_ 大的 30
ok "$([ "$(lite_eval main "${R} const h = r('大的')?.http?.resp; return h?.bytes + ':' + h?.truncated")" = "3145728:false" ] && [ "$(stat -f %z "$(body 大的)")" = 3145728 ] && echo 1)" "3145728 字节、没截断、盘上那份一样大"

echo "== 慢的：发出去之后点取消，当场停，curl 也没了"
N0=$(hits)
send 34
lite_wait main "${R} return r('慢的')?.status === 'running'" 5 >/dev/null
wait_for 5 "[ \$(hits) -gt ${N0} ]"
lite_eval main "[...document.querySelectorAll('.http .bar button')].find((b) => b.textContent.trim() === '取消')?.click(); return true" >/dev/null
T0=$(lite_now_ms)
done_ 慢的 5
ok "$([ "$(lite_eval main "${R} return r('慢的')?.status + ':' + r('慢的')?.http?.error")" = "stopped:已取消" ] && echo 1)" "取消之后 $(( $(lite_now_ms) - T0 )) ms 标成已停止"
ok "$(wait_for 3 "! pgrep -f 'curl.*${PORT}/slow'" && echo 1)" "curl 进程没了"

echo "== 没定义的变量：不发、说清是哪个"
N0=$(hits)
send 37
sleep 1.5
ok "$([ "$(hits)" = "${N0}" ] && [ -z "$(lite_eval main "${R} return r('没定义')?.status ?? ''")" ] && echo 1)" "{{nope}} 没定义：服务端没收到、没开格子"

echo "== 没有副作用"
ok "$([ "$(cd "${P}" && find . -type f | sort | md5)" = "${BEFORE}" ] && echo 1)" "项目里一个文件都没多（响应体在应用数据目录里，不像 IDEA 那样写进 .idea/httpRequests）"

lite_quit
printf '\n通过 %s 条，失败 %s 条\n' "${PASS}" "${FAIL}"
[ "${FAIL}" = 0 ]
