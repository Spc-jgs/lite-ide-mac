//! Dock 图标右键 →「在草稿本新建」（docs/SCRATCH.md 7.3）：人在别的应用里想记一句，手就在 Dock 上 ——
//! 备忘录的「新建备忘录」、Safari 的「新建窗口」都放在这儿。
//!
//! Dock 右键菜单要应用代理答 `applicationDockMenu:`。代理是 tao 建的，那个类上没有这个方法，Tauri 也没开放 ——
//! 和 `terminate.rs` 同一个办法：启动时给活着的代理对象的类补上（`class_addMethod`，装不上往 app.log 写 WARN、不覆盖已有的），
//! 外加菜单项要调的动作 `liteScratchbook:`（菜单项的 target 就是代理自己）。
//!
//! **只用 `objc2` 的 `msg_send!`，不引 `objc2-foundation`**：那要多一个直接依赖（`NSString` 的类型在它里面）。
//! `msg_send!` 在调试构建里会核对消息的类型编码，写错了当场报，不是悄悄崩。
//! 菜单启动时建一份、留到进程结束（不释放）：Dock 每次来要都给同一份，不用每次右键都管一遍引用计数。
//!
//! 测试身份的 `.app` 是 Accessory 应用，**不进 Dock**（#54，免得抢焦点），所以这条路自动验收测不到，只能在正式包上点一下。
//! 「在草稿本新建」本身（`winctl::open_scratchbook`）和菜单那条路是同一个函数，那条是测过的。

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send, sel, MainThreadMarker};
use objc2_app_kit::NSApplication;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();
/// 建好的那份菜单。进程活多久它活多久（`Retained::into_raw`，不释放）
static MENU: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

/// 装上。要在主线程、应用代理建好之后调（`setup` 里，同 `terminate::install`）
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let Some(mtm) = MainThreadMarker::new() else {
        applog::write(applog::Level::Warn, "dock", "装 Dock 菜单不在主线程上，没装");
        return;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        applog::write(applog::Level::Warn, "dock", "没有应用代理，Dock 右键菜单没装");
        return;
    };
    let obj: &AnyObject = delegate.as_ref();
    let cls = obj.class();
    // SAFETY: 两个函数的签名和类型串一致 —— applicationDockMenu: 返回 id（@），liteScratchbook: 返回 void（v）；参数都是 self、_cmd、sender
    let (menu_ok, action_ok) = unsafe {
        let dock: Imp = std::mem::transmute(dock_menu as unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> *mut AnyObject);
        let act: Imp = std::mem::transmute(scratchbook as unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject));
        let c = cls as *const AnyClass as *mut AnyClass;
        (
            objc2::ffi::class_addMethod(c, sel!(applicationDockMenu:), dock, c"@@:@".as_ptr()).as_bool(),
            objc2::ffi::class_addMethod(c, sel!(liteScratchbook:), act, c"v@:@".as_ptr()).as_bool(),
        )
    };
    if !(menu_ok && action_ok) {
        applog::write(
            applog::Level::Warn,
            "dock",
            &format!("{} 已经有 applicationDockMenu: / liteScratchbook:（tao 升级了？），没覆盖 —— Dock 右键「在草稿本新建」要重新验", cls.name().to_string_lossy()),
        );
        return;
    }
    // SAFETY: 主线程上；发的都是 NSMenu / NSMenuItem 的公开方法，参数类型和它们的声明一致
    let menu = unsafe { build(obj) };
    // 回读一遍：几项、第一项叫什么。测试身份的 .app 不进 Dock、这个菜单永远不会被要，这一行是它「真建出来了」的唯一证据
    // （release 构建里 msg_send! 不核对类型，发错了是空菜单而不是报错）
    let (n, first) = unsafe { readback(&menu) };
    MENU.store(Retained::into_raw(menu), Ordering::Release);
    crate::diag!("Dock 菜单装在 {} 上：{n} 项，第一项「{first}」", cls.name().to_string_lossy());
    if n != 1 || first != "在草稿本新建" {
        applog::write(applog::Level::Warn, "dock", &format!("Dock 菜单建出来不对：{n} 项，第一项「{first}」"));
    }
}

unsafe fn readback(menu: &AnyObject) -> (isize, String) {
    let n: isize = msg_send![menu, numberOfItems];
    if n < 1 {
        return (n, String::new());
    }
    let item: *mut AnyObject = msg_send![menu, itemAtIndex: 0isize];
    let title: *mut AnyObject = msg_send![item, title];
    let cs: *const std::ffi::c_char = msg_send![title, UTF8String];
    (n, if cs.is_null() { String::new() } else { std::ffi::CStr::from_ptr(cs).to_string_lossy().into_owned() })
}

/// 一项的菜单：「在草稿本新建」→ 代理的 `liteScratchbook:`
unsafe fn build(target: &AnyObject) -> Retained<AnyObject> {
    let title = ns_string(c"在草稿本新建");
    let empty = ns_string(c"");
    let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
    let alloc: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
    let item: Retained<AnyObject> = msg_send![alloc, initWithTitle: &*title, action: sel!(liteScratchbook:), keyEquivalent: &*empty];
    let _: () = msg_send![&*item, setTarget: target];
    let _: () = msg_send![&*menu, addItem: &*item];
    menu
}

/// UTF-8 → NSString（`stringWithUTF8String:` 返回的是自动释放的，`Retained` 收的时候会 retain 一次）
unsafe fn ns_string(s: &std::ffi::CStr) -> Retained<AnyObject> {
    msg_send![class!(NSString), stringWithUTF8String: s.as_ptr()]
}

unsafe extern "C-unwind" fn dock_menu(_: &AnyObject, _: Sel, _: *mut AnyObject) -> *mut AnyObject {
    // +0 返回：菜单的所有权一直在 MENU 那儿
    MENU.load(Ordering::Acquire)
}

unsafe extern "C-unwind" fn scratchbook(_: &AnyObject, _: Sel, _: *mut AnyObject) {
    if let Some(app) = APP.get() {
        crate::winctl::open_scratchbook(app);
    }
}
