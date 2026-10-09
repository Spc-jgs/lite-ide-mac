//! 终端：起、写、改尺寸、背压确认、杀。

use super::*;

/// 前端报「这批字节我已经吃下去了」，把背压的水位降下来（issue #18 第一条）。
///
/// **在 `term.write(bytes, cb)` 的回调里叫**，不是收到就叫 —— 那个回调
/// 在 xterm 真的解析完之后才响，而要限的正是「还没被消费的写队列」。
/// 收到就叫等于没有背压。
///
/// 找不到这个 id 就静默返回：终端刚被关掉时，最后几条 ack 一定是打空的，
/// 那不是错误。
#[tauri::command]
pub fn pty_ack(id: u32, bytes: u32, state: State<'_, AppState>) {
    if let Some(flow) = state.pty_flow(id) {
        flow.acked(bytes as u64);
    }
}

/// 起一个终端。输出走 Channel 流式回传，不经 JSON 数组。
///
/// 用真 pty 而不是模拟 shell —— vim / less / gradle 进度条全靠 pty 的行为。
#[tauri::command]
pub fn pty_spawn(
    cwd: String,
    cols: u16,
    rows: u16,
    on_data: tauri::ipc::Channel<Vec<u8>>,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    use std::io::Read;

    // 用哪个 shell 归设置管（issue #44 的 terminal.shell）；空串 = $SHELL
    let shell = crate::settingsctl::terminal_shell(&state);
    let (sess, mut reader) =
        ptysvc::Session::spawn_with(&shell, &cwd, cols, rows).map_err(|e| format!("终端起不来：{e}"))?;
    // 满了就拒绝。`sess` 在这儿 drop 掉 —— Session::drop 会 kill 那个 zsh，
    // 所以刚起的这个不会变成孤儿（UNINSTALL.md 的承诺）
    // 记在发起调用的窗口名下：那个窗口关掉时只杀它自己的终端
    let (id, flow) = state.insert_pty(window.label(), sess)?;
    crate::diag!("pty_spawn id={id} cwd={cwd}");

    std::thread::Builder::new()
        .name(format!("pty-read-{id}"))
        .spawn(move || {
            let mut buf = [0u8; 8192];
            let mut gave_up = false;
            loop {
                /*
                 * **读之前先问闸**（issue #18 第一条）。
                 *
                 * 未确认的字节数到水位就在这儿停住，让 pty master 的缓冲区
                 * 自己把 shell 顶回去 —— 那正是真终端里 `cat` 大文件
                 * 不会撑爆内存的原因。判据、水位和「前端不回话怎么办」
                 * 全在 `ptysvc::Flow` 里，这儿只是叫一下。
                 */
                if !flow.wait_room() {
                    break;
                }
                /*
                 * 闸放弃了 —— 前端两秒没回一个 ack。
                 *
                 * **这一行是前端那半唯一的自证方式。** 背压能不能成立，
                 * 取决于 `Terminal.svelte` 在 `term.write` 的回调里有没有
                 * 报回来；那条通道断掉的表现是「什么都没变，只是队列又无界了」——
                 * 不说一声的话，没有任何人会发现。
                 *
                 * 只说一次（`trusted` 一旦关掉就不会再打开），所以不会刷屏。
                 */
                if !flow.trusted() && !gave_up {
                    gave_up = true;
                    crate::diag!("pty {id} 两秒没等到 ack，背压关掉了");
                    applog::write(
                        applog::Level::Warn,
                        "pty",
                        &format!("终端 {id} 的 ack 通道没回话，背压已关闭 —— 输出队列回到无上限"),
                    );
                }
                match reader.read(&mut buf) {
                    // EOF：shell 退出了
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        /*
                         * 前端已经关掉这个终端时 send 会失败，正常收摊。
                         *
                         * **但这一 break 是有代价的**：这里退出之后就再没人排空
                         * pty master，而紧接着到来的正是 pty_kill。退出中的 shell
                         * 写满缓冲区就卡在写上收不了尾，`child.wait()` 于是永远
                         * 等不到 —— M23 那次界面永久卡死就是这么来的（issue #2）。
                         *
                         * 现在不挂住，靠的是 `ptysvc::Session::kill()` 在杀之前
                         * 自己接了一条临时排空线程。**改那边之前先看这里** ——
                         * 回归测试在 ptysvc 里，动这个 break 的人不一定会跑到。
                         */
                        if on_data.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        flow.sent(n as u64);
                    }
                }
            }
        })
        .map_err(|e| format!("读线程起不来：{e}"))?;

    Ok(id)
}

#[tauri::command]
pub fn pty_write(id: u32, data: String, state: State<'_, AppState>) -> Result<(), String> {
    let sess = state.pty(id).ok_or("终端已关闭")?;
    // 先落成局部变量，让 MutexGuard 在本语句结束时就释放；
    // 直接把链式表达式当返回值会让 guard 活过 sess，借用检查不过
    let r = sess
        .lock()
        .expect("pty 锁被毒化")
        .write_input(data.as_bytes());
    r.map_err(|e| format!("写入失败：{e}"))
}

#[tauri::command]
pub fn pty_resize(id: u32, cols: u16, rows: u16, state: State<'_, AppState>) -> Result<(), String> {
    let sess = state.pty(id).ok_or("终端已关闭")?;
    let r = sess.lock().expect("pty 锁被毒化").resize(cols, rows);
    r.map_err(|e| format!("调整尺寸失败：{e}"))
}

#[tauri::command]
pub fn pty_kill(id: u32, state: State<'_, AppState>) -> bool {
    crate::diag!("pty_kill id={id}");
    state.kill_pty(id)
}
