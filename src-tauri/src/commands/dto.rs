//! 过 IPC 的全部 DTO，和前端 `src/lib/ipc/commands.ts` 一一对应。
//!
//! **集中在这一个文件是有意的**：`tests/dto_sync.rs` 读这个文件和 `commands.ts` 逐字段比，
//! 两边各一个文件，对照起来也方便。新加 DTO 放这里，并在 `dto_sync.rs` 的 `PAIRS` 里登记。

use super::*;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub handle: u32,
    pub name: String,
    pub size: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatDto {
    pub line_count: u64,
    pub indexed_bytes: u64,
    pub total_bytes: u64,
    pub complete: bool,
    pub index_bytes: u64,
    /// 各级别行数，顺序同 Level：error/warn/info/debug/trace/other
    pub levels: [u64; Level::COUNT],
    pub levels_complete: bool,
    pub levels_scanned: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterStatDto {
    /// 命中条数
    pub hits: u64,
    pub complete: bool,
    pub scanned_lines: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshDto {
    /// "none" | "grew" | "rotated"
    pub kind: &'static str,
    pub new_lines: u64,
    pub line_count: u64,
}

/// ⌘P / ⌘Click 用的文件索引。`truncated`：到了 5 万的上限、后面的没看 ——
/// 界面上「没找到」要说成「索引只看了前五万个」，不能说成「不在项目里」
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFilesDto {
    pub files: Vec<String>,
    pub truncated: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathInfo {
    /// "file" | "dir"
    pub kind: &'static str,
    /// "edit" | "log"，kind == "dir" 时无意义
    pub mode: &'static str,
    pub path: String,
    pub name: String,
    pub size: u64,
    /// 判为 log 模式的原因，用于界面上说明「为什么这个文件是只读的」
    pub reason: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// 名字命中生成物名单（`node_modules` `target` `dist` …）。
    /// **它在树里、点得开** —— 界面只是把它压暗、不预取。见 `fsservice::list_dir`。
    pub generated: bool,
    /// 这个名字有第二种可能（`dist` / `build` / `vendor`），
    /// 前端要拿 [`ignored_dirs`] 的答案对一遍才决定压不压暗。
    pub contested: bool,
    /// 往下「只有一个子目录」的那一串名字，不含自己。显示规则在前端 `tree-rows.ts`，见 `fsservice::Entry::chain`
    pub chain: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDto {
    pub content: String,
    /// WHATWG 编码标签，如 `UTF-8` / `GBK`
    pub encoding: String,
    pub bom: bool,
    /// 有解不出的字节 —— 界面必须把这件事说出来，
    /// 带着它保存等于把那些字节永久换成 U+FFFD
    pub lossy: bool,
    /// 盘上的换行符：`LF` / `CRLF` / `CR` / `mixed`。内容已统一成 \n，
    /// 保存时把它传回 `write_text` 才能原样写回（fsservice::eol）
    pub eol: String,
}

#[derive(serde::Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct StampDto {
    pub mtime_ms: u64,
    pub size: u64,
}

/// 草稿的锚点（M10）：在哪个项目 / 分支 / 提交 / 文件行写的。四样都可为空。
/// 两个方向都走它：新建时前端传进来，列表时 Rust 从文件头解出来。
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AnchorDto {
    pub project: String,
    pub branch: String,
    pub head: String,
    /// `相对路径:行`
    pub at: String,
}

/// 草稿列表里的一条（issue #40）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchDto {
    pub name: String,
    pub path: String,
    pub mtime_ms: u64,
    pub first_line: String,
    /// 文件头里的锚点；没有头就是 None
    pub anchor: Option<AnchorDto>,
}

/// 「安装命令行工具…」的结果（issue #40）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallDto {
    /// 脚本真身的路径（应用数据目录下的 `bin/lite`）
    pub script: String,
    /// `/usr/local/bin/lite` 装上了没。没装上时前端把 `link_cmd` 摆出来让人自己跑
    pub linked: bool,
    pub replaced: bool,
    /// 手动补上软链的那一句
    pub link_cmd: String,
}

/// Git 控制台里的一条（issue #29）。
///
/// 时间只给 Unix 毫秒，**不在 Rust 侧格式化** —— 那要知道时区，
/// 而前端本来就有 `Date`，让它按用户的本地时区显示。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCmdDto {
    pub ms: u64,
    pub cwd: String,
    pub argv: Vec<String>,
    /// `null` = 没跑起来（git 不在），或者被我们主动掐掉了
    pub code: Option<i32>,
    pub dur_ms: u32,
    pub err: String,
    pub err_truncated: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitDto {
    pub path: String,
    pub line: u64,
    pub text: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitEntryDto {
    pub path: String,
    /// 暂存区状态字符
    pub index: String,
    /// 工作区状态字符
    pub work: String,
    pub untracked: bool,
    pub conflicted: bool,
    pub staged: bool,
    pub unstaged: bool,
    pub orig: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusDto {
    /// 仓库根的绝对路径。前端拿它把相对路径拼成绝对路径去对文件树
    pub root: String,
    pub branch: String,
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    pub detached: bool,
    /// HEAD 短 sha，空仓库是空串
    pub head: String,
    pub unborn: bool,
    /// 只有文件；整个未跟踪的目录在 `untracked_dirs` 里
    pub entries: Vec<GitEntryDto>,
    /// 整个未跟踪的目录（路径以 `/` 结尾）。文件树按前缀匹配用
    pub untracked_dirs: Vec<String>,
    pub truncated: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffDto {
    pub text: String,
    /// 超过 `gitsvc::MAX_DIFF_BYTES` 被掐断了。界面必须把这件事说出来 ——
    /// 一份看着完整、其实少了后半截的差异，比一句「显示不下」危险得多
    pub truncated: bool,
}

impl From<gitsvc::Diff> for DiffDto {
    fn from(d: gitsvc::Diff) -> Self {
        Self {
            text: d.text,
            truncated: d.truncated,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntryDto {
    pub sha: String,
    pub short: String,
    pub author: String,
    pub email: String,
    pub when: String,
    pub date: String,
    pub subject: String,
    /// 父提交完整 sha；合并提交有多个，泳道图靠它连线
    pub parents: Vec<String>,
    pub refs: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDto {
    pub name: String,
    pub sha: String,
    pub upstream: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub when: String,
    pub subject: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDto {
    pub path: String,
    pub sha: String,
    pub branch: String,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub current: bool,
}

/// blame 的一段（issue #33 ⑭）。`sha` 全零 = 未提交的行
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameHunkDto {
    pub sha: String,
    pub short: String,
    pub author: String,
    /// 作者时间，unix 秒
    pub time: i64,
    pub summary: String,
    /// 现文件里的起始行（1-based）
    pub start: u32,
    pub count: u32,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameDto {
    pub hunks: Vec<BlameHunkDto>,
    /// 输出被 1MB 上限截断了：后面的行没有注解，界面要说出来
    pub truncated: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashDto {
    pub index: u32,
    pub message: String,
}

/// 切分支失败时给前端的东西。
///
/// **不是一个字符串。** 「本地改动会被覆盖」这一档要带上挡路的文件名，
/// 界面才给得出「去提交 / 丢弃这些改动」两个按钮 —— 而不是把 git 那句
/// "Please commit your changes or stash them" 原样贴给用户看。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchErrDto {
    /// `local-changes` / `other`
    pub kind: String,
    pub message: String,
    /// 挡路的文件，只有 `kind == "local-changes"` 时非空
    pub files: Vec<String>,
    /// git 的原话。界面上「看 git 怎么说」那种展开要用
    pub raw: String,
}

/// 删分支失败时给前端的东西。和 [`SwitchErrDto`] 同一个思路：
/// 「还有没合并的提交」不是出错，是要你决定 —— 界面上给「仍然删除」那条路。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchErrDto {
    /// `not-merged` / `other`
    pub kind: String,
    pub message: String,
    /// git 的原话
    pub raw: String,
}

/// 一条白名单之外的 `.git/config` 键，给确认卡片列出来看的
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustSuspectDto {
    pub key: String,
    pub value: String,
    /// `file:.git/config` 那种；被 `include` 进来的指向别的文件
    pub origin: String,
}

/// 开仓库前的信任扫描结果。`trusted` = 没有可疑项，或者用户对**这一份** config 信任过。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustScanDto {
    pub root: String,
    pub trusted: bool,
    pub suspects: Vec<TrustSuspectDto>,
    /// `.git/hooks` 里会执行的钩子名。只列出来知情，不影响 `trusted`
    pub hooks: Vec<String>,
    /// 信任按它记；前端原样传回 `git_trust_grant`
    pub fingerprint: String,
}

/// 一条进度，推给前端的形状。
///
/// `percent` / `done` 可能是 None —— git 的进度文案不是稳定接口，
/// 认不出来时 `phase` 里是整段原文，界面显示成一行状态而不是进度条。
/// **绝不能因为解析不出来就把一次成功的操作报成失败。**
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub phase: String,
    pub percent: Option<u8>,
    pub done: Option<u64>,
    pub total: Option<u64>,
    pub finished: bool,
}

/// 远程操作失败时给前端的东西。
///
/// `kind` 决定界面显示什么（认证提示 / 先拉一下 / 去解冲突），
/// `raw` 是 git 的原话 —— **必须留着能展开看**：转译错了的时候，
/// 人得有办法绕过我们。和差异视图的 `truncated` 是同一条判据。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteErrDto {
    /// `auth-https` / `auth-ssh` / `cancelled` / `rejected` / `conflict` / `other`
    pub kind: String,
    pub message: String,
    pub raw: String,
}

/// 「最近打开」的项目（多窗口第 4 步起名单在 Rust，`windows.rs`）。
///
/// `open` 是开着的窗口各自的项目根：前端清理别的项目的会话快照时，这些一律留着 ——
/// 开着十几个窗口时它们不一定都还在「最近打开」那 8 个里。
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentDto {
    pub projects: Vec<String>,
    pub open: Vec<String>,
}
