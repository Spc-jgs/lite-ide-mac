#!/usr/bin/env python3
"""
冒充一个 ACP agent（docs/TAKEOVER.md MVP-6），给测试身份的 .app 用（scripts/lib/bridge.sh 经 LITE_AGENT_CMD 交给它）：
smoke 不能真去调模型 —— 花钱、要网、每次说的不一样。讲的剧本和浏览器里的桩（src/lib/dev/mock/agent.ts）一样，
但这一个走真的那条线：Rust 起进程、按行收发 JSON-RPC、反向请求、关窗口整组停。

一轮（session/prompt）里，提示里写「改 <相对路径>」：
  读它（fs/read_text_file —— 编辑器里有没保存的那份就是那份）→ 说出第一行 →
  提示里有「越界」的话，试着写项目外面的文件、说出被拒的原话 →
  要权限（session/request_permission，等人点）→ 在第二行插一行写回去（fs/write_text_file）→ 新建 NOTES-agent.md → 说完
停这一轮（session/cancel）之后回 stopReason: cancelled。stdin 读到头就退（同真适配器）。
"""
import json
import os
import sys

SID = "fake-1"
cwd = ""
nid = 1000
cancelled = False


def send(o):
    sys.stdout.write(json.dumps(o, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def update(u):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": SID, "update": u}})


def say(text):
    update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})


def request(method, params):
    """问宿主、等它回。等的时候来的 session/cancel 记下来"""
    global nid, cancelled
    nid += 1
    me = nid
    send({"jsonrpc": "2.0", "id": me, "method": method, "params": params})
    while True:
        line = sys.stdin.readline()
        if not line:
            sys.exit(0)
        m = json.loads(line)
        if m.get("id") == me and "method" not in m:
            return m
        if m.get("method") == "session/cancel":
            cancelled = True


def turn(prompt):
    global cancelled
    cancelled = False
    text = "\n".join(b.get("text", "") for b in prompt if b.get("type") == "text")
    rel = ""
    for line in text.splitlines():
        if line.startswith("改 "):
            rel = line[2:].strip()
    if not rel:
        say("说「改 <相对路径>」我就改那个文件。")
        return "end_turn"
    path = os.path.join(cwd, rel)
    update({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": f"先读 {rel}"}})
    update({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": f"Read {rel}", "kind": "read", "status": "in_progress", "locations": [{"path": path}]})
    r = request("fs/read_text_file", {"sessionId": SID, "path": path})
    if "error" in r:
        update({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed"})
        say("读不到：" + r["error"].get("message", ""))
        return "end_turn"
    content = r["result"]["content"]
    update({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"})
    say("第一行是：" + content.split("\n")[0] + "\n")
    if "越界" in text:
        w = request("fs/write_text_file", {"sessionId": SID, "path": os.path.join(cwd, "..", "lite-agent-outside.txt"), "content": "x"})
        say("越界写" + ("被拒：" + w["error"].get("message", "") if "error" in w else "写成了") + "\n")
    update({"sessionUpdate": "tool_call", "toolCallId": "t2", "title": f"Edit {rel}", "kind": "edit", "status": "pending", "locations": [{"path": path}]})
    p = request("session/request_permission", {
        "sessionId": SID,
        "toolCall": {"toolCallId": "t2", "title": f"Edit {rel}"},
        "options": [
            {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
            {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
        ],
    })
    outcome = p.get("result", {}).get("outcome", {})
    if cancelled or outcome.get("outcome") == "cancelled":
        return "cancelled"
    if outcome.get("optionId") != "allow":
        update({"sessionUpdate": "tool_call_update", "toolCallId": "t2", "status": "failed"})
        say("好，不改了。")
        return "end_turn"
    lines = content.split("\n")
    lines.insert(1, "// agent 加的一行")
    request("fs/write_text_file", {"sessionId": SID, "path": path, "content": "\n".join(lines)})
    update({"sessionUpdate": "tool_call_update", "toolCallId": "t2", "status": "completed"})
    request("fs/write_text_file", {"sessionId": SID, "path": os.path.join(cwd, "NOTES-agent.md"), "content": f"# 说明\n\n{rel} 加了一行。\n"})
    say("改好了。")
    return "end_turn"


def main():
    global cwd
    for line in sys.stdin:
        try:
            m = json.loads(line)
        except ValueError:
            continue
        method, mid, params = m.get("method"), m.get("id"), m.get("params") or {}
        if method == "initialize":
            send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 1, "agentCapabilities": {}, "authMethods": [],
                  "agentInfo": {"name": "fake-acp", "title": "假 Agent", "version": "0.0.1"}}})
        elif method == "session/new":
            cwd = params.get("cwd", "")
            send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": SID, "modes": {"currentModeId": "default", "availableModes": [
                {"id": "default", "name": "Default"}, {"id": "acceptEdits", "name": "Accept Edits"}]}}})
        elif method == "session/set_mode":
            send({"jsonrpc": "2.0", "id": mid, "result": {}})
            update({"sessionUpdate": "current_mode_update", "currentModeId": params.get("modeId")})
        elif method == "session/prompt":
            send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": turn(params.get("prompt") or [])}})
        elif method == "session/cancel":
            pass
        elif mid is not None:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": f"fake-acp 不认识 {method}"}})


if __name__ == "__main__":
    main()
