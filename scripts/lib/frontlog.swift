// 记下「哪个应用变成了前台」：每次一行，`<毫秒时间戳>\t<bundle id>\t<名字>\t<pid>`，写到 stdout。
//
// 给 smoke ⑳（#54）取证用：每段开头采样「谁在最前」只能知道「到这段开头时已经被抢了」，看不到是哪一刻、被谁拿走的；
// 系统的激活通知直接给出时间和应用。**只读**：订阅 NSWorkspace 的通知，不发按键、不读 AX、不截图，碰不到你正在用的应用。
//
// 起的人（smoke）没了就自己退：每秒看一眼父进程是不是变成了 launchd（pid 1），不留一个常驻进程。
import AppKit

setvbuf(stdout, nil, _IOLBF, 0)

func line(_ app: NSRunningApplication?) {
    let ms = Int64(Date().timeIntervalSince1970 * 1000)
    print("\(ms)\t\(app?.bundleIdentifier ?? "?")\t\(app?.localizedName ?? "?")\t\(app?.processIdentifier ?? 0)")
}

// 起来那一刻谁在最前，先记一行：之后每一行都是「从上一行那个切到这一个」
line(NSWorkspace.shared.frontmostApplication)

NSWorkspace.shared.notificationCenter.addObserver(
    forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
) { n in
    line(n.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication)
}

Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
    if getppid() == 1 { exit(0) }
}

RunLoop.main.run()
