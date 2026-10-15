//! 语言服务器客户端（docs/IDEA.md 第 2 步）。三层，各自能单独测：
//!
//! - [`conn`]：线。JSON-RPC、`Content-Length` 分帧、请求配回话、超时取消、服务器问我们的另一条线程答、单条消息有上限
//! - [`server`]：进程。自己的进程组、stderr 排空只留尾巴、等待线程收尸并报崩溃、先按协议说再见再杀
//! - [`jdtls`]：jdtls 怎么起。找 JDK 21、找 Lombok、拼参数 —— 第 0 步实测定下来的三样参数都在这里
//! - [`install`]：第一次用时把钉死的那一版 jdtls 下进应用数据目录（校验 sha256、原子地装）
//!
//! 通用的那两层不知道 Java：以后接 Python（basedpyright）、TS 走同一套（IDEA.md 第 3 节第 6 条）。

pub mod conn;
pub mod install;
pub mod jdtls;
pub mod server;
pub mod uri;

pub use conn::{Conn, Error};
pub use server::{Exit, Server, Spec};
