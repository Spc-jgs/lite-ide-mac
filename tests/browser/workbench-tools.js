// 延伸到 Services / 构建结果 / 调试 / 文件树菜单。只注入演示运行记录，不启动 Java。
async page => {
  const checks = [];
  const ok = (condition, message) => checks.push({ message, pass: !!condition });
  await page.setViewportSize({ width: 1280, height: 850 });
  await page.goto('http://127.0.0.1:1432');
  await page.waitForFunction(() => window.__lite?.project.root === '/proj');
  await page.evaluate(() => { window.__lite.runs.services = 1; });
  await page.getByRole('button', { name: 'Services', exact: true }).click();
  const svc = page.getByRole('separator', { name: '调整服务列表宽度', exact: true }); await svc.waitFor();
  const serviceBefore = await page.locator('.svc > .left').evaluate(e => e.clientWidth);
  await svc.focus(); await page.keyboard.press('ArrowRight');
  ok(await page.locator('.svc > .left').evaluate(e => e.clientWidth) === serviceBefore + 8, 'Services 树宽度可调');
  const b = await svc.boundingBox();
  await page.mouse.move(b.x + 3, b.y + 20); await page.mouse.down();
  await svc.evaluate(e => e.dispatchEvent(new PointerEvent('pointercancel', { pointerId: 1, bubbles: true })));
  await page.mouse.up();
  ok(!await svc.evaluate(e => e.classList.contains('active')), '手势取消后清掉拖动状态');
  await page.evaluate(() => {
    const r = { id: 999, root: '/proj', name: '构建项目', command: 'mvn compile', log: '/proj/logs/inventory.log', status: 'done', code: 0, holder: null, service: false, startedAt: 0, tests: null, debug: null, http: null, problems: { items: [], total: 0 } };
    window.__lite.runs.list = [r]; window.__lite.runs.activeId = r.id;
  });
  await page.getByRole('button', { name: '运行', exact: true }).filter({ has: page.locator('[data-n="run"]') }).last().click();
  const run = page.getByRole('separator', { name: '调整运行结果树宽度', exact: true }); await run.waitFor();
  const runBefore = await page.locator('.tests-pane').evaluate(e => e.clientWidth);
  await run.focus(); await page.keyboard.press('ArrowRight');
  ok(await page.locator('.tests-pane').evaluate(e => e.clientWidth) === runBefore + 8, '构建结果树宽度可调');
  await page.evaluate(() => {
    window.__lite.runs.list[0].debug = { run: 999, phase: 'ended', note: '', thread: null, reason: '', frames: [], totalFrames: 0, threads: [] };
  });
  await page.getByRole('tab', { name: '调试器', exact: true }).click();
  const debug = page.getByRole('separator', { name: '调整调用栈宽度', exact: true }); await debug.waitFor();
  const debugBefore = await page.locator('.frames').evaluate(e => e.clientWidth);
  await debug.focus(); await page.keyboard.press('ArrowRight');
  ok(await page.locator('.frames').evaluate(e => e.clientWidth) === debugBefore + 8, '调试调用栈宽度可调');
  const side = page.getByRole('separator', { name: '调整侧边栏宽度', exact: true });
  const sideBefore = Number(await side.getAttribute('aria-valuenow'));
  await side.focus(); await page.keyboard.press('ArrowRight');
  ok(Number(await side.getAttribute('aria-valuenow')) === sideBefore + 8, '侧边栏可用键盘调整');
  await side.dblclick();
  ok(Number(await side.getAttribute('aria-valuenow')) === 240, '侧边栏双击恢复默认宽度');
  await page.getByRole('button', { name: '项目视图选项', exact: true }).click();
  ok(await page.getByRole('menuitemcheckbox').count() === 2, '文件树两个独立开关使用 checkbox 语义');
  await page.keyboard.press('Escape');
  const failed = checks.filter(c => !c.pass);
  if (failed.length) throw new Error(JSON.stringify({ checks }));
  return { passed: checks.length, checks };
}
