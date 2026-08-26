fn main() {
    // 前端产物变了也要重新嵌入。没有这一行时，只改前端再跑 `cargo build`
    // 会因为 Rust 代码没动而整个跳过编译，二进制里留着上一版的 dist ——
    // 表现为「改了界面却毫无变化」，极难察觉。
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
