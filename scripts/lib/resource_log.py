"""独立日志的有限资源场景；沿用 resource_guard 的进程归属与清理，不加到正式应用。"""
import hashlib
import json
from pathlib import Path
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor

import resource_guard as g

METHOD_FILES = [Path(__file__), Path(g.__file__), g.ROOT / "scripts/resources.sh", g.ROOT / "scripts/lib/resource-probe.c"]


def method_sha():
    h = hashlib.sha256()
    for path in METHOD_FILES:
        h.update(path.name.encode() + b"\0" + path.read_bytes())
    return h.hexdigest()


def archive_method(out):
    for path in METHOD_FILES:
        (out / path.name).write_bytes(path.read_bytes())


def file_sha(path):
    # 1 GiB fixture 也只用 1 MiB 缓冲；观察者不另读一份全文撑高系统压力。
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(g.MIB), b""):
            h.update(block)
    return h.hexdigest()


def expected_lines(path):
    # 与 prepare 的整行重复 fixture 对齐，防止“过滤完成”其实只是还没开始的旧状态。
    line = b"2026-01-01 00:00:00 INFO trace=resource-check bounded fixture\n"
    size = path.stat().st_size
    if not size or size % len(line):
        raise RuntimeError("日志 fixture 不是完整行")
    return size // len(line)


def native_snapshot(recorder, phase, metadata):
    ids, catalog = recorder.discover()
    identities = {x["pid"]: x for x in g.probe("--sample", *ids)}
    for x in catalog:
        if x["pid"] not in ids or x["name"] == "com.apple.WebKit.Networking":
            continue
        before = identities[x["pid"]]
        if "error" in before:
            raise RuntimeError("原生诊断进程身份缺失")
        try:
            text = g.run(["vmmap", "--summary", x["pid"]], timeout=12)
            after = g.probe("--sample", x["pid"])[0]
            if "error" in after or before["start"] != after["start"]:
                raise RuntimeError("原生诊断期间进程身份改变")
            (recorder.out / f"{phase}-{x['name']}.vmmap.txt").write_text(text)
        except (RuntimeError, subprocess.TimeoutExpired) as e:
            metadata.append({"phase": phase, "native_error": str(e)})


def log_js(body):
    return "const h=__lite.tabs.list[0]?.handle; if(h===undefined) return false; " + body


def action(recorder, phase, body, ready, seconds):
    # 操作与采样重叠，避免只在 openPath 返回以后才开始计峰值；1 秒采样仍可能漏瞬时峰。
    with ThreadPoolExecutor(max_workers=1) as pool:
        future = pool.submit(g.js, body)
        recorder.window(phase, seconds)
        future.result()
    g.wait_js(ready, seconds=30)
    return g.js(log_js('return {stat:await __TAURI_INTERNALS__.invoke("log_stat",{handle:h}),filter:await __TAURI_INTERNALS__.invoke("log_filter_stat",{handle:h}),rows:document.querySelectorAll(".lv .row").length}'))


