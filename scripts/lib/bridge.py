#!/usr/bin/env python3
"""测试通道的客户端（协议见 src-tauri/src/testbridge.rs 头上）。

用法（socket 路径从环境变量 LITE_TEST_SOCK 拿）：
  bridge.py windows                    开着的窗口（JSON 数组，前台的在前）
  bridge.py menu <id> [窗口]            触发菜单项（不带窗口 = 和点原生菜单一样发给前台）
  bridge.py close <窗口>                关一个窗口
  bridge.py eval <窗口> '<JS 函数体>'    在那个窗口里跑，打印 return 的值（JSON）

出错（连不上、页面里抛了、超时）打印到 stderr、退出码 1。用 Python 而不是 nc：JS 里的引号和换行要正确转义成 JSON，
在 bash 里手拼迟早拼错。
"""
import json
import os
import socket
import sys


def call(req: dict, timeout: float = 30) -> object:
    path = os.environ.get("LITE_TEST_SOCK")
    if not path:
        raise SystemExit("没设 LITE_TEST_SOCK")
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(path)
    s.sendall((json.dumps(req, ensure_ascii=False) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    resp = json.loads(buf.decode() or "{}")
    if not resp.get("ok"):
        raise RuntimeError(resp.get("error") or "没有回应")
    return resp.get("value")


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    cmd, rest = argv[0], argv[1:]
    try:
        if cmd == "windows":
            v = call({"cmd": "windows"})
        elif cmd == "menu":
            req = {"cmd": "menu", "id": rest[0]}
            if len(rest) > 1:
                req["window"] = rest[1]
            v = call(req)
        elif cmd == "close":
            v = call({"cmd": "close", "window": rest[0]})
        elif cmd == "eval":
            v = call({"cmd": "eval", "window": rest[0], "js": rest[1]})
        else:
            print(f"不认识的子命令：{cmd}", file=sys.stderr)
            return 2
    except (OSError, RuntimeError) as e:
        print(f"bridge {cmd}：{e}", file=sys.stderr)
        return 1
    # 字符串原样打（方便 bash 比较），别的打 JSON
    print(v if isinstance(v, str) else json.dumps(v, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
