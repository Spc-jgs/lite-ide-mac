// 在 VITE_TEST_BRIDGE=1 的 pnpm dev（1432）上，用 playwright-cli run-code --filename 执行。
// 只用 IPC 桩；延迟和失败为故意注入，不触及正式应用或真实项目。
async page => {
  const checks = [], measurements = {};
  const ok = (condition, message) => checks.push({ message, pass: !!condition });
  await page.setViewportSize({ width: 1280, height: 850 });
  await page.addInitScript(() => { for (const key of Object.keys(localStorage)) if (key === 'lite-ide.layout' || key.startsWith('lite-ide.session')) localStorage.removeItem(key); });
  await page.goto('http://127.0.0.1:1432');
  await page.waitForFunction(() => window.__lite?.project.root === '/proj');
  await page.evaluate(async () => {
    window.__uiOriginalInvoke = window.__TAURI_INTERNALS__.invoke;
  });
  await page.getByRole('button', { name: 'Git', exact: true }).click();
  await page.locator('.log .crow').first().waitFor();
  try {
    await page.evaluate(() => {
      window.__uiCalls = [];
      window.__TAURI_INTERNALS__.invoke = async (cmd, args, ...rest) => {
        if (cmd === 'git_commit_files') {
          const n = window.__uiCalls.push(args.sha);
          await new Promise(r => setTimeout(r, n === 1 ? 350 : 40));
          return [{ path: args.sha + '.java', index: 'M', worktree: ' ', untracked: false, orig: null }];
        }
        return window.__uiOriginalInvoke(cmd, args, ...rest);
      };
    });
    await page.locator('.log .crow').nth(1).click();
    await page.waitForFunction(() => window.__uiCalls.length === 1);
    await page.locator('.log .crow').nth(2).click();
    await page.waitForTimeout(450);
    measurements.commitRace = await page.evaluate(() => ({
      selected: document.querySelector('.log .sha')?.textContent,
      file: document.querySelector('.log .drow')?.getAttribute('title'), calls: window.__uiCalls,
    }));
    ok(measurements.commitRace.file === measurements.commitRace.calls[1] + '.java', '乱序返回只展示当前提交文件');
    await page.evaluate(() => {
      window.__TAURI_INTERNALS__.invoke = async (cmd, args, ...rest) => {
        if (cmd === 'git_commit_files') throw new Error('模拟读取失败');
        return window.__uiOriginalInvoke(cmd, args, ...rest);
      };
    });
    await page.locator('.log .crow').nth(3).click();
    await page.waitForTimeout(100);
    const failed = await page.locator('.log .right').innerText();
    ok(failed.includes('模拟读取失败') && !failed.includes('0 个文件') && !failed.includes('没有文件'), '读取失败不冒充空提交');
    ok(await page.locator('.log .right').getByRole('button', { name: '重试', exact: true }).count() === 1, '读取失败提供重试');
    await page.evaluate(() => { window.__TAURI_INTERNALS__.invoke = window.__uiOriginalInvoke; });
    await page.locator('.log .crow').nth(4).click();
    await page.waitForTimeout(100);

    const detail = page.getByRole('separator', { name: '调整提交详情宽度', exact: true });
    await detail.focus(); await page.keyboard.press('ArrowLeft');
    ok(await detail.getAttribute('aria-valuenow') === '328', 'Git 分栏支持方向键');
    const box = await detail.boundingBox();
    await page.mouse.move(box.x + 3, box.y + 30); await page.mouse.down();
    await page.mouse.move(box.x - 37, box.y + 30); await page.mouse.up();
    ok(await detail.getAttribute('aria-valuenow') === '368', 'Git 分栏支持拖动');
    await detail.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '恢复默认尺寸', exact: true }).click();
    ok(await detail.getAttribute('aria-valuenow') === '320', '分隔条右键尺寸操作可用');
    await page.getByRole('separator', { name: '调整提交说明高度', exact: true }).focus();
    await page.keyboard.press('ArrowDown');
    ok(await page.getByRole('separator', { name: '调整提交说明高度', exact: true }).getAttribute('aria-valuenow') === '104', '提交说明高度可调整');

    await page.getByRole('button', { name: '最大化工具窗', exact: true }).click();
    const maximum = await page.locator('.panel').evaluate(e => e.clientHeight);
    await page.getByRole('button', { name: '恢复工具窗', exact: true }).click();
    ok(maximum > 500 && await page.locator('.panel').evaluate(e => e.clientHeight) < 300, '最大化工具窗后恢复正常高度');

    measurements.widths = [];
    for (const width of [1280, 1000, 900, 800, 720]) {
      await page.setViewportSize({ width, height: 850 }); await page.waitForTimeout(80);
      const size = await page.locator('.log .subject').first().evaluate(e => e.clientWidth);
      measurements.widths.push({ width, titleWidth: size });
      ok(size > 100, `${width}px 提交标题仍有可读空间`);
    }
    await page.locator('.log .crow').first().click();
    ok(await page.locator('.log .right').isVisible() && !await page.locator('.log .left').isVisible(), '窄区域进入单栏详情');
    await page.getByRole('button', { name: '返回列表', exact: true }).click();
    ok(await page.locator('.log .left').isVisible(), '窄区域可返回提交列表');
    await page.evaluate(() => {
      document.querySelector('.twidget.proj .wlabel').textContent = 'project-'.repeat(40);
      const branch = document.querySelectorAll('.twidget .wlabel')[1]; if (branch) branch.textContent = 'feature/'.repeat(40);
    });
    measurements.stop = await page.locator('.rw [aria-label="停止"]').evaluate(e => ({ right: e.getBoundingClientRect().right, viewport: innerWidth }));
    ok(measurements.stop.right <= measurements.stop.viewport, '长项目名和分支不挤走停止按钮');
    await page.setViewportSize({ width: 1280, height: 850 });

    await page.evaluate(async () => {
      const m = await import('/src/lib/state/keymap.ts'); window.__uiKeyOverrides = m.overrides();
      m.setOverrides({ 'tw-structure': '⌥⌘7', 'debug-last': '⌥⌘D' });
    });
    await page.getByRole('button', { name: '结构', exact: true }).hover();
    await page.waitForTimeout(100);
    ok((await page.getByRole('button', { name: '结构', exact: true }).getAttribute('title')).includes('⌥⌘7'), '导轨提示读取生效键位');
    ok((await page.locator('.rw [aria-label="调试"]').getAttribute('title')).includes('⌥⌘D'), '运行提示随键位更新');
    await page.evaluate(async () => { (await import('/src/lib/state/keymap.ts')).setOverrides(window.__uiKeyOverrides); });

    await page.evaluate(async () => { (await import('/src/lib/settings/open.ts')).openSettingsDialog('/proj'); });
    const settings = page.getByRole('dialog', { name: '设置', exact: true }); await settings.waitFor();
    await settings.getByRole('button', { name: '确定', exact: true }).focus(); await page.keyboard.press('Tab');
    ok(await settings.evaluate(e => e.contains(document.activeElement)), '设置末尾 Tab 留在对话框内');
    await page.keyboard.press('Shift+Tab');
    ok(await settings.evaluate(e => e.contains(document.activeElement)), '设置 Shift+Tab 留在对话框内');
    ok(await settings.getAttribute('aria-modal') === 'true' && await page.locator('.workspace').evaluate(e => !!e.closest('[inert]')), '设置声明模态并隔离背景');
    const font = settings.locator('.row').filter({ has: page.getByText('editor.fontSize', { exact: true }) }).locator('input');
    await font.fill('invalid');
    await settings.getByRole('button', { name: '终端', exact: true }).click();
    await settings.getByRole('button', { name: '确定', exact: true }).click();
    ok(await font.isVisible() && await font.inputValue() === 'invalid' && await font.getAttribute('aria-invalid') === 'true', '错误字段自动显示且保留无效输入');
    ok(await font.evaluate(e => e === document.activeElement), '确定时聚焦首个错误字段');
    await font.fill('14');
    await page.evaluate(() => {
      window.__uiSaving = false;
      window.__TAURI_INTERNALS__.invoke = async (cmd, args, ...rest) => {
        if (cmd === 'settings_apply') { window.__uiSaving = true; await new Promise(r => setTimeout(r, 500)); }
        return window.__uiOriginalInvoke(cmd, args, ...rest);
      };
    });
    await settings.getByRole('button', { name: '应用', exact: true }).click();
    await page.waitForFunction(() => window.__uiSaving);
    ok(await settings.getByRole('button', { name: '取消', exact: true }).isDisabled(), '提交保存后取消禁用');
    await page.keyboard.press('Escape');
    ok(await settings.isVisible(), '保存中 Esc 不关闭表单');
    await page.waitForTimeout(550);
    await settings.getByRole('button', { name: '取消', exact: true }).click(); await settings.waitFor({ state: 'detached' });
    ok(!await page.locator('.workspace').evaluate(e => !!e.closest('[inert]')), '关闭模态后背景恢复交互');
    await page.evaluate(() => { window.__TAURI_INTERNALS__.invoke = window.__uiOriginalInvoke; });

    await page.evaluate(async () => { (await import('/src/lib/settings/open.ts')).openRunConfigs('/proj'); });
    const configs = page.getByRole('dialog', { name: '运行配置', exact: true }); await configs.waitFor();
    ok(await configs.evaluate(e => e.contains(document.activeElement)), '运行配置打开时获得焦点');
    await configs.getByRole('button', { name: '确定', exact: true }).focus(); await page.keyboard.press('Tab');
    ok(await configs.evaluate(e => e.contains(document.activeElement)), '运行配置 Tab 循环');
    await configs.getByRole('button', { name: '取消', exact: true }).click(); await configs.waitFor({ state: 'detached' });

    await page.getByRole('separator', { name: '调整底部工具窗高度', exact: true }).focus();
    await page.keyboard.press('End');
    await page.evaluate(async () => {
      await window.__lite.tabflow.openPath('/proj/README.md');
      const t = window.__lite.tabs.active; t.dirty = true;
      document.querySelector('.log .crow')?.focus({ preventScroll: true });
      window.__lite.tabflow.requestClose(t.id);
    });
    const confirm = page.getByRole('alertdialog', { name: '保存更改', exact: true }); await confirm.waitFor();
    measurements.confirm = await confirm.getByRole('button', { name: '保存', exact: true }).evaluate(e => ({ top: e.getBoundingClientRect().top, bottom: e.getBoundingClientRect().bottom, viewport: innerHeight, visible: !!e.getClientRects().length }));
    ok(measurements.confirm.visible && measurements.confirm.bottom < measurements.confirm.viewport, '高工具窗下保存按钮仍在窗口内');
    await page.keyboard.press('Tab');
    ok(await confirm.evaluate(e => e.contains(document.activeElement)), '确认框 Tab 留在内部');
    await confirm.getByRole('button', { name: '取消', exact: true }).click();
    await page.waitForTimeout(180);
    await page.evaluate(() => { if (window.__lite.tabs.active) window.__lite.tabs.active.dirty = false; });
    ok(await page.locator('.layer .dialog').count() === 0, '移到窗口层的确认按钮事件仍有效');
    ok(await page.locator('.log .crow').first().evaluate(e => e === document.activeElement), '确认关闭后归还原焦点');

    await page.evaluate(async () => {
      const { notify } = window.__lite;
      notify.dismiss(); notify.ok('测试通知', 300);
    });
    await page.getByRole('button', { name: '关闭通知', exact: true }).focus();
    await page.waitForTimeout(500);
    ok(await page.getByRole('button', { name: '关闭通知', exact: true }).isVisible(), '键盘停在通知操作上暂停自动关闭');
    await page.getByRole('button', { name: '关闭通知', exact: true }).click();
    ok(await page.evaluate(() => window.__lite.notify.toast === null), '通知有明确关闭入口');
  } finally {
    await page.evaluate(() => { window.__TAURI_INTERNALS__.invoke = window.__uiOriginalInvoke; });
  }
  const failed = checks.filter(c => !c.pass);
  if (failed.length) throw new Error(JSON.stringify({ failed, checks, measurements }));
  return { passed: checks.length, checks, measurements };
}
