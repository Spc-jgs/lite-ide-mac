#!/usr/bin/env python3
"""资源门禁本身必须认得缺失、PID 重用、预热与不可比报告，不能把没量到写成 0。"""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET
import configparser

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/lib"))
import resource_guard as g


def row(pid=1, start=7, cpu=100, error=None):
    r = {"pid": pid, "start": start, "cpu_ns": cpu, "footprint": 10, "fds": 3}
    if error is not None: r["error"] = error
    return r


def point(name="baseline_idle", memory=100 * g.MIB):
    return {"phase": name, "duration_s": 60, "known_footprint_bytes": memory, "attribution": "responsible-pid",
            "cpu_percent_one_core": .2, "logical_write_bytes": 0, "main_fds": 10, "views": {"tabs": 0, "editors": 0}}


def report():
    context = {"profile": "quick", "scenario": {"rounds": 4}, "os": "1", "arch": "arm64", "surface": "test-identity-background",
               "system_before": {"hw.model": "model", "hw.memsize": "16", "hw.ncpu": "8"}}
    return g.analyse(context, [point()], [])


class Analysis(unittest.TestCase):
    def test_isolated_log_scenario_does_not_open_editors(self):
        self.assertEqual(g.fixture_documents("logs"), {})
        a = g.scenario("quick", "logs", "burst", log_mib=64)
        b = g.scenario("quick", "logs", "burst", log_mib=1024)
        c = g.scenario("quick", "logs", "burst", log_mib=64, native_diagnostics=True)
        self.assertEqual(a["warmups"], 0)
        self.assertEqual(a["focus"], "logs")
        self.assertNotEqual(a, b)
        self.assertNotEqual(a, c)
        self.assertEqual(len(a["log_method_sha256"]), 64)

    def test_log_fixture_hash_is_streamed_and_count_checks_complete_lines(self):
        import resource_log as logs
        line = b"2026-01-01 00:00:00 INFO trace=resource-check bounded fixture\n"
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "large.log"
            p.write_bytes(line * 3)
            self.assertEqual(logs.expected_lines(p), 3)
            with mock.patch.object(Path, "read_bytes", side_effect=AssertionError("whole fixture read")):
                self.assertEqual(logs.file_sha(p), __import__("hashlib").sha256(line * 3).hexdigest())
            p.write_bytes(line + b"partial")
            with self.assertRaises(RuntimeError):
                logs.expected_lines(p)

    def test_native_diagnostics_rejects_reused_pid(self):
        import resource_log as logs
        r = mock.Mock()
        r.discover.return_value = ([7], [{"pid": 7, "name": "lite-ide"}])
        before, after = row(pid=7, start=1), row(pid=7, start=2)
        metadata = []
        with tempfile.TemporaryDirectory() as d:
            r.out = Path(d)
            with mock.patch.object(g, "probe", side_effect=[[before], [after]]), mock.patch.object(g, "run", return_value="snapshot"):
                logs.native_snapshot(r, "test", metadata)
            self.assertEqual(list(r.out.iterdir()), [])
        self.assertEqual(len(metadata), 1)
        self.assertIn("身份改变", metadata[0]["native_error"])

    def test_diagnostic_scenarios_have_separate_identity(self):
        a, b = report(), report()
        a["context"]["scenario"] = g.scenario("full", "mixed", "paced")
        b["context"]["scenario"] = g.scenario("full", "mixed", "paced", lifetimes=True)
        self.assertEqual(len(b["context"]["scenario"]["lifetime_probe"]), 64)
        self.assertEqual(b["context"]["scenario"]["lifetime_sampling"], "final")
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        a = copy.deepcopy(b)
        b["context"]["scenario"] = g.scenario("full", "mixed", "paced", lifetimes=True, lifetime_sampling="checkpoints")
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        b["context"]["scenario"] = g.scenario("full", "mixed", "paced", lifetimes=True, control="ipc")
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")

    def test_final_census_does_not_deref_in_intermediate_checkpoints(self):
        with mock.patch.object(g, "js") as js:
            g.view_counts(False)
            self.assertNotIn("snapshot()", js.call_args.args[0])
            g.view_counts(True)
            self.assertIn("snapshot()", js.call_args.args[0])

    def test_control_matches_probe_schedule_without_opening_documents(self):
        calls = []
        def js(body):
            calls.append("record" if "record()" in body else "open")
            if "record()" not in body:
                self.assertNotIn("openPath", body)
            return False
        with mock.patch.object(g, "js", side_effect=js), \
             mock.patch.object(g, "wait_js", side_effect=lambda s: calls.append("ready")), \
             mock.patch.object(g.time, "sleep", side_effect=lambda s: calls.append(s)):
            g.open_documents([Path("/fixture/a.txt")], 1.8, True, "ipc")
        self.assertEqual(calls, ["open", "ready", "record", 1.8, "ready"])
        with mock.patch.object(g, "js", return_value=False), mock.patch.object(g, "wait_js"):
            with self.assertRaisesRegex(RuntimeError, "未找到当前编辑器"):
                g.open_documents([Path("/fixture/a.txt")], 1.8, True)

    def test_isolation_fixtures_keep_bytes_and_file_count_equal(self):
        text, code = g.fixture_documents("json-text"), g.fixture_documents("json")
        self.assertEqual(len(text), 12)
        self.assertEqual(len(code), 12)
        self.assertTrue(all(Path(x).suffix == ".txt" for x in text))
        self.assertTrue(all(Path(x).suffix == ".json" for x in code))
        self.assertEqual({Path(k).stem: v.encode() for k, v in text.items()},
                         {Path(k).stem: v.encode() for k, v in code.items()})
        self.assertTrue(all(len(json.loads(v)) == 400 for v in code.values()))
        self.assertEqual(len(g.fixture_documents("mixed")), 12)

    def test_new_scenarios_cannot_be_compared_as_an_optimization(self):
        original = copy.deepcopy(g.PROFILES)
        base = g.scenario("full", "mixed", "burst")
        self.assertEqual(base, original["full"])
        for workload, pace in [("json", "burst"), ("json-text", "burst"), ("mixed", "paced")]:
            a, b = report(), report()
            a["context"]["scenario"] = base
            b["context"]["scenario"] = g.scenario("full", workload, pace)
            self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        self.assertNotEqual(g.scenario("full", "json", "burst"), g.scenario("full", "json-text", "burst"))
        self.assertEqual(g.scenario("full", "mixed", "paced")["open_delay_s"], 1.8)
        a, b = report(), report()
        a["context"]["scenario"] = g.scenario("full", "mixed", "paced")
        b["context"]["scenario"] = g.scenario("full", "mixed", "paced", "off")
        self.assertIs(b["context"]["scenario"].get("minimap"), False)
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        self.assertEqual(g.PROFILES, original)

    def test_paced_opens_wait_for_each_editor_before_delay(self):
        calls = []
        with mock.patch.object(g, "js", side_effect=lambda s: calls.append("open")), \
             mock.patch.object(g, "wait_js", side_effect=lambda s: calls.append("ready")), \
             mock.patch.object(g.time, "sleep", side_effect=lambda s: calls.append(s)):
            g.open_documents([Path("/fixture/a.txt"), Path("/fixture/b.txt")], 1.8)
            self.assertEqual(calls, ["open", "ready", 1.8, "open", "ready", 1.8, "ready"])
            calls.clear()
            g.open_documents([Path("/fixture/a.txt"), Path("/fixture/b.txt")], 0)
            self.assertEqual(calls, ["open", "open", "ready"])

    def test_paced_ready_uses_the_same_canonical_path_as_probe(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "real").mkdir()
            (root / "alias").symlink_to(root / "real", target_is_directory=True)
            path = root / "alias/sample.json"
            path.write_text("[]")
            with mock.patch.object(g, "js"), mock.patch.object(g, "wait_js") as ready, mock.patch.object(g.time, "sleep"):
                g.open_documents([path], 1.8)
            self.assertIn(json.dumps(str(path.resolve())), ready.call_args_list[0].args[0])
            self.assertNotIn(json.dumps(str(path)), ready.call_args_list[0].args[0])

    def test_fixtures_are_source_documents_not_repeated_invalid_text(self):
        self.assertEqual(len(json.loads(g.fixture_text("json"))), 400)
        self.assertEqual(len(ET.fromstring(g.fixture_text("xml"))), 400)
        ini = configparser.ConfigParser(); ini.read_string(g.fixture_text("ini"))
        self.assertEqual(len(ini["sample"]), 400)
        self.assertIn(".sample-399 { color:", g.fixture_text("css"))

    def test_coalition_requires_positive_ids_and_a_fresh_app_group(self):
        main = {"pid": 7, "resource_coalition": 80, "jetsam_coalition": 81}
        self.assertEqual(g.fresh_coalition([main], 7, [(20, 21)]), (80, 81))
        self.assertIsNone(g.fresh_coalition([main], 7, [(80, 81)]))
        self.assertIsNone(g.fresh_coalition([main], 8, []))
        self.assertIsNone(g.coalition_pair({"resource_coalition": 0, "jetsam_coalition": 81}))
        self.assertIsNone(g.coalition_pair({"resource_coalition": None, "jetsam_coalition": None}))

    def test_recorder_never_claims_another_apps_webkit(self):
        def process(pid, parent, app, webkit, pair):
            return {"pid": pid, "ppid": parent, "app": app, "webkit": webkit,
                    "resource_coalition": pair[0], "jetsam_coalition": pair[1]}
        catalog = [process(7, 1, True, False, (80, 81)), process(8, 1, False, True, (80, 81)),
                   process(9, 1, False, True, (20, 21)), process(10, 7, False, False, (80, 81))]
        def probe(*args):
            return catalog if args[0] == "--catalog" else [row(pid=i) for i in args[1:]]
        with tempfile.TemporaryDirectory() as d, mock.patch.dict(os.environ, {"LITE_WORK": d}), mock.patch.object(g, "probe", side_effect=probe), mock.patch.object(g, "run", side_effect=AssertionError("不应猜 vmmap")):
            recorder = g.Recorder({"output": d, "before_pids": [9], "before_coalitions": [(20, 21)]})
            ids, _ = recorder.discover()
            self.assertEqual(ids, [7, 8, 10])
            self.assertTrue(recorder.coalition_confirmed)
            self.assertFalse(recorder.ambiguous)

    def test_offline_java_build_refuses_network(self):
        source = (ROOT / "scripts/java-bundle.sh").read_text()
        function = "download() {\n" + source.split("download() {\n", 1)[1].split("\n}\n", 1)[0] + "\n}\n"
        # curl 用本地函数替身：检验离线缺缓存确实失败，整个测试不联网。
        command = function + '\ncurl() { echo NETWORK_CALLED; }; download fixture-url'
        offline = subprocess.run(["bash", "-c", command], env=dict(os.environ, LITE_BUILD_OFFLINE="1"), capture_output=True, text=True)
        self.assertNotEqual(offline.returncode, 0)
        self.assertNotIn("NETWORK_CALLED", offline.stdout)
        online = subprocess.run(["bash", "-c", command], env=dict(os.environ, LITE_BUILD_OFFLINE=""), capture_output=True, text=True)
        self.assertEqual(online.returncode, 0)
        self.assertIn("NETWORK_CALLED", online.stdout)

    def test_missing_and_denied_are_not_zero(self):
        self.assertIsNone(g.total([], "footprint"))
        self.assertIsNone(g.total([row(), row(pid=2, error=1)], "footprint"))
        self.assertIsNone(g.delta([row()], [row(error=1)], "cpu_ns"))
        p = point(); p["cpu_percent_one_core"] = None
        self.assertTrue(any(x["status"] == "UNKNOWN" for x in g.analyse({}, [p], [])["findings"]))

    def test_reused_pid_and_process_changes_invalidate_counters(self):
        self.assertIsNone(g.delta([row()], [row(start=8, cpu=1000)], "cpu_ns"))
        self.assertIsNone(g.delta([row()], [row(), row(pid=2)], "cpu_ns"))
        self.assertIsNone(g.delta([row(cpu=200)], [row(cpu=100)], "cpu_ns"))
        self.assertEqual(g.delta([row(cpu=100)], [row(cpu=350)], "cpu_ns"), 250)

    def test_warm_cache_plateau_is_not_a_leak(self):
        ps = [point(f"closed-{i}", 180 * g.MIB) for i in range(4)]
        r = g.analyse({}, ps, [])
        self.assertFalse(any(x["metric"] == "retained_memory" for x in r["findings"]))
        self.assertEqual(g.trend([1, 1, 1, 1])["slope"], 0)
        self.assertIsNone(g.trend([1, 2, None, 4])["slope"])
        self.assertIsNone(g.trend([1, 2, 3])["slope"])

    def test_stable_closed_memory_does_not_hide_opened_peak_budget(self):
        p = point("opened-1", 250 * g.MIB)
        p["known_footprint_peak_bytes"] = 350 * g.MIB
        ps = [p, *[point(f"closed-{i}", 180 * g.MIB) for i in range(4)]]
        r = g.analyse({}, ps, [])
        self.assertEqual(r["verdict"], "REVIEW")
        self.assertTrue(any(x["metric"] == "opened_memory_budget" for x in r["findings"]))
        p["attribution"] = "partial"
        self.assertFalse(any(x["metric"] == "opened_memory_budget" for x in g.analyse({}, ps, [])["findings"]))

    def test_workflow_peak_includes_log_not_only_editor_window(self):
        opened, log = point("opened-1", 180 * g.MIB), point("log_open", 220 * g.MIB)
        opened["known_footprint_peak_bytes"] = 250 * g.MIB
        log["known_footprint_peak_bytes"] = 450 * g.MIB
        r = g.analyse({}, [opened, log], [])
        self.assertEqual(r["known_workflow_peak_bytes"], 450 * g.MIB)
        self.assertFalse(any(f["metric"] == "opened_memory_budget" for f in r["findings"]))
        self.assertIsNone(g.analyse({}, [], [])["known_workflow_peak_bytes"])
        self.assertEqual(g.analyse({}, [point("log_open", 0)], [])["known_workflow_peak_bytes"], 0)

    def test_sustained_growth_is_a_signal_not_proof(self):
        ps = [point(f"closed-{i}", (100 + 4 * i) * g.MIB) for i in range(4)]
        r = g.analyse({}, ps, [])
        self.assertEqual(r["verdict"], "REVIEW")
        self.assertTrue(any(x["metric"] == "retained_memory" for x in r["findings"]))
        self.assertNotEqual(r["verdict"], "LEAK")

    def test_unattributed_webkit_and_still_open_views(self):
        p = point(); p["attribution"] = "partial"
        self.assertTrue(any(x["status"] == "UNKNOWN" for x in g.analyse({}, [p], [])["findings"]))
        p = point("closed-1"); p["views"]["editors"] = 1
        self.assertEqual(g.analyse({}, [p], [])["verdict"], "FAIL")
        self.assertEqual(g.analyse({}, [], [123])["verdict"], "FAIL")

    def test_comparison_rejects_different_context_or_missing_metrics(self):
        a, b = report(), report()
        self.assertEqual(g.compare(a, b)["status"], "NO_SIGNAL")
        b["context"]["os"] = "2"
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        b = report(); b["verdict"] = "INVALID"
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        b = report(); b["checkpoints"][0]["known_footprint_bytes"] = None
        self.assertEqual(g.compare(a, b)["changes"][0]["status"], "UNKNOWN")
        b = report(); b["checkpoints"] = []
        self.assertEqual(g.compare(a, b)["status"], "UNKNOWN")
        b = report(); b["checkpoints"][0]["attribution"] = "partial"
        self.assertTrue(all(x["status"] == "UNKNOWN" for x in g.compare(a, b)["changes"]))

    def test_comparison_has_absolute_noise_floor(self):
        a, b = report(), report()
        b["checkpoints"][0]["known_footprint_bytes"] += g.MIB
        self.assertEqual(g.compare(a, b)["status"], "NO_SIGNAL")
        b["checkpoints"][0]["known_footprint_bytes"] += 30 * g.MIB
        self.assertEqual(g.compare(a, b)["status"], "REVIEW")

    def test_memory_pressure_change_is_not_an_improvement(self):
        a = report()
        a["context"]["system_before"]["kern.memorystatus_vm_pressure_level"] = "1"
        a["context"]["system_after"] = {"kern.memorystatus_vm_pressure_level": "2"}
        r = g.analyse(a["context"], a["checkpoints"], [])
        self.assertTrue(any(x["metric"] == "memory_pressure" and x["status"] == "UNKNOWN" for x in r["findings"]))
        self.assertEqual(g.compare(a, a)["status"], "UNKNOWN")


class NativeProbe(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        g.PROBE.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["xcrun", "clang", "-O2", "-Wall", "-Wextra", "-Werror", str(ROOT / "scripts/lib/resource-probe.c"), "-o", str(g.PROBE)], check=True)

    def test_real_process_counters_and_missing_process(self):
        before = g.probe("--sample", os.getpid())[0]
        until = time.process_time() + .05
        while time.process_time() < until: pass
        after = g.probe("--sample", os.getpid())[0]
        self.assertEqual(before["start"], after["start"])
        self.assertGreater(after["cpu_ns"] - before["cpu_ns"], 10_000_000)
        self.assertGreater(after["footprint"], 0)
        self.assertGreater(after["fds"], 0)
        self.assertIn("error", g.probe("--sample", 2147483647)[0])


if __name__ == "__main__":
    unittest.main()
