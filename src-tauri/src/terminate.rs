//! Dock 右键「退出」、注销、关机也先让每个窗口存好现场再退（#52）。
//!
//! ⌘Q 走我们自己的菜单项（`winctl::quit`：先 `flush`、回齐再退）。另外三条路不经过菜单：AppKit 直接
//! `terminate:`，先问应用代理 `applicationShouldTerminate:`，没人答就当场退，前端一点机会都没有 ——
//! 10-08 实测 `quit` Apple Event（和 Dock 右键同一条路）3 次全丢最后几秒的输入。
//!
//! 应用代理是 tao 建的，那个类上**没有**这个方法，Tauri 也没开放（tao 0.35.3 的 `app_delegate.rs`）。
//! 所以启动时在运行时给它补一个：
//!
//! - **类从活着的代理对象上取**（`object_getClass`），不写死 tao 内部的类名 —— 类名改了照样装得上；
//! - **装不上就说出来**：哪天 tao 自己实现了它，`class_addMethod` 返回假（不覆盖已有的），往 app.log 写一条 WARN，
//!   不悄悄失效。那时候该做的是去看 tao 的实现，而不是硬换掉它；
//! - 答 **`NSTerminateLater`**，不是 `NSTerminateCancel` 再自己退：Cancel 会把注销 / 关机整个打断（「lite-ide 中断了注销」），
//!   人还得再点一次；Later 是「等我一下」，系统挂着，存完我们 `replyToApplicationShouldTerminate:YES`，注销接着走。
//!
//! 等的这段 AppKit 跑在模态的 run loop 模式里。前端回话要经过 tao 的事件代理，它挂在 `kCFRunLoopCommonModes` 上，
//! 那个模式包含在内 —— 这条靠 `scripts/accept/quit.sh` 在真 .app 上盯着。

use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use std::sync::OnceLock;
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// 装上 `applicationShouldTerminate:`。要在主线程、应用代理建好之后调（`setup` 里）
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let Some(mtm) = MainThreadMarker::new() else {
        applog::write(applog::Level::Warn, "quit", "装退出钩子不在主线程上，没装");
        return;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        applog::write(applog::Level::Warn, "quit", "没有应用代理，退出钩子没装 —— Dock 退出会丢最后几秒的输入");
        return;
    };
    let obj: &AnyObject = delegate.as_ref();
    let cls = obj.class();
    // SAFETY: 函数签名和类型串一致：返回 NSUInteger（Q），参数 self（@）、_cmd（:）、sender（@）
    let added = unsafe {
        let imp: Imp = std::mem::transmute(
            should_terminate as unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> NSApplicationTerminateReply,
        );
        objc2::ffi::class_addMethod(
            cls as *const _ as *mut _,
            sel!(applicationShouldTerminate:),
            imp,
            c"Q@:@".as_ptr(),
        )
    };
    if added.as_bool() {
        crate::diag!("退出钩子装在 {} 上", cls.name().to_string_lossy());
    } else {
        applog::write(
            applog::Level::Warn,
            "quit",
            &format!(
                "{} 已经有 applicationShouldTerminate:（tao 升级了？），没覆盖 —— Dock 退出会不会丢输入要重新验（#52）",
                cls.name().to_string_lossy()
            ),
        );
    }
}

/// 回复「可以退了」。只在主线程上调（`winctl::finish_quit` 经 `run_on_main_thread` 进来）
pub fn reply_now() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(true);
    }
}

unsafe extern "C-unwind" fn should_terminate(_: &AnyObject, _: Sel, _: *mut AnyObject) -> NSApplicationTerminateReply {
    let Some(app) = APP.get() else { return NSApplicationTerminateReply::TerminateNow };
    if crate::winctl::system_quit(app) {
        crate::diag!("系统要退出：先让各窗口存好现场");
        NSApplicationTerminateReply::TerminateLater
    } else {
        // 我们自己的退出已经发出去了（exit 在路上），不用再等
        NSApplicationTerminateReply::TerminateNow
    }
}
