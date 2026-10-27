// 防止 Windows release 下弹出多余控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // git 要凭据时执行的是我们自己（#62，`GIT_ASKPASS` 指到这个可执行文件）：带着这个环境变量起来的，问一句、把答案打到 stdout 就退，不起界面
    if std::env::var_os(gitsvc::askpass::SOCK_ENV).is_some() {
        std::process::exit(gitsvc::askpass::client(&std::env::args().nth(1).unwrap_or_default()));
    }
    lite_ide_lib::run()
}
