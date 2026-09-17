import {
  autosaveDue,
  scratchSaveState,
  AUTOSAVE_IDLE_MS,
  AUTOSAVE_RETRY_MS,
  type AutosaveInput,
} from "../src/lib/state/autosave.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) {
    pass++;
  } else {
    fail++;
    console.error("  ✗ " + m);
  }
};

/** 一份「该存」的基线：草稿、编辑中、脏、没冲突、空闲够久、没失败过 */
const base: AutosaveInput = {
  scratch: true,
  editing: true,
  dirty: true,
  conflict: false,
  idleMs: AUTOSAVE_IDLE_MS,
  failedMs: null,
  force: false,
};

// ── 1. 基线要存 ──
ok(autosaveDue(base), "草稿、脏、空闲够久：该存");

// ── 2. 非草稿永远不存 —— 这是整个判据里最不能静默失效的一条 ──
ok(!autosaveDue({ ...base, scratch: false }), "项目文件不自动存");
ok(!autosaveDue({ ...base, scratch: false, force: true }), "项目文件强制也不存");
ok(
  !autosaveDue({ ...base, scratch: false, idleMs: Infinity, force: true }),
  "项目文件：别的条件全满足也不存",
);

// ── 3. 没东西可存的情况 ──
ok(!autosaveDue({ ...base, dirty: false }), "不脏就不写");
ok(!autosaveDue({ ...base, editing: false }), "日志 / 差异模式没有保存这回事");
ok(!autosaveDue({ ...base, conflict: true }), "有冲突交给「用磁盘上的 / 保留我的」，不自动盖");
ok(!autosaveDue({ ...base, conflict: true, force: true }), "有冲突强制也不盖");

// ── 4. 空闲期 ──
ok(!autosaveDue({ ...base, idleMs: AUTOSAVE_IDLE_MS - 1 }), "还在打字（没到空闲期）不写");
ok(autosaveDue({ ...base, idleMs: AUTOSAVE_IDLE_MS - 1, force: true }), "强制时不等空闲期");
ok(autosaveDue({ ...base, idleMs: Infinity }), "从没输入过（恢复出来的脏草稿）也算空闲");

// ── 5. 失败退避 ──
ok(!autosaveDue({ ...base, failedMs: 0 }), "刚失败过不立刻重试");
ok(!autosaveDue({ ...base, failedMs: AUTOSAVE_RETRY_MS - 1 }), "退避期内不重试");
ok(autosaveDue({ ...base, failedMs: AUTOSAVE_RETRY_MS }), "退避期过了再试");
ok(!autosaveDue({ ...base, failedMs: 0, force: true }), "退避期内强制也不硬撞 —— 盘满不会在半秒内自己好");

// ── 「存了没存」的三个状态 ──
{
  ok(scratchSaveState({ scratch: false, dirty: true, failed: true }) === null, "非草稿一律 null，走原来的圆点");
  ok(scratchSaveState({ scratch: true, dirty: true, failed: false }) === "pending", "改了没落盘：pending");
  ok(scratchSaveState({ scratch: true, dirty: false, failed: false }) === "saved", "干净：saved");
  ok(scratchSaveState({ scratch: true, dirty: true, failed: true }) === "failed", "失败盖过一切");
  ok(scratchSaveState({ scratch: true, dirty: false, failed: true }) === "failed", "失败标记没清之前，干净也算失败（要人看见）");
}

console.log(`autosave: ${pass} passed, ${fail} failed`);
if (fail > 0) process.exit(1);
