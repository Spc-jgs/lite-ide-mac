#!/usr/bin/env python3
"""外部、按需的资源验收；不驻留，不给正式应用加采样器。方法与边界见 docs/RESOURCE-GUARD.md。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import signal
import sys
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/lib"))
from bridge import call

PROFILES = {
    "quick": {"rounds": 4, "warmups": 1, "idle": 15, "settle": 5, "log_mib": 16, "tab_limit": 25, "fixture": "valid-syntax-v1"},
    "full": {"rounds": 8, "warmups": 2, "idle": 60, "settle": 15, "log_mib": 64, "tab_limit": 25, "fixture": "valid-syntax-v1"},
}
WORKLOADS = ("mixed", "json", "json-text", "logs")
PACES = {"burst": 0, "paced": 1.8}
MIB = 1024 * 1024
JS_CALLS = 0
EXE = ROOT / "src-tauri/target/test-app.noindex/lite-ide-mwtest.app/Contents/MacOS/lite-ide"
PROBE = ROOT / "src-tauri/target/resource-tools.noindex/resource-probe"
BUILD_STAMP = PROBE.parent / "test-build.json"
LIFETIME_SCRIPT = ROOT / "scripts/lib/resource-lifetimes.js"
SOURCE_ROOTS = ["src", "public", "src-tauri/src", "src-tauri/crates", "src-tauri/icons", "src-tauri/capabilities"]
SOURCE_FILES = ["index.html", "vite.config.ts", "svelte.config.js", "package.json", "pnpm-lock.yaml", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs", "src-tauri/tauri.conf.json", "src-tauri/tauri.java.json", "src-tauri/Info.plist"]


def run(args, timeout=10):
    r = subprocess.run([str(x) for x in args], capture_output=True, text=True, timeout=timeout)
    if r.returncode:
        raise RuntimeError(f"{Path(str(args[0])).name} 失败：{r.stderr[:500]}")
    if len(r.stdout) > 2 * MIB:
        raise RuntimeError("探针输出超过 2 MiB，停止采样")
    return r.stdout


def probe(*args):
    return json.loads(run([PROBE, *map(str, args)]))


def coalition_pair(row):
    values = (row.get("resource_coalition"), row.get("jetsam_coalition"))
    return values if all(isinstance(x, int) and x > 0 for x in values) else None


def fresh_coalition(catalog, main, before):
    pair = coalition_pair(next((r for r in catalog if r["pid"] == main), {}))
    # LaunchServices 必须给本轮应用一个新组；共享了旧组就不能认领它的全部 WebKit。
    return pair if pair and pair not in {tuple(x) for x in before} else None


def source_files():
    paths = {ROOT / p for p in SOURCE_FILES if (ROOT / p).is_file()}
    for name in SOURCE_ROOTS:
        paths.update(p for p in (ROOT / name).rglob("*") if p.is_file() and not p.is_symlink())
    return sorted(paths)


def fingerprint():
    h = hashlib.sha256()
    for p in source_files():
        h.update(str(p.relative_to(ROOT)).encode())
        h.update(b"\0")
        h.update(hashlib.sha256(p.read_bytes()).digest())
    return h.hexdigest()


def median(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def trend(values):
    """只看预热之后的同场景样本；正斜率是一条线索，不是泄漏证明。"""
    if len(values) < 4 or any(x is None for x in values):
        return {"slope": None, "r2": None, "growth": None}
    n = len(values)
    mx, my = (n - 1) / 2, statistics.mean(values)
    slope = sum((i - mx) * (v - my) for i, v in enumerate(values)) / sum((i - mx) ** 2 for i in range(n))
    ss = sum((v - my) ** 2 for v in values)
    residual = sum((v - my - slope * (i - mx)) ** 2 for i, v in enumerate(values))
    return {"slope": slope, "r2": 1 - residual / ss if ss else 0, "growth": values[-1] - values[0]}


def total(rows, key):
    if not rows or any(r.get(key) is None or "error" in r for r in rows):
        return None
    return sum(r[key] for r in rows)


def delta(before, after, key):
    a = {(r["pid"], r.get("start")): r for r in before}
    b = {(r["pid"], r.get("start")): r for r in after}
    if not a or a.keys() != b.keys():
        return None  # 进程退出、重启、PID 重用：缺失不是 0，也不混用累积计数器。
    out = []
    for k in a:
        x, y = a[k].get(key), b[k].get(key)
        if x is None or y is None or "error" in a[k] or "error" in b[k] or y < x:
            return None
        out.append(y - x)
    return sum(out)


def storage():
    base = Path.home() / "Library/Application Support/com.liteide.mwtest"
    roots = {"data": base, "logs": Path.home() / "Library/Logs/com.liteide.mwtest", "webkit": Path.home() / "Library/WebKit/com.liteide.mwtest", "cache": Path.home() / "Library/Caches/com.liteide.mwtest"}
    result = {}
    for name, root in roots.items():
        sizes, blocks, files, errors = 0, 0, 0, 0
        if root.exists():
            for directory, _, names in os.walk(root, followlinks=False):
                for f in names:
                    try:
                        p = Path(directory) / f
                        if p.is_symlink():
                            continue
                        s = p.stat()
                        sizes += s.st_size
                        blocks += s.st_blocks * 512
                        files += 1
                    except OSError:
                        errors += 1
        result[name] = {"bytes": sizes, "allocated_bytes": blocks, "files": files, "errors": errors}
    return result


def system():
    out = {}
    for key in ["hw.model", "hw.memsize", "hw.ncpu", "kern.memorystatus_vm_pressure_level", "vm.swapusage"]:
        try:
            out[key] = run(["sysctl", "-n", key]).strip()
        except (RuntimeError, subprocess.TimeoutExpired):
            out[key] = None
    return out


def preflight():
    if platform.system() != "Darwin" or not EXE.is_file():
        raise RuntimeError("需要 macOS 和新打的测试 .app：scripts/build-test-app.sh")
    if any(r["app"] for r in probe("--catalog", EXE)):
        raise RuntimeError("测试身份已在运行；请等别的验收退出，不结束或接管它")
    stamp = EXE.stat().st_mtime_ns
    stale = [str(p.relative_to(ROOT)) for p in source_files() if p.stat().st_mtime_ns > stamp]
    if stale:
        raise RuntimeError(f"测试 .app 比源码旧：{stale[0]}；先重打测试包")
    if not BUILD_STAMP.exists():
        raise RuntimeError("缺少测量构建凭据；运行 scripts/resources.sh --build")
    stamp = json.loads(BUILD_STAMP.read_text())
    if stamp != {"source": fingerprint(), "binary": hashlib.sha256(EXE.read_bytes()).hexdigest()}:
        raise RuntimeError("源码 / 测试二进制与构建凭据不一致；运行 scripts/resources.sh --build")


def build():
    if any(r["app"] for r in probe("--catalog", EXE)):
        raise RuntimeError("测试身份正在使用中，不能替换它的构建")
    before = fingerprint()
    env = dict(os.environ, CARGO_NET_OFFLINE="true", LITE_BUILD_OFFLINE="1")
    log = PROBE.parent / "test-build.log"
    with log.open("w") as f:
        r = subprocess.run([str(ROOT / "scripts/build-test-app.sh")], cwd=ROOT, env=env, stdout=f, stderr=subprocess.STDOUT, timeout=1200)
    if r.returncode:
        raise RuntimeError(f"测试构建失败，见 {log}")
    if fingerprint() != before:
        raise RuntimeError("构建期间应用源码变化，本次不写构建凭据；等另一轮编辑完成再运行")
    BUILD_STAMP.write_text(json.dumps({"source": before, "binary": hashlib.sha256(EXE.read_bytes()).hexdigest()}))


def fixture_text(ext):
    # 每种文件用合法语法，避免把错误恢复的压力混进正常编辑预算。
    rows = range(400)
    if ext == "json":
        return "[\n" + ",\n".join(json.dumps({"row": i, "value": 123}) for i in rows) + "\n]\n"
    if ext in ["xml", "html"]:
        tag = "root" if ext == "xml" else "main"
        return f"<{tag}>\n" + "".join(f'<p id="row{i}">sample value 123</p>\n' for i in rows) + f"</{tag}>\n"
    line = {
        "txt": lambda i: f"sample row {i} value 123",
        "md": lambda i: f"- sample row {i}: **value 123**",
        "css": lambda i: f".sample-{i} {{ color: #336699; margin: 0; }}",
        "sql": lambda i: f"SELECT {i} AS sample_value;",
        "csv": lambda i: f"sample-{i},123",
        "ini": lambda i: f"sample_{i}=123",
        "toml": lambda i: f"sample_{i} = 123",
        "yaml": lambda i: f"sample_{i}: 123",
        "properties": lambda i: f"sample.{i}=123",
    }[ext]
    return ("[sample]\n" if ext == "ini" else "") + "\n".join(line(i) for i in rows) + "\n"


def fixture_documents(workload):
    if workload == "logs":
        return {}
    if workload == "mixed":
        return {f"sample.{ext}": fixture_text(ext) for ext in ["txt", "json", "md", "html", "css", "xml", "sql", "csv", "ini", "toml", "yaml", "properties"]}
    # 对照组逐字相同，仅扩展名不同；不把文件大小差异误归因于语法处理。
    ext = "txt" if workload == "json-text" else "json"
    return {f"sample-{i:02}.{ext}": fixture_text("json") for i in range(12)}


def scenario(profile, workload, pace, minimap="on", lifetimes=False, control="none", lifetime_sampling="final", log_mib=None, native_diagnostics=False):
    p = dict(PROFILES[profile])
    if workload == "logs":
        from resource_log import method_sha
        p.update(fixture="isolated-log-v1", focus="logs", warmups=0, native_diagnostics=native_diagnostics, log_method_sha256=method_sha())
        if log_mib is not None:
            p["log_mib"] = log_mib
        return p
    # 默认场景保持原样；新变量写进可比性键，不能与旧场景算优化差值。
    if workload != "mixed":
        p["fixture"] = f"valid-syntax-v1-{workload}"
    if pace != "burst":
        p["open_delay_s"] = PACES[pace]
    if minimap == "off":
        p["minimap"] = False
    if lifetimes:
        p["lifetime_probe"] = hashlib.sha256(LIFETIME_SCRIPT.read_bytes()).hexdigest()
        p["lifetime_sampling"] = lifetime_sampling
    if control != "none":
        p["control"] = control
    return p


def open_documents(paths, delay, lifetimes=False, control="none"):
    for path in paths:
        js("return true" if control == "ipc" else f"await __lite.tabflow.openPath({json.dumps(str(path))}, {{preview:false}}); return true")
        if delay:
            # probe 返回规范路径；macOS 的 /tmp 实际指向 /private/tmp。
            wait_js("return true" if control == "ipc" else f"return __lite.tabs.active?.path === {json.dumps(str(path.resolve()))} && !!document.querySelector('.cm-content')")
            if lifetimes:
                recorded = js("return window.__resourceLifetimes.record()")
                if control != "ipc" and not recorded:
                    raise RuntimeError("生命周期探针未找到当前编辑器，不把漏采记为已回收")
            time.sleep(delay)
    wait_js("return __lite.tabs.list.length === 0" if control == "ipc" else f"return __lite.tabs.list.length === {len(paths)} && !!document.querySelector('.cm-content')")


def prepare(args):
    preflight()
    work = Path(os.environ["LITE_WORK"])
    fixture = work / "fixture"
    fixture.mkdir()
    for name, text in fixture_documents(args.workload).items():
        # 不用 Java / Rust / Python / TS 扩展名，普通编辑场景不能顺带启动语言服务器或下载安装。
        (fixture / name).write_text(text)
    p = scenario(args.profile, args.workload, args.pace, args.minimap, args.inspect_lifetimes, args.control, args.lifetime_sampling, args.log_mib, args.native_diagnostics)
    line = b"2026-01-01 00:00:00 INFO trace=resource-check bounded fixture\n"
    chunk = line * (MIB // len(line))
    with (fixture / "large.log").open("xb") as f:
        for _ in range(p["log_mib"]):
            f.write(chunk)
    out = Path(args.output) if args.output else ROOT / "src-tauri/target/resource-reports.noindex" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-") + work.name)
    out.mkdir(parents=True, exist_ok=False)
    if args.workload == "logs":
        from resource_log import archive_method
        archive_method(out)
    if args.inspect_lifetimes:
        (out / "lifetime-probe.js").write_bytes(LIFETIME_SCRIPT.read_bytes())
    context = {"schema": 3, "profile": args.profile, "scenario": p, "head": run(["git", "rev-parse", "HEAD"]).strip(),
               "dirty": bool(run(["git", "status", "--porcelain"]).strip()), "source_sha256": fingerprint(),
               "binary_sha256": hashlib.sha256(EXE.read_bytes()).hexdigest(), "os": platform.mac_ver()[0], "arch": platform.machine(),
               "system_before": system(), "surface": "test-identity-background", "recorded_utc": datetime.now(timezone.utc).isoformat(),
               "fixture_bytes": sum(x.stat().st_size for x in fixture.iterdir()), "baseline": args.baseline}
    catalog = probe("--catalog", EXE)
    state = {"output": str(out), "fixture": str(fixture), "context": context,
             "before_pids": [r["pid"] for r in catalog],
             "before_coalitions": sorted({coalition_pair(r) for r in catalog if coalition_pair(r)})}
    (work / "resources.json").write_text(json.dumps(state))


def js(body):
    global JS_CALLS
    JS_CALLS += 1
    return call({"cmd": "eval", "window": "main", "js": body}, timeout=25)


def wait_js(body, seconds=12):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if js(body):
            return
        time.sleep(.25)
    raise RuntimeError("测试页面未达到场景所需状态")


def view_counts(lifetimes=True):
    # 默认只在恢复空闲末尾解引用，减少反复观察对回收时机的干扰。
    census = "window.__resourceLifetimes?{lifetimes:window.__resourceLifetimes.snapshot()}:{}" if lifetimes else "{}"
    return js("return {tabs:__lite.tabs.list.length,editors:document.querySelectorAll('.cm-content').length,minimaps:document.querySelectorAll('.cm-minimap').length,nodes:document.querySelectorAll('*').length,terms:__lite.terms.list.length,runs:__lite.runs.list.length,...(" + census + ")}")


class Recorder:
    def __init__(self, state):
        self.state = state
        self.out = Path(state["output"])
        self.main = None
        self.owned = set()
        self.identities = {}
        self.children = set()
        self.ambiguous = set()
        self.checked = set(state["before_pids"])
        self.coalition_confirmed = False
        self.checkpoints = []
        self.n = 0
        self.observer_cpu_start = time.process_time()

    def discover(self):
        catalog = probe("--catalog", EXE)
        apps = [r["pid"] for r in catalog if r["app"]]
        if self.main is None:
            if len(apps) != 1:
                raise RuntimeError("不能唯一确认刚启动的测试进程")
            self.main = apps[0]
            self.owned.add(self.main)
            first = probe("--sample", self.main)[0]
            if "error" in first:
                raise RuntimeError("无法读取测试进程的身份和计数")
            self.state["main"] = {"pid": self.main, "start": first["start"]}
            (Path(os.environ["LITE_WORK"]) / "resources.json").write_text(json.dumps(self.state))
        changed = True
        while changed:
            changed = False
            for r in catalog:
                if r["ppid"] in self.owned and r["pid"] not in self.owned:
                    self.owned.add(r["pid"])
                    self.children.add(r["pid"])
                    changed = True
        pair = fresh_coalition(catalog, self.main, self.state.get("before_coalitions", []))
        self.coalition_confirmed = bool(pair)
        for r in catalog:
            pid = r["pid"]
            if not r["webkit"]:
                continue
            if pair and coalition_pair(r):
                self.ambiguous.discard(pid)
                if coalition_pair(r) == pair:
                    self.owned.add(pid)
                continue
            if pid in self.checked:
                continue
            self.checked.add(pid)
            try:
                text = run(["vmmap", "--summary", pid], timeout=8)
                responsible = re.search(r"^Responsible:\s+.*\[(\d+)\]", text, re.M)
                if responsible and int(responsible[1]) == self.main:
                    self.owned.add(pid)
                elif not responsible:
                    self.ambiguous.add(pid)
            except (RuntimeError, subprocess.TimeoutExpired):
                self.ambiguous.add(pid)
        alive = {r["pid"] for r in catalog}
        self.ambiguous.intersection_update(alive)
        rows = probe("--sample", *sorted(self.owned & alive)) if self.owned & alive else []
        for row in rows:
            if "error" in row:
                continue
            previous = self.identities.setdefault(row["pid"], row["start"])
            if previous != row["start"]:
                self.owned.discard(row["pid"])
                self.children.discard(row["pid"])
        return sorted(self.owned & alive), catalog

    def window(self, name, seconds):
        print(f"资源测量：{name}（{seconds}s）", flush=True)
        ids, _ = self.discover()
        begin, start = probe("--sample", *ids), time.monotonic()
        samples = []
        raw = self.out / "samples.jsonl"
        while True:
            now = time.monotonic()
            rows = probe("--sample", *ids)
            sample = {"phase": name, "elapsed_s": now - start, "processes": rows}
            with raw.open("a") as f:
                f.write(json.dumps(sample) + "\n")
            if raw.stat().st_size > 4 * MIB:
                raise RuntimeError("原始采样超过 4 MiB，停止；不继续写盘")
            samples.append(rows)
            if now - start >= seconds:
                break
            time.sleep(min(1, seconds - (now - start)))
        elapsed = time.monotonic() - start
        end_ids, after_catalog = self.discover()
        end = probe("--sample", *end_ids)
        cpu = delta(begin, end, "cpu_ns")
        known_webkit = [r for r in after_catalog if r["pid"] in end_ids and r["webkit"]]
        main_rows = [next((r for r in rows if r["pid"] == self.main), {}) for rows in samples]
        point = {"phase": name, "duration_s": elapsed,
                 "known_footprint_bytes": median([total(rows, "footprint") for rows in samples]),
                 "known_footprint_peak_bytes": max((total(rows, "footprint") for rows in samples if total(rows, "footprint") is not None), default=None),
                 "missing_samples": sum(total(rows, "footprint") is None for rows in samples),
                 "main_footprint_bytes": median([r.get("footprint") for r in main_rows]),
                 "main_fds": median([r.get("fds") for r in main_rows]),
                 "main_threads": median([r.get("threads") for r in main_rows]),
                 "cpu_percent_one_core": None if cpu is None else cpu / 1e9 / elapsed * 100,
                 "logical_write_bytes": delta(begin, end, "logical_write"),
                 "disk_write_bytes": delta(begin, end, "disk_write"),
                 "idle_wakeups": delta(begin, end, "idle_wakeups"),
                 "process_count": len(end_ids), "confirmed_webkit_count": len(known_webkit),
                 "process_names": {str(r["pid"]): r["name"] for r in after_catalog if r["pid"] in end_ids},
                 "attribution": "partial" if not known_webkit or self.ambiguous else "fresh-coalition" if self.coalition_confirmed else "responsible-pid",
                 "unattributed_new_webkit_count": len(self.ambiguous),
                 "views": view_counts(self.state["context"].get("scenario", {}).get("lifetime_sampling") != "final" or name == "recovered_idle"), "storage": storage()}
        self.checkpoints.append(point)
        (self.out / "checkpoints.json").write_text(json.dumps(self.checkpoints, indent=2))
        return point


def analyse(context, points, orphans):
    findings = []
    pressure_key = "kern.memorystatus_vm_pressure_level"
    before_pressure = context.get("system_before", {}).get(pressure_key)
    after_pressure = context.get("system_after", {}).get(pressure_key)
    if before_pressure is not None and after_pressure is not None and before_pressure != after_pressure:
        findings.append({"status": "UNKNOWN", "metric": "memory_pressure", "reason": "测量前后系统内存压力变化，回收/压缩会影响 footprint；内存下降不能当优化收益", "before": before_pressure, "after": after_pressure})
    idle = [x for x in points if x["phase"].endswith("idle")]
    closed = [x for x in points if x["phase"].startswith("closed-")]
    growth = trend([p["known_footprint_bytes"] for p in closed])
    if growth["slope"] is not None and growth["slope"] > MIB and growth["growth"] > 8 * MIB and growth["r2"] > .7:
        findings.append({"status": "WARN", "metric": "retained_memory", "reason": "关完序列持续增长；无预热的场景需另排首次初始化，长测 / Allocations 确认，不直接判内存泄漏"})
    opened = [p for p in points if p["phase"].startswith("opened-") and p["attribution"] != "partial"]
    peak = max((p.get("known_footprint_peak_bytes") or p["known_footprint_bytes"] or 0 for p in opened), default=0)
    if peak > 300 * MIB and context.get("scenario", {}).get("control") != "ipc":
        findings.append({"status": "WARN", "metric": "opened_memory_budget", "value": peak, "limit": 300 * MIB,
                         "reason": "约定场景打开 12 文件后的采样峰值超过 300 MiB 参考预算，需定位瞬时分配/缓存；不能用关闭后不增长代替峰值预算验收"})
    for p in idle:
        for key, limit, label in [("cpu_percent_one_core", 2, "idle_cpu"), ("logical_write_bytes", p["duration_s"] / 60 * MIB, "idle_logical_writes")]:
            value = p.get(key)
            if value is None:
                findings.append({"status": "UNKNOWN", "metric": label, "phase": p["phase"], "reason": "缺失计数或进程集合改变"})
            elif value > limit:
                findings.append({"status": "WARN", "metric": label, "phase": p["phase"], "value": value, "limit": limit})
        if p["phase"] == "baseline_idle" and p["attribution"] != "partial" and p.get("known_footprint_bytes") is not None and p["known_footprint_bytes"] > 200 * MIB:
            findings.append({"status": "WARN", "metric": "shell_memory", "reason": "无 JVM 的测试壳空闲 footprint 超过 200 MiB，需复查场景与预算"})
    if any(p["attribution"] == "partial" for p in points):
        findings.append({"status": "UNKNOWN", "metric": "whole_app_memory_cpu_io", "reason": "WebKit 归属不完整，已知进程合计只是下界，不能判整应用预算通过"})
    if any(p["known_footprint_bytes"] is None or p.get("missing_samples", 0) for p in points):
        findings.append({"status": "UNKNOWN", "metric": "memory", "reason": "有进程无法读取"})
    fd = trend([p["main_fds"] for p in closed])
    if fd["growth"] is not None and fd["growth"] > 8 and fd["slope"] > 1 and fd["r2"] > .7:
        findings.append({"status": "WARN", "metric": "fds", "reason": "同场景关闭后的 FD 持续增长"})
    threads = trend([p.get("main_threads") for p in closed])
    if threads["growth"] is not None and threads["growth"] > 3 and threads["slope"] > .5 and threads["r2"] > .7:
        findings.append({"status": "WARN", "metric": "threads", "reason": "同场景关闭后的主进程线程持续增长"})
    if any(p["views"]["tabs"] or p["views"]["editors"] for p in closed):
        findings.append({"status": "FAIL", "metric": "closed_views", "reason": "关闭后标签或编辑器仍在"})
    if orphans:
        findings.append({"status": "FAIL", "metric": "child_processes", "reason": "退出后仍有本轮确认的子进程存活", "count": len(orphans)})
    all_peaks = [p.get("known_footprint_peak_bytes") if p.get("known_footprint_peak_bytes") is not None else p.get("known_footprint_bytes") for p in points]
    known_workflow_peak = max((v for v in all_peaks if v is not None), default=None)
    return {"schema": 3, "context": context, "checkpoints": points, "closed_memory_trend": growth,
            "known_workflow_peak_bytes": known_workflow_peak,
            "findings": findings, "verdict": "FAIL" if any(f["status"] == "FAIL" for f in findings) else "REVIEW" if findings else "NO_SIGNAL",
            "leak_conclusion": "有限场景未发现增长线索不等于证明无泄漏", "orphans": orphans}


def compare(a, b):
    keys = ["profile", "scenario", "os", "arch", "surface"]
    if a.get("schema") != b.get("schema") or a.get("verdict") == "INVALID" or b.get("verdict") == "INVALID" or any(a["context"].get(k) != b["context"].get(k) for k in keys) or any(a["context"]["system_before"].get(k) != b["context"]["system_before"].get(k) for k in ["hw.model", "hw.memsize", "hw.ncpu"]):
        return {"status": "UNKNOWN", "reason": "场景 / 系统 / 机器条件不同，拒绝混比"}
    pressure_key = "kern.memorystatus_vm_pressure_level"
    pressures = [r["context"].get(k, {}).get(pressure_key) for r in [a, b] for k in ["system_before", "system_after"]]
    if len({x for x in pressures if x is not None}) > 1:
        return {"status": "UNKNOWN", "reason": "系统内存压力变化，拒绝把回收/压缩影响当性能差异"}
    rows = []
    old = {p["phase"]: p for p in a["checkpoints"]}
    if not old or set(old) != {p["phase"] for p in b["checkpoints"]}:
        return {"status": "UNKNOWN", "reason": "测量阶段缺失或不同，拒绝混比"}
    for p in b["checkpoints"]:
        q = old.get(p["phase"])
        if not q:
            continue
        for metric, floor in [("known_footprint_bytes", 16 * MIB), ("cpu_percent_one_core", 1), ("logical_write_bytes", MIB)]:
            x, y = q.get(metric), p.get(metric)
            if x is None or y is None or q["attribution"] == "partial" or p["attribution"] == "partial":
                rows.append({"phase": p["phase"], "metric": metric, "status": "UNKNOWN"})
            elif y - x > max(floor, abs(x) * .2):
                rows.append({"phase": p["phase"], "metric": metric, "status": "WARN", "before": x, "after": y})
    return {"status": "REVIEW" if rows else "NO_SIGNAL", "changes": rows, "policy": "20% + 绝对噪声底，仅提示复测，不自动放宽预算"}


def measure():
    state = json.loads((Path(os.environ["LITE_WORK"]) / "resources.json").read_text())
    if state["context"]["scenario"].get("focus") == "logs":
        from resource_log import measure as measure_logs
        return measure_logs(state)
    r = Recorder(state)
    p = state["context"]["scenario"]
    fixture = Path(state["fixture"])
    paths = sorted(x for x in fixture.iterdir() if x.name != "large.log")
    before = {x.name: hashlib.sha256(x.read_bytes()).hexdigest() for x in fixture.iterdir()}

    def close():
        call({"cmd": "menu", "id": "close-all-tabs", "window": "main"})
        wait_js("return __lite.tabs.list.length === 0 && document.querySelectorAll('.cm-content').length === 0")

    def cycle():
        open_documents(paths, p.get("open_delay_s", 0), "lifetime_probe" in p, p.get("control", "none"))
        if p.get("minimap") is False:
            wait_js("return !document.querySelector('.cm-minimap')")

    if "lifetime_probe" in p:
        js(LIFETIME_SCRIPT.read_text())
    if p.get("minimap") is False:
        wait_js("return __lite.settings.v.minimap === true")
        call({"cmd": "menu", "id": "toggle-minimap", "window": "main"})
        wait_js("return __lite.settings.v.minimap === false")
    close()
    time.sleep(5)
    r.window("baseline_idle", p["idle"])
    for _ in range(p["warmups"]):
        cycle()
        close()
        time.sleep(p["settle"])
    for n in range(p["rounds"]):
        cycle()
        r.window(f"opened-{n + 1}", 2)
        close()
        time.sleep(p["settle"])
        r.window(f"closed-{n + 1}", 2)
    js(f"await __lite.tabflow.openPath({json.dumps(str(fixture / 'large.log'))}, {{preview:false}}); return true")
    wait_js("return !!document.querySelector('.pane .bar .search input')")
    r.window("log_open", 3)
    js("const el=document.querySelector('.pane .bar .search input'); el.value='resource-check'; el.dispatchEvent(new Event('input',{bubbles:true})); return true")
    r.window("log_filtered", 3)
    close()
    time.sleep(p["settle"])
    r.window("recovered_idle", p["idle"])
    if "lifetime_probe" in p:
        js("return window.__resourceLifetimes.stop()")
    identities = probe("--sample", *sorted(r.children | {r.main}))
    call({"cmd": "menu", "id": "quit", "window": "main"})
    time.sleep(5)
    still = probe("--sample", *[x["pid"] for x in identities]) if identities else []
    orphans = [x["pid"] for x in still if "error" not in x and any(x["pid"] == y["pid"] and x["start"] == y.get("start") for y in identities)]
    context = state["context"]
    context["system_after"] = system()
    context["observer_python_cpu_s"] = time.process_time() - r.observer_cpu_start
    context["observer_eval_calls"] = JS_CALLS
    log = Path.home() / "Library/Logs/com.liteide.mwtest/app.log"
    budget = re.findall(r"\[budget\].*?boot=(\d+)ms", log.read_text(errors="replace")[-2 * MIB:]) if log.exists() else []
    context["boot_observed_ms"] = int(budget[-1]) if budget else None
    context["boot_classification"] = "本次构建后的启动观察值，未声称是热启动或 p95"
    context["source_unchanged"] = fingerprint() == context["source_sha256"]
    context["fixture_unchanged"] = before == {x.name: hashlib.sha256(x.read_bytes()).hexdigest() for x in fixture.iterdir()}
    context["diagnostic_unchanged"] = "lifetime_probe" not in p or hashlib.sha256(LIFETIME_SCRIPT.read_bytes()).hexdigest() == p["lifetime_probe"]
    report = analyse(context, r.checkpoints, orphans)
    if not context["source_unchanged"] or not context["fixture_unchanged"] or not context["diagnostic_unchanged"]:
        report["verdict"] = "INVALID"
        report["findings"].append({"status": "FAIL", "metric": "context", "reason": "测量期间源码、只读 fixture 或生命周期探针改变；本轮不作基线"})
    if context["baseline"]:
        report["comparison"] = compare(json.loads(Path(context["baseline"]).read_text()), report)
        if report["comparison"]["status"] != "NO_SIGNAL" and report["verdict"] == "NO_SIGNAL":
            report["verdict"] = "REVIEW"
    out = Path(state["output"])
    (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False))
    lines = [f"# 资源测量：{report['verdict']}", "", f"场景：{context['profile']} / {p['fixture']}；每文件打开后等待：{p.get('open_delay_s', 0)}s；缩略图：{p.get('minimap', True)}；HEAD：{context['head']}；dirty：{context['dirty']}",
             "", "| 场景 | 已知 footprint 中位 MiB | 采样峰值 MiB | 单核 CPU % | logical writes KiB | 主进程 FD | WebKit 归属 |", "|---|---:|---:|---:|---:|---:|---|"]
    def fmt(v, scale=1):
        return "UNKNOWN" if v is None else f"{v / scale:.2f}"
    for x in r.checkpoints:
        lines.append(f"| {x['phase']} | {fmt(x['known_footprint_bytes'], MIB)} | {fmt(x.get('known_footprint_peak_bytes'), MIB)} | {fmt(x['cpu_percent_one_core'])} | {fmt(x['logical_write_bytes'], 1024)} | {fmt(x['main_fds'])} | {x['attribution']} |")
    lines += ["", f"已知进程全流程采样峰值：{fmt(report['known_workflow_peak_bytes'], MIB)} MiB（含日志阶段；归属不完整时只是下界）。",
              "NO_SIGNAL 仅表示本方法已有调查线未触发；日志等专项的完整覆盖与结论仍按 RESOURCE-CHECKLIST 逐项记录。"]
    if "lifetime_probe" in p:
        lines += ["", f"## 弱引用观测（{p.get('lifetime_sampling', 'checkpoints')}；alive 不等于泄漏；未主动 GC）", "", "| 阶段 | view 活着/观察过 | doc 活着/观察过 | canvas 活着/观察过 | 非零画布 |", "|---|---:|---:|---:|---:|"]
        for x in r.checkpoints:
            census = x["views"].get("lifetimes")
            if census:
                counts = [f"{census[k]['alive']}/{census[k]['observed']}" for k in ["views", "docs", "canvases"]]
                lines.append(f"| {x['phase']} | {' | '.join(counts)} | {census['canvases']['nonzero']} |")
    if p.get("control") == "ipc":
        lines += ["", "本轮为 IPC 对照：opened-* 不打开编辑器；之后日志仍真实打开/过滤，用于观察无编辑器历史的日志场景。"]
    lines += ["", "## 线索 / 缺失", "", *[f"- {json.dumps(f,ensure_ascii=False)}" for f in report["findings"]], "", report["leak_conclusion"]]
    (out / "report.md").write_text("\n".join(lines) + "\n")
    print(f"报告：{out / 'report.md'}", flush=True)
    if report["verdict"] in ["FAIL", "INVALID"]:
        raise RuntimeError("验收失败或测量条件变化，见 report.json；不把缺失当通过")


def cleanup():
    """只结束本轮记录且身份未变的测试主进程，绝不按进程名批量结束正式应用。"""
    work = Path(os.environ["LITE_WORK"])
    state = json.loads((work / "resources.json").read_text())
    owner = state.get("main")
    if owner:
        def alive():
            row = probe("--sample", owner["pid"])[0]
            return "error" not in row and row.get("start") == owner["start"]
        if alive():
            try:
                call({"cmd": "menu", "id": "quit", "window": "main"}, timeout=3)
            except (OSError, RuntimeError):
                pass
            for sig in [signal.SIGTERM, signal.SIGKILL]:
                for _ in range(20):
                    if not alive(): break
                    time.sleep(.1)
                if alive():
                    os.kill(owner["pid"], sig)
    if any(r["app"] for r in probe("--catalog", EXE)):
        raise RuntimeError("测试身份仍有进程，保留临时目录，不清别人正在用的数据")
    lsregister = Path("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
    subprocess.run([str(lsregister), "-u", str(EXE.parents[2])], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("mode", choices=["prepare", "measure", "compare", "build", "cleanup"])
    ap.add_argument("--profile", choices=PROFILES, default="quick")
    ap.add_argument("--workload", choices=WORKLOADS, default="mixed")
    ap.add_argument("--pace", choices=PACES, default="burst")
    ap.add_argument("--minimap", choices=["on", "off"], default="on")
    ap.add_argument("--inspect-lifetimes", action="store_true")
    ap.add_argument("--lifetime-sampling", choices=["final", "checkpoints"], default="final")
    ap.add_argument("--control", choices=["none", "ipc"], default="none")
    ap.add_argument("--log-mib", type=int, choices=[16, 64, 1024])
    ap.add_argument("--native-diagnostics", action="store_true")
    ap.add_argument("--output")
    ap.add_argument("--baseline")
    ap.add_argument("reports", nargs="*")
    args = ap.parse_args()
    if args.mode == "prepare" and (args.inspect_lifetimes or args.control == "ipc") and args.pace != "paced":
        ap.error("生命周期诊断 / IPC 对照需要 --pace paced，以便观察每次挂载")
    if args.mode == "prepare" and ((args.log_mib is not None or args.native_diagnostics) and args.workload != "logs"):
        ap.error("--log-mib / --native-diagnostics 仅用于 --workload logs")
    if args.mode == "prepare" and args.workload == "logs" and (args.inspect_lifetimes or args.control != "none" or args.minimap != "on" or args.pace != "burst"):
        ap.error("独立日志场景不混入编辑器诊断/配置变量")
    try:
        if args.mode == "cleanup": cleanup()
        elif args.mode == "build": build()
        elif args.mode == "prepare": prepare(args)
        elif args.mode == "measure": measure()
        elif len(args.reports) == 2: print(json.dumps(compare(*[json.loads(Path(p).read_text()) for p in args.reports]), ensure_ascii=False, indent=2))
        else: ap.error("compare 需要两份 report.json")
    except (RuntimeError, OSError, subprocess.TimeoutExpired) as e:
        print(f"resources: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
