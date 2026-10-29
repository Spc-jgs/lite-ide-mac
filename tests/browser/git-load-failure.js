// 模拟底部工具窗模块请求失败：只报告一次，不因 loading 恢复反复拉取。
async page => {
 await page.unrouteAll();let requests=0;const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/terminal/Terminal.svelte*',route=>{requests++;return route.abort();});
 await page.addInitScript(()=>{for(const k of Object.keys(localStorage))if(k==='lite-ide.layout'||k.startsWith('lite-ide.session'))localStorage.removeItem(k);});
 await page.goto('http://127.0.0.1:1432');await page.waitForFunction(()=>window.__lite?.git.status);
 await page.getByRole('button',{name:'Git',exact:true}).click();await page.waitForTimeout(350);
 const before=await page.evaluate(()=>window.__lite.notify.log.filter(n=>n.text.includes('终端加载失败')).length);
 await page.waitForTimeout(350);
 const after=await page.evaluate(()=>window.__lite.notify.log.filter(n=>n.text.includes('终端加载失败')).length);
 await page.unrouteAll();
 if(before!==1||after!==1||errors.length)throw new Error(JSON.stringify({before,after,requests,errors}));
 return {passed:3,before,after,requests,errors};
}
