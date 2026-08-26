// 防止 Windows release 下弹出多余控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    lite_ide_lib::run()
}
