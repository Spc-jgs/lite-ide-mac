// 克隆弹窗上收到凭据询问：使用演示组件，不发网络请求或真实凭据。
async page => {
  const checks = [];
  const ok = (pass, message) => checks.push({ pass: !!pass, message });
  await page.goto('http://127.0.0.1:1432');
  await page.waitForFunction(() => window.__lite?.project.root === '/proj');
  for (const parentFirst of [false, true]) {
    await page.evaluate(async () => {
      const { openModal } = await import('/src/lib/settings/modal-host.ts');
      const { default: Clone } = await import('/src/lib/welcome/CloneDialog.svelte');
      window.__uiCloseParent = openModal(Clone, { recent: [] });
    });
    const parent = page.getByRole('dialog', { name: '从 Git 克隆', exact: true }); await parent.waitFor();
    await page.evaluate(async () => {
      const { openModal } = await import('/src/lib/settings/modal-host.ts');
      const { default: Ask } = await import('/src/lib/git/GitAskpass.svelte');
      window.__uiCloseChild = openModal(Ask, { prompt: '测试用户名', secret: false, onanswer: () => {} });
    });
    const child = page.getByRole('dialog', { name: 'Git 凭据', exact: true }); await child.waitFor();
    await child.locator('input').focus();
    await page.keyboard.type('demo-user');
    ok(await child.locator('input').inputValue() === 'demo-user', `凭据窗口可输入，父窗口不抢焦点（${parentFirst}）`);
    await page.keyboard.press('Tab');
    ok(await child.evaluate(e => e.contains(document.activeElement)), `嵌套窗口 Tab 留在最上层（${parentFirst}）`);
    if (parentFirst) {
      await page.evaluate(() => window.__uiCloseParent());
      await child.locator('input').fill('still-editable');
      ok(await child.locator('input').inputValue() === 'still-editable', '底层先关闭时上层继续可用');
      await page.evaluate(() => window.__uiCloseChild());
    } else {
      await child.getByRole('button', { name: '取消', exact: true }).click(); await child.waitFor({ state: 'detached' });
      ok(await parent.evaluate(e => e.contains(document.activeElement)) && await page.locator('.workspace').evaluate(e => !!e.closest('[inert]')), '上层关闭后焦点与模态返回底层');
      await page.evaluate(() => window.__uiCloseParent());
    }
    await parent.waitFor({ state: 'detached' });
    ok(!await page.locator('.workspace').evaluate(e => !!e.closest('[inert]')), `全部关闭后恢复背景（${parentFirst}）`);
  }
  const failed = checks.filter(c => !c.pass);
  if (failed.length) throw new Error(JSON.stringify({ checks }));
  return { passed: checks.length, checks };
}
