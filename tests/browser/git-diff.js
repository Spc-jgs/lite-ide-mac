// IPC 桩中的 Git / 差异回归：在 VITE_TEST_BRIDGE=1 的 1432 端口上由 playwright-cli 执行。
async page => {
 const checks=[],data={};const ok=(pass,label)=>{checks.push({pass:!!pass,label});if(!pass)throw new Error(label);};
 await page.addInitScript(()=>{for(const k of Object.keys(localStorage))if(k==='lite-ide.layout'||k.startsWith('lite-ide.session'))localStorage.removeItem(k);});
 await page.setViewportSize({width:1500,height:1000});await page.goto('http://127.0.0.1:1432');
 await page.waitForFunction(()=>window.__lite?.git.status);
 await page.evaluate(async()=>{
  const original=window.__TAURI_INTERNALS__.invoke;
  window.__gitQA={original,rows:await original('git_log_entries',{root:'/proj'}),status:JSON.parse(JSON.stringify(window.__lite.git.status)),calls:[],fail:false,slow:0};
  window.__TAURI_INTERNALS__.invoke=async(cmd,args,...rest)=>{
   const t=window.__gitQA;t.calls.push({cmd,args});
   if(cmd==='git_status')return structuredClone(t.status);
   if(cmd==='git_log_entries'){const rows=structuredClone(t.rows),fail=t.fail;if(t.slow)await new Promise(r=>setTimeout(r,t.slow));if(fail)throw new Error('测试：历史读取失败');return rows;}
   return original(cmd,args,...rest);
  };
 });
 await page.getByRole('button',{name:'Git',exact:true}).click();await page.locator('.log .crow').first().waitFor();
 await page.locator('.log .crow').nth(2).click();const selected=await page.locator('.log .crow.on').getAttribute('title');
 await page.locator('.log .q').fill('终端');
 await page.evaluate(()=>{const t=window.__gitQA;t.rows.unshift({...t.rows[0],sha:'a'.repeat(40),short:'aaaaaaa',subject:'外部新增提交',parents:[t.rows[0].sha],refs:['HEAD']});t.status.head='aaaaaaa';});
 await page.getByRole('button',{name:'刷新 Git',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.log .cnt')?.textContent.includes('/ 12'));
 ok(await page.locator('.log .q').inputValue()==='终端','刷新保留关键词');ok(await page.locator('.log .crow.on').getAttribute('title')===selected,'刷新保留当前提交');
 await page.getByRole('button',{name:'跳到最新提交',exact:true}).click();ok((await page.locator('.log .crow.on').innerText()).includes('外部新增提交'),'跳到最新提交清除关键词并定位');
 await page.evaluate(()=>{const t=window.__gitQA;t.rows.unshift({...t.rows[0],sha:'b'.repeat(40),short:'bbbbbbb',subject:'焦点切回后的提交'});t.status.head='bbbbbbb';window.dispatchEvent(new Event('focus'));});
 await page.locator('.log .crow').filter({hasText:'焦点切回后的提交'}).waitFor();ok(true,'窗口恢复焦点自动刷新历史');
 await page.evaluate(async()=>{const t=window.__gitQA;t.rows.unshift({...t.rows[0],sha:'c'.repeat(40),short:'ccccccc',subject:'远程引用变化'});(await import('/src/lib/dev/mock/events.ts')).mockEmit('fs-changed','git');});
 await page.locator('.log .crow').filter({hasText:'远程引用变化'}).waitFor();ok(true,'.git 事件在 HEAD 不变时也刷新');
 const count=await page.evaluate(()=>window.__gitQA.calls.filter(c=>c.cmd==='git_log_entries').length);
 await page.evaluate(()=>window.__lite.git.refresh());await page.waitForTimeout(60);
 ok(await page.evaluate(()=>window.__gitQA.calls.filter(c=>c.cmd==='git_log_entries').length)===count,'普通工作区保存不重读历史');
 await page.evaluate(()=>window.__gitQA.fail=true);await page.getByRole('button',{name:'刷新 Git',exact:true}).click();await page.locator('.log .load-error').waitFor();
 ok(await page.locator('.log .crow').count()===14,'刷新失败保留列表并报错');
 await page.evaluate(()=>window.__gitQA.fail=false);await page.locator('.log .load-error').getByRole('button',{name:'重试',exact:true}).click();await page.locator('.log .load-error').waitFor({state:'detached'});ok(true,'失败后原地重试');
 await page.evaluate(()=>window.__gitQA.slow=250);await page.getByRole('button',{name:'刷新 Git',exact:true}).click();await page.locator('.log .crow').nth(4).click();const during=await page.locator('.log .crow.on').getAttribute('title');await page.waitForTimeout(350);
 ok(await page.locator('.log .crow.on').getAttribute('title')===during,'刷新中更换选择不被旧选择覆盖');await page.evaluate(()=>window.__gitQA.slow=0);
 await page.getByRole('treeitem',{name:'README.md A',exact:true}).click({button:'right'});await page.getByRole('menuitem',{name:'查看提交历史',exact:true}).click();await page.locator('.log .pin').waitFor();
 ok((await page.locator('.log .pin').innerText()).includes('README.md'),'文件树历史绑定右键文件');ok(await page.evaluate(()=>window.__gitQA.calls.filter(c=>c.cmd==='git_log_entries').at(-1).args.path)==='README.md','文件历史请求的相对路径正确');
 await page.getByRole('button',{name:'清除筛选',exact:true}).click();await page.locator('.log .crow').first().click({button:'right'});ok(await page.getByRole('menuitem',{name:'复制哈希',exact:true}).count()===1,'提交右键菜单保留');await page.keyboard.press('Escape');
 await page.locator('.log .drow').first().click({button:'right'});ok(await page.getByRole('menuitem',{name:'打开工作区文件',exact:true}).count()===1,'历史文件菜单补齐打开文件和路径');await page.keyboard.press('Escape');
 await page.locator('.log .drow').first().click();await page.locator('div.diff').waitFor();
 const raw='diff --git a/src/OrderService.java b/src/OrderService.java\n--- a/src/OrderService.java\n+++ b/src/OrderService.java\n@@ -200,87 +200,93 @@\n'+Array.from({length:3},(_,i)=>' // context '+i+'\n').join('')+'-    private String token = "old";\n+    private String token = "'+'long content '.repeat(35)+'";\n'+Array.from({length:6},(_,i)=>'+    private int added'+i+' = '+i+';\n').join('')+Array.from({length:83},(_,i)=>' // context after '+i+'\n').join('');
 await page.evaluate(raw=>{window.__lite.tabs.active.diffRaw=raw;},raw);await page.waitForTimeout(120);const diff=page.locator('div.diff');
 ok(await diff.locator('.grid').count()===1,'默认双栏');ok((await diff.locator('.sources').innerText()).includes('提交前'),'显示比较来源');ok(await diff.locator('.change-gutter').count()>80,'中央独立分隔');
 data.side=await diff.evaluate(el=>({body:el.querySelector('.body').getBoundingClientRect().toJSON(),scroll:el.querySelector('.scrolls').getBoundingClientRect().toJSON(),width:el.clientWidth,grid:el.querySelector('.grid').clientWidth}));
 ok(data.side.width===data.side.grid,'长行不撑宽双栏');ok(Math.abs(data.side.scroll.top-data.side.body.bottom)<2,'横向滚动条固定在可视底边');
 const bars=diff.getByRole('scrollbar');ok(await bars.count()===2,'双栏均有横向滚动入口');await bars.first().focus();await page.keyboard.press('End');const sx=await bars.first().getAttribute('aria-valuenow');ok(Number(sx)>0&&await bars.nth(1).getAttribute('aria-valuenow')===sx,'双栏滚动同步');await page.keyboard.press('Home');
 const br=await bars.first().boundingBox();await page.mouse.move(br.x+8,br.y+5);await page.mouse.down();await page.mouse.move(br.x+90,br.y+5);await page.mouse.up();ok(Number(await bars.first().getAttribute('aria-valuenow'))>0,'滚动条支持拖动');await bars.first().focus();await page.keyboard.press('Home');await page.screenshot({path:'/tmp/liteide-git-diff-side.png'});
 await diff.getByRole('button',{name:'统一',exact:true}).click();ok(await diff.locator('.uni').count()===1,'统一视图可切换');
 data.unified=await diff.locator('.uni').evaluate(el=>({width:el.clientWidth,row:el.querySelector('.row.add').clientWidth,numbers:[...el.querySelectorAll('.row.add:first-of-type .no')].map(e=>e.getBoundingClientRect().left)}));
 const numsBefore=await diff.locator('.uni .row.add').first().locator('.no').evaluateAll(els=>els.map(e=>e.getBoundingClientRect().left));
 ok(data.unified.width===data.unified.row&&numsBefore.length===2,'统一视图双行号且不被长行撑宽');await diff.getByRole('scrollbar').focus();await page.keyboard.press('End');const numsAfter=await diff.locator('.uni .row.add').first().locator('.no').evaluateAll(els=>els.map(e=>e.getBoundingClientRect().left));ok(JSON.stringify(numsBefore)===JSON.stringify(numsAfter),'统一视图横向滚动时行号固定');
 await diff.getByRole('button',{name:'换行',exact:true}).click();ok(await diff.getByRole('scrollbar').count()===0,'换行后收起横向滚动条');ok(await diff.locator('.uni .row.add').first().evaluate(e=>e.clientHeight)>40,'统一视图长行换行');await page.screenshot({path:'/tmp/liteide-git-diff-unified.png'});
 await diff.getByRole('button',{name:'双栏',exact:true}).click();ok(await diff.locator('.grid .tx.add').first().evaluate(e=>e.clientHeight)>40,'双栏长行换行');const pair=await diff.locator('.grid .tx.add').first().evaluate(e=>({l:e.previousElementSibling.previousElementSibling.previousElementSibling.clientHeight,r:e.clientHeight}));ok(pair.l===pair.r,'双栏两侧行高对齐');
 await diff.locator('.body').focus();await page.keyboard.press('Shift+F10');ok(await page.getByRole('menuitem',{name:'下一处更改',exact:true}).count()===1,'差异区键盘菜单');await page.keyboard.press('Escape');
 await page.setViewportSize({width:900,height:850});await page.waitForTimeout(120);ok(await diff.locator('.uni').count()===1,'窄区域自动统一');await page.setViewportSize({width:1500,height:1000});await page.waitForTimeout(120);ok(await diff.locator('.grid').count()===1,'拉宽恢复用户双栏选择');
 await page.getByRole('button',{name:'Git 更改',exact:true}).click();await page.locator('.frow[title="README.md"]').focus();await page.keyboard.press('Shift+F10');ok(await page.getByRole('menuitem',{name:'查看已暂存更改',exact:true}).isEnabled(),'更改列表键盘菜单');await page.keyboard.press('Escape');
 await page.evaluate(()=>{window.__TAURI_INTERNALS__.invoke=window.__gitQA.original;});
 const failed=checks.filter(c=>!c.pass);if(failed.length)throw new Error(JSON.stringify({checks,data}));return {passed:checks.length,failed:failed.length,checks,data};
}
