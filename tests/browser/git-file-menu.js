// 分屏中的 Git 菜单必须绑定右键文件，而非活动标签。
async page => {
 const checks=[];const ok=(pass,label)=>{if(!pass)throw new Error(label);checks.push(label);};
 await page.unrouteAll();await page.setViewportSize({width:1500,height:1000});
 await page.addInitScript(()=>{for(const k of Object.keys(localStorage))if(k==='lite-ide.layout'||k.startsWith('lite-ide.session'))localStorage.removeItem(k);});
 await page.goto('http://127.0.0.1:1432');await page.waitForFunction(()=>window.__lite?.git.status);
 await page.evaluate(async()=>{
  await window.__lite.tabflow.openPath('/proj/README.md',{preview:false});
  await window.__lite.tabflow.openPath('/proj/src/OrderService.java',{preview:false});
  const {tabs}=window.__lite;tabs.moveToOther(tabs.activeId);tabs.focusGroup(0);
  const invoke=window.__TAURI_INTERNALS__.invoke;window.__menuCalls=[];
  window.__TAURI_INTERNALS__.invoke=(cmd,args,...rest)=>{window.__menuCalls.push({cmd,args});return invoke(cmd,args,...rest);};
 });
 await page.locator('.cm-content').nth(1).waitFor();
 await page.locator('.cm-content').nth(1).evaluate(el=>{const r=el.getBoundingClientRect();el.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:r.left+15,clientY:r.top+45}));});
 await page.getByRole('menuitem',{name:'查看提交历史',exact:true}).waitFor();
 ok(await page.evaluate(()=>window.__lite.tabs.active.path)==='/proj/README.md','右键前活动文件仍是另一组 README');
 await page.getByRole('menuitem',{name:'查看提交历史',exact:true}).click();await page.locator('.log .pin').waitFor();
 ok((await page.locator('.log .pin').innerText()).includes('OrderService.java'),'分屏文件历史绑定右键 Java 文件');
 ok(await page.evaluate(()=>window.__menuCalls.filter(c=>c.cmd==='git_log_entries').at(-1).args.path)==='src/OrderService.java','文件历史请求路径正确');
 await page.locator('.cm-content').nth(1).evaluate(el=>{const r=el.getBoundingClientRect();el.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:r.left+15,clientY:r.top+45}));});
 await page.getByRole('menuitem',{name:'这一行的历史',exact:true}).click();await page.waitForTimeout(100);
 const request=await page.evaluate(()=>window.__menuCalls.filter(c=>c.cmd==='git_log_entries').at(-1).args);
 ok(request.path==='src/OrderService.java'&&request.lines?.[0]>1,'行历史使用右键文件自身光标行');
 await page.getByRole('button',{name:'Git 更改',exact:true}).click();
 const row=page.locator('.frow[title="src/App.svelte"]');await row.click({button:'right'});
 ok(await page.getByRole('menuitem',{name:'查看工作区更改',exact:true}).isEnabled(),'未暂存文件开放工作区更改');
 ok(!await page.getByRole('menuitem',{name:'查看已暂存更改',exact:true}).isEnabled(),'不存在的暂存更改入口灰显');
 await page.getByRole('menuitem',{name:'放弃文件更改…',exact:true}).click();
 ok(await page.evaluate(()=>window.__lite.git.pendingDiscard?.[0].path)==='src/App.svelte','放弃只登记右键文件，等待确认');
 ok(await page.evaluate(()=>window.__menuCalls.filter(c=>c.cmd==='git_discard').length)===0,'确认前没有执行丢弃');
 await page.evaluate(()=>{window.__lite.git.pendingDiscard=null;});
 return {passed:checks.length,checks,lines:request.lines};
}