def measure(state):
    recorder = g.Recorder(state)
    p, context = state["context"]["scenario"], state["context"]
    fixture = Path(state["fixture"]) / "large.log"
    digest, expected = file_sha(fixture), expected_lines(fixture)
    metadata = []

    def native(phase):
        if p["native_diagnostics"]:
            native_snapshot(recorder, phase, metadata)

    g.call({"cmd": "menu", "id": "close-all-tabs", "window": "main"})
    time.sleep(5)
    recorder.window("baseline_idle", p["idle"])
    native("baseline")
    for i in range(1, p["rounds"] + 1):
        result = action(recorder, f"log-open-{i}", f"await __lite.tabflow.openPath({json.dumps(str(fixture))},{{preview:false}}); return true",
                        log_js(f'const s=await __TAURI_INTERNALS__.invoke("log_stat",{{handle:h}}); return s.complete && s.levelsComplete && s.lineCount==={expected} && !!document.querySelector(".pane .bar .search input")'), 6)
        metadata.append({"phase": f"log-open-{i}", **result})
        recorder.window(f"log-ready-{i}", 5)
        if i == 1:
            native("log-ready")
        result = action(recorder, f"log-filter-{i}", "const el=document.querySelector('.pane .bar .search input'); el.value='resource-check'; el.dispatchEvent(new Event('input',{bubbles:true})); return true",
                        log_js(f'const s=await __TAURI_INTERNALS__.invoke("log_filter_stat",{{handle:h}}); return !!s && s.complete && s.hits==={expected}'), 8)
        metadata.append({"phase": f"log-filter-{i}", **result})
        recorder.window(f"log-filter-ready-{i}", 5)
        if i == 1:
            native("filtered")
        g.call({"cmd": "menu", "id": "close-all-tabs", "window": "main"})
        g.wait_js('return __lite.tabs.list.length===0 && !document.querySelector(".pane .bar .search input")')
        time.sleep(p["settle"])
        recorder.window(f"closed-{i}", 3)
    recorder.window("recovered_idle", p["idle"])
    native("recovered")
    identities = g.probe("--sample", *sorted(recorder.owned))
    g.call({"cmd": "menu", "id": "quit", "window": "main"})
    time.sleep(5)
    still = g.probe("--sample", *[x["pid"] for x in identities])
    orphans = [x["pid"] for x in still if "error" not in x and any(x["pid"] == y["pid"] and x["start"] == y.get("start") for y in identities)]
    context.update(system_after=g.system(), observer_eval_calls=g.JS_CALLS,
                   source_unchanged=g.fingerprint() == context["source_sha256"], fixture_unchanged=file_sha(fixture) == digest,
                   diagnostic_unchanged=method_sha() == p["log_method_sha256"], expected_lines=expected,
                   observer_python_cpu_s=time.process_time() - recorder.observer_cpu_start)
    report = g.analyse(context, recorder.checkpoints, orphans)
    report["log_checks"] = metadata
    if any("native_error" in x for x in metadata):
        report["findings"].append({"status": "UNKNOWN", "metric": "native_snapshot", "reason": "原生分类缺失，见 log_checks"})
        if report["verdict"] == "NO_SIGNAL":
            report["verdict"] = "REVIEW"
    if not all(context[k] for k in ("source_unchanged", "fixture_unchanged", "diagnostic_unchanged")):
        report["verdict"] = "INVALID"
    if context.get("baseline"):
        report["comparison"] = g.compare(json.loads(Path(context["baseline"]).read_text()), report)
        if report["comparison"]["status"] != "NO_SIGNAL" and report["verdict"] == "NO_SIGNAL":
            report["verdict"] = "REVIEW"
    (recorder.out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
    lines = [f"# 独立日志资源：{report['verdict']}", "", f"fixture {fixture.stat().st_size} B / {expected} 行；{p['rounds']} 轮；原生诊断 {p['native_diagnostics']}。", "",
             "后台测试身份、刚生成的缓存热文件；并发采样不等于真实前台首屏或滚动延迟。无预热，第一轮单列。",
             "vmmap 只在采样窗外读取，仍会扰动进程；1 秒采样峰值可能遗漏更短峰值。", "",
             "| 阶段 | footprint 中位 MiB | 采样峰 MiB | 单核 CPU % | 主进程 FD | 归属 |", "|---|---:|---:|---:|---:|---|"]
    def fmt(value, scale=1):
        return "UNKNOWN" if value is None else f"{value / scale:.2f}"
    for x in recorder.checkpoints:
        lines.append(f"| {x['phase']} | {fmt(x['known_footprint_bytes'], g.MIB)} | {fmt(x['known_footprint_peak_bytes'], g.MIB)} | {fmt(x['cpu_percent_one_core'])} | {fmt(x['main_fds'])} | {x['attribution']} |")
    lines += ["", "NO_SIGNAL 仅指已有调查线；不把日志套入 12 文件 300 MiB 门槛，不把有限循环判为无线性泄漏。原有 1 GiB 预算还需分别核对常驻、过滤命中规模、前台延迟和帧率。", "", *[f"- {json.dumps(x, ensure_ascii=False)}" for x in report["findings"]]]
    (recorder.out / "report.md").write_text("\n".join(lines) + "\n")
    print(f"报告：{recorder.out / 'report.md'}", flush=True)
    if report["verdict"] in ("FAIL", "INVALID"):
        raise RuntimeError("独立日志验收失败或方法改变，见 report.json")
