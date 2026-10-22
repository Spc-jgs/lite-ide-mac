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

/// Maven 工具窗（照 IDEA 的，2026-10-15 重做）：读 pom 就有的那部分，打开工具窗立刻给。插件和依赖要跑 mvn，另外要（`MavenResolvedDto`）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenProjectDto {
    pub reactors: Vec<MavenReactorDto>,
    /// Lifecycle 底下的阶段（IDEA 默认显示的 9 个）
    pub phases: Vec<String>,
    /// 所有模块里声明的 profile（同名的合成一个），Profiles 节点
    pub profiles: Vec<MavenProfileDto>,
    /// 工具栏的两个开关
    pub offline: bool,
    pub skip_tests: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenReactorDto {
    /// 相对项目根（项目根就是 reactor 根时是 ""）
    pub rel: String,
    /// 模块，根模块在第一个，按 pom 里的先后
    pub modules: Vec<MavenModuleDto>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenModuleDto {
    /// 相对 reactor 根
    pub rel: String,
    pub artifact_id: String,
    /// 把它列进 `<modules>` 的那个模块（相对 reactor 根）；根模块是 null。工具窗照 IDEA 把子模块挂在它底下
    pub parent: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenProfileDto {
    pub id: String,
    /// pom 里写了 `<activeByDefault>true`
    pub active_by_default: bool,
    /// 勾上了（跑目标、起服务都带 `-P`）
    pub on: bool,
}

/// 跑一次 mvn 算出来的：每个模块真用的插件和依赖树（`maven_run.rs`）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenResolvedDto {
    pub reactors: Vec<MavenResolvedReactorDto>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenResolvedReactorDto {
    pub rel: String,
    pub modules: Vec<MavenResolvedModuleDto>,
    /// mvn 失败了：报错那几行（空 = 成功）
    pub error: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenResolvedModuleDto {
    pub artifact_id: String,
    pub plugins: Vec<MavenPluginDto>,
    pub deps: Vec<MavenDepDto>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenPluginDto {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    /// `spring-boot`：目标写成 `spring-boot:run`
    pub prefix: String,
    /// 插件 jar 里描述的目标；本地仓库里没有 jar 时是空的
    pub goals: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenDepDto {
    /// 0 = 直接依赖
    pub depth: u32,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub scope: String,
    pub optional: bool,
}

/// 合并视图里的一份日志（图例上画一格）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeSourceDto {
    pub path: String,
    pub name: String,
    /// `full` / `noYear` / `timeOnly` / `none`：这份的时间戳带不带日期 —— 不带的借了别人的，界面要说出来
    pub clock: &'static str,
    /// 手动的时钟偏移（毫秒）
    pub offset_ms: i64,
}

/// 打开（或改了偏移之后）的合并视图
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeInfoDto {
    pub handle: u32,
    pub sources: Vec<MergeSourceDto>,
    /// 缺日期的那几份借的是哪一天（`2026-08-24`）；没有一份带年月日时为空
    pub fill_date: Option<String>,
    /// 几份加起来的字节数
    pub size: u64,
}

/// 一段合并后的行各自来自哪份、是那份的第几行（0-based）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRowsDto {
    pub src: Vec<u8>,
    pub line: Vec<u64>,
}

/// 日志归类里的一类
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternDto {
    /// 模板的原始字节（变量位置是 `<*>`）。不在 Rust 侧转码：文件可能是 GBK，前端拿标签的编码解（同日志行）
    pub template: Vec<u8>,
    pub count: u64,
    /// 对比时基准那边出现了几次；不对比时是 0
    pub base: u64,
    /// 各级别各几行，顺序同 `LogStat.levels`
    pub levels: [u64; 6],
}

/// 日志归类的进度和结果（`logengine::pattern`）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternsDto {
    /// 次数最多的前若干类
    pub patterns: Vec<PatternDto>,
    /// 一共分出几类（可能比 `patterns` 多）
    pub total: u64,
    /// 归了多少行（带时间戳的文件只算记录头）
    pub lines: u64,
    /// 对比时基准那边归了多少行；不对比时是 0
    pub base_lines: u64,
    /// 类数到了上限之后没归进去的行
    pub overflow: u64,
    pub scanned_bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
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
    /// 手动补上软链的那一句；那个位置被别人占着时是空串（不给命令 —— 原来给的 `ln -sf` 会把它盖掉）
    pub link_cmd: String,
    /// `/usr/local/bin/lite` 已经是别人的东西：说它是什么。没被占是 None
    pub taken: Option<String>,
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
    /// 命中在 `text` 里的哪几段（UTF-16 下标）。前端照这个高亮，不自己照着搜索词再找一遍（#42）
    pub spans: Vec<[u32; 2]>,
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
/// `keep` 是开着的窗口各自的项目根 + 最近关掉、点 Dock 还会开回来的那几个
/// （`Windows::keep_roots`）：前端清理会话快照时这些一律留着 —— 它们不一定都还在
/// 「最近打开」那 8 个里。
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentDto {
    pub projects: Vec<String>,
    pub keep: Vec<String>,
}

/// 生效的设置（issue #44，docs/SETTINGS.md）：`settings.json` 和 `ui-state.json` 合起来、空值换成实际用的
/// （`settings::effective`）。前端拿到直接用，不再自己合并 —— 多个窗口各算一遍迟早有一个算得不一样。
/// 变了的时候 Rust 广播 `settings-changed`，负载就是这个，全量
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub editor_font_family: String,
    /// 实际字号 = 基础 + ⌘= 的偏移
    pub editor_font_size: i64,
    /// `settings.json` 里写的基础字号
    pub editor_font_base: i64,
    /// 已经换算过：没写就是编辑器那个
    pub terminal_font_family: String,
    pub terminal_font_size: i64,
    /// 空 = `$SHELL`
    pub terminal_shell: String,
    pub minimap: bool,
    pub tree_compact: bool,
    pub tree_follow: bool,
    pub git_grouped: bool,
    /// 读 `settings.json` 时发现的问题（SETTINGS.md 第 6 节：每一种都要告诉人）
    pub problems: Vec<SettingProblemDto>,
    /// `settings.json` 的绝对路径（状态栏的提示点了要打开它）
    pub path: String,
}

/// 设置文件里的一个问题。`line` / `col` 从 1 数，**`col` 按 UTF-16 数**（CM6 的位置就是 JS 字符串下标，拿到就能用）
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingProblemDto {
    pub text: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
    /// 整份文件用不了（语法错）：这时生效的是上一份好的，或者默认值
    pub fatal: bool,
}

/// 一个设置项的定义（`settings::DEFS`），给编辑 `settings.json` 时的补全用
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingDefDto {
    pub key: String,
    /// 设置页上那一项的名字
    pub label: String,
    /// `"string"` / `"integer"` / `"boolean"` / `"array"`（字符串列表）
    pub kind: String,
    /// 默认值的 JSON 字面量，原样可以插进文件
    pub default: String,
    pub doc: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
}

// ── 跨文件替换（#42）。字段说明见 replacesvc，这里只是过 IPC 的形状 ──

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceHitDto {
    pub line: u64,
    /// UTF-16，从 1 起
    pub col: u32,
    pub text: String,
    pub spans: Vec<[u32; 2]>,
    /// 跨了几行（1 = 不跨行）
    pub lines: u32,
    /// 跨行时改前那几行（超过 200 行中间折叠）
    pub block: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceFileDto {
    pub rel: String,
    /// 绝对路径：前端按它找开着的标签
    pub path: String,
    /// 用的是编辑器里那份（开着、有未保存改动）
    pub editor: bool,
    pub hits: Vec<ReplaceHitDto>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceSkipDto {
    pub rel: String,
    pub why: String,
    /// 给人看的那句话（replacesvc::Why::text）
    pub text: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceScanDto {
    pub files: Vec<ReplaceFileDto>,
    pub skipped: Vec<ReplaceSkipDto>,
    pub binary: usize,
    pub total: usize,
    pub truncated: bool,
    pub index_truncated: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceAfterDto {
    pub text: String,
    pub spans: Vec<[u32; 2]>,
    /// 有一边跨行时改后那几行
    pub block: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceEditDto {
    pub from: u32,
    pub to: u32,
    pub insert: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceChangedDto {
    pub rel: String,
    pub path: String,
    pub count: usize,
    pub edits: Vec<ReplaceEditDto>,
    pub on_disk: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceOutcomeDto {
    pub changed: Vec<ReplaceChangedDto>,
    pub skipped: Vec<ReplaceSkipDto>,
    pub undo: bool,
    /// 改了名的文件（语言服务器的改名改过去的、撤销改回来的）：开着它的标签要跟着挪到新路径
    pub renamed: Vec<ReplaceRenamedDto>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRenamedDto {
    pub from: String,
    pub to: String,
}

/// 执行失败。`code`：`truncated`（命中太多）/ `pending`（上次中断还没收拾）/ `too-big`（要点头才做，`bytes` 是多大）/ `failed`
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceErrorDto {
    pub code: String,
    pub message: String,
    pub bytes: u64,
}

/// 上次留下的替换日志。`state`：`preparing` / `committing` / `done`
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePendingDto {
    pub state: String,
    pub root: String,
    pub at_ms: u64,
    pub files: usize,
    pub changed: usize,
}

// ── 任务（#48，docs/TASKS.md） ──

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefDto {
    pub name: String,
    /// 一整行命令（列表里给人看，悬停时整行显示）
    pub command: String,
    /// 相对项目根；"" = 根
    pub cwd: String,
    /// "file"（`.lite-ide/tasks.json`）/ "package"（自动认出来的 package.json scripts）
    pub source: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListDto {
    pub tasks: Vec<TaskDefDto>,
    /// `tasks.json` 在不在：列表底部给「打开」还是「新建」
    pub file: bool,
    /// 读的时候发现的问题，一条一句话（写错的那个任务跳过了、哪一行语法错）
    pub problems: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunDto {
    /// 这一次运行的 id（停、关都拿它）
    pub id: u32,
    pub name: String,
    pub command: String,
    /// 输出写在哪个文件：前端拿去开日志句柄
    pub log: String,
    /// Spring Boot 服务（Services 工具窗里跑，不进运行窗；同 IDEA）
    pub service: bool,
}

/// Services 工具窗里的一个 Spring Boot 服务（认出来的入口；跑没跑、跑成什么样前端从运行记录里看）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDto {
    /// 任务名（和任务列表、运行记录里的一样）
    pub task: String,
    pub main_class: String,
    /// 所在模块，相对项目根
    pub module: String,
    /// 在跑的话它（整个进程组）在听哪几个 TCP 端口；没跑或者还没起来是空的
    pub ports: Vec<u16>,
}

/// 「任务退出了」事件（`task-exit`，只发给起它的窗口）
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskExitDto {
    pub id: u32,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    /// 是我们停的（不算失败，见 tasksvc::State::Exited）
    pub stopped: bool,
    pub failed: bool,
    /// 失败是因为端口被占、而且查得到是谁占着：卡片上摆出来（TASKS.md Q6）
    pub port: Option<PortHolderDto>,
}

/// 一个跑着的 JVM 此刻的堆（docs/IDEA.md 第 27 节），字节
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskHeapDto {
    pub id: u32,
    pub used: u64,
    pub committed: u64,
    pub max: u64,
    pub gc_count: u64,
    pub gc_ms: u64,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PortHolderDto {
    pub port: u16,
    pub pid: i32,
    /// 进程名（lsof 给的）
    pub command: String,
    /// 是我们起过的哪个任务（这次的或上次没停干净的）；不是就 None
    pub ours: Option<String>,
}

/// 上次没停干净的任务（#48 第 4 步）：应用崩了，它还在自己的进程组里跑着
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStaleDto {
    pub pgid: i32,
    pub name: String,
    pub command: String,
}

/// Java 智能的状态（docs/IDEA.md 第 2 步）：状态栏那格。`phase`：starting / importing / ready / failed / stopped
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JavaStatusDto {
    pub root: String,
    pub phase: String,
    /// 导入中：jdtls 说的进度那句；出错：原因（缺 JDK 21 / 缺 jdtls / 崩了带 stderr 最后一句）
    pub message: String,
    /// 用的哪个 JDK、Lombok 挂没挂上
    pub detail: String,
}

impl From<&crate::java::Status> for JavaStatusDto {
    fn from(s: &crate::java::Status) -> Self {
        use crate::java::Phase;
        let (phase, message) = match &s.phase {
            Phase::Starting => ("starting", String::new()),
            Phase::Importing(m) => ("importing", m.clone()),
            Phase::Ready => ("ready", String::new()),
            Phase::Failed(m) => ("failed", m.clone()),
            Phase::Stopped => ("stopped", String::new()),
        };
        JavaStatusDto { root: s.root.clone(), phase: phase.into(), message, detail: s.detail.clone() }
    }
}

/// 一个文件的诊断（`java-diagnostics` 事件）。`total` 是真条数，超过上限时 `items` 只有前一截（先错误后警告）
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JavaDiagnosticsDto {
    pub root: String,
    pub path: String,
    pub total: u32,
    pub items: Vec<JavaDiagnosticDto>,
}

/// 行、列都从 0 数，列按 UTF-16 数（LSP 的默认，和 CM6 的字符串下标一致）
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JavaDiagnosticDto {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    /// 1 错 2 警告 3 信息 4 提示
    pub severity: u8,
    pub message: String,
}

impl From<crate::java::Diagnostic> for JavaDiagnosticDto {
    fn from(d: crate::java::Diagnostic) -> Self {
        JavaDiagnosticDto { line: d.line, character: d.character, end_line: d.end_line, end_character: d.end_character, severity: d.severity, message: d.message }
    }
}

/// 跳转 / 查用法的一处。`path` 为空 = jar 里的类：`uri` 是 `jdt://…`，内容用 `java_class_contents` 要，`label` 是类的全名
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JavaLocationDto {
    pub uri: String,
    pub path: Option<String>,
    pub label: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    /// 那一行的文字（查用法的列表里给人看；跳转不带，空串）
    pub text: String,
}

impl From<crate::java::Location> for JavaLocationDto {
    fn from(l: crate::java::Location) -> Self {
        JavaLocationDto {
            uri: l.uri,
            path: l.path,
            label: l.label,
            line: l.line,
            character: l.character,
            end_line: l.end_line,
            end_character: l.end_character,
            text: String::new(),
        }
    }
}

/// 一处改动（补全要插的、自动 import 要加的）。行列从 0 数、列按 UTF-16
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaEditDto {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub text: String,
}

impl From<crate::java::Edit> for JavaEditDto {
    fn from(e: crate::java::Edit) -> Self {
        JavaEditDto { line: e.line, character: e.character, end_line: e.end_line, end_character: e.end_character, text: e.text }
    }
}

/// 光标底下能改名的那一段（prepareRename）。`placeholder` 为空就取这一段的原文
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRenameTargetDto {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub placeholder: Option<String>,
}

/// 一条单测的结果（surefire 报告里的一个 testcase）。`outcome`：passed / failed / error / skipped
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseDto {
    pub class: String,
    pub name: String,
    pub ms: u64,
    pub outcome: String,
    pub message: String,
    pub detail: String,
}

/// 一次跑单测的结果。`total` 是真条数（比 `cases` 多就是截了）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResultsDto {
    pub cases: Vec<TestCaseDto>,
    pub total: u32,
}

/// Spring 的一个接口（docs/IDEA.md 第 25 节）。`methods` 是 GET / POST / …（`@RequestMapping` 没写 method 的是 ANY）；`file` 绝对路径，`line` 是方法名那一行
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpringEndpointDto {
    pub methods: Vec<String>,
    pub path: String,
    pub controller: String,
    pub method: String,
    pub file: String,
    pub line: u32,
}

/// 一个配置键在哪：哪份配置文件（`name` 是文件名，看 profile 用）、第几行、那一行写的什么
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpringConfigHitDto {
    pub path: String,
    pub name: String,
    pub line: u32,
    /// 那一行的文字（好几处时列表里给人看：`greeting: hello`）
    pub text: String,
}

/// MyBatis 跳转（docs/IDEA.md 第 24 节）的一个点：接口的方法，或者 XML 里的一段语句。`kind` 是 select / insert / update / delete（方法是空的）；
/// `path` 是它在哪个文件（「这个文件里的」那些是空的）；`line` 从 1 数
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MybatisSpotDto {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
}

/// 打开的这个文件（Java 接口 / Mapper XML）里的点，和对面那些点。前端按名字配对：两边都有的才画图标
#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MybatisLinksDto {
    pub here: Vec<MybatisSpotDto>,
    pub there: Vec<MybatisSpotDto>,
}

/// 构建（⌘F9）认出来的一条编译错误 / 警告。`message` 第一行是 javac 的话，后面是补充（符号、位置）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProblemDto {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub error: bool,
    pub message: String,
}

/// 一次构建的编译错误。`total` 比 `items` 多 = 截了
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProblemsDto {
    pub items: Vec<BuildProblemDto>,
    pub total: u32,
}

/// 一条修复（⌥↵）。`raw` 原样交回 `java_apply_action`
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaActionDto {
    pub title: String,
    pub kind: String,
    pub raw: String,
}

/// 做一条修复的结果：只改当前文件的 → `edits`（前端在编辑器里一笔做完，⌘Z 能撤）；改到别的文件的 → `outcome`（照改名那套落盘，带撤销卡片）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaActionResultDto {
    pub edits: Vec<JavaEditDto>,
    pub outcome: Option<ReplaceOutcomeDto>,
}

/// 一个补全项。插什么看 `edit`（null = 按光标前那个词插 `label`）；`raw` 原样交回 `java_resolve`
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaCompletionItemDto {
    pub label: String,
    pub kind: u32,
    pub detail: String,
    pub description: String,
    pub edit: Option<JavaEditDto>,
    pub snippet: bool,
    pub sort: String,
    pub raw: String,
}

/// 一次补全。`incomplete`：服务器自己截了（或者超过 `java::COMPL_MAX`），接着敲要重新问；`total` 是真项数
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaCompletionDto {
    pub items: Vec<JavaCompletionItemDto>,
    pub incomplete: bool,
    pub total: u32,
}

/// resolve 回来的：要一起做的改动（自动 import）+ 说明文字（纯文本，可能是 markdown 原文）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaResolvedDto {
    pub edits: Vec<JavaEditDto>,
    pub doc: String,
}

/// 查用法的结果。`total` 是真条数；超过 `java::REFS_MAX` 时 `items` 只有前一截，界面要说出来
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRefsDto {
    pub items: Vec<JavaLocationDto>,
    pub total: u32,
}

/// 调试会话的一帧（docs/IDEA.md 第 18 节）。`path` 是源文件、或者 jar 里的类的 `jdt://` 地址，没有源码是空的
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DebugFrameDto {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub label: String,
    pub line: u32,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DebugThreadDto {
    pub id: i64,
    pub name: String,
}

/// 调试会话此刻的样子（"debug-state" 事件和 `debug_state` 命令都是它）。`phase`：waiting / running / paused / ended；
/// `note`：waiting 时在等什么、ended 时为什么结束（正常结束是空的）
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DebugStateDto {
    pub run: u32,
    pub phase: String,
    pub note: String,
    pub thread: Option<i64>,
    pub reason: String,
    pub frames: Vec<DebugFrameDto>,
    pub total_frames: u32,
    pub threads: Vec<DebugThreadDto>,
}

impl DebugStateDto {
    pub fn of(run: u32, s: &crate::debug::Snapshot) -> Self {
        use crate::debug::Phase;
        let (phase, note) = match &s.phase {
            Phase::Waiting(w) => ("waiting", w.clone()),
            Phase::Running => ("running", String::new()),
            // 停着时 note 是插件的提醒（条件算不了照样停下了），平时是空的
            Phase::Paused => ("paused", s.warn.clone()),
            Phase::Ended(w) => ("ended", w.clone()),
        };
        DebugStateDto {
            run,
            phase: phase.into(),
            note,
            thread: s.thread,
            reason: s.reason.clone(),
            frames: s.frames.iter().map(|f| DebugFrameDto { id: f.id, name: f.name.clone(), path: f.path.clone(), label: f.label.clone(), line: f.line }).collect(),
            total_frames: s.total_frames as u32,
            threads: s.threads.iter().map(|t| DebugThreadDto { id: t.id, name: t.name.clone() }).collect(),
        }
    }
}

/// 一个变量（或者一次求值的结果）。`reference` 正数 = 能展开；`indexed` = 数组 / 集合有几个元素
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugVarDto {
    pub name: String,
    pub value: String,
    pub ty: String,
    pub reference: i64,
    pub indexed: u32,
}

impl From<crate::debug::Var> for DebugVarDto {
    fn from(v: crate::debug::Var) -> Self {
        DebugVarDto { name: v.name, value: v.value, ty: v.ty, reference: v.reference, indexed: v.indexed }
    }
}

/// 一层变量。`more`：超过上限没列出来的个数（照实说，不悄悄少一截）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugVarsDto {
    pub vars: Vec<DebugVarDto>,
    pub more: u32,
}

/// 设置对话框要的全部（docs/SETTINGS.md 第 13 节）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPageDto {
    pub defs: Vec<SettingDefDto>,
    /// 每个键现在生效的值，没写的是默认值
    pub values: serde_json::Map<String, serde_json::Value>,
    /// settings.json 里写了的键
    pub written: Vec<String>,
    pub path: String,
    /// settings.json 整份读不懂时为什么（这时设置页不写全局设置，先修好它）
    pub broken: Option<String>,
    pub jdks: Vec<JdkDto>,
    /// 手动加的 JDK 里认不出来的（目录没了、不是 JDK）
    pub bad_jdks: Vec<String>,
    /// 没打开项目时没有
    pub project: Option<ProjectPageDto>,
}

/// 一个 JDK
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JdkDto {
    pub version: u32,
    pub home: String,
    /// 设置里手动加的
    pub manual: bool,
}

/// 设置对话框里「此项目」那一栏
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPageDto {
    pub root: String,
    /// 项目选的 JDK，空 = 按 pom 自动
    pub jdk: String,
    /// 自动挑的话挑哪个（给人看的一句）
    pub auto_jdk: String,
    pub offline: bool,
    pub skip_tests: bool,
    /// 项目根上有 mvnw
    pub wrapper: bool,
}

/// 运行配置对话框要的
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunConfigsDto {
    pub services: Vec<RunConfigDto>,
    pub jdks: Vec<JdkDto>,
    /// IDEA 的 workspace.xml 读不了的（太大、读不出），一条一句
    pub idea_problems: Vec<String>,
}

/// 停不掉的一个进程（#68）：组里活着、但不归我们管
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StuckProcDto {
    pub pid: i32,
    /// 用户名（`root`），查不到是 `uid N`
    pub user: String,
    /// 可执行文件路径
    pub command: String,
}

/// 一个 Spring 服务的运行配置
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunConfigDto {
    pub name: String,
    pub main_class: String,
    pub module: String,
    pub profiles: String,
    pub vm_options: String,
    pub program_args: String,
    pub env: Vec<EnvVarDto>,
    /// 空 = 跟项目
    pub jdk: String,
    /// IDEA 里这个服务的运行配置（`.idea/workspace.xml`，`ideacfg`），对话框里点一下填进表单
    pub idea: Vec<IdeaRunDto>,
}

/// IDEA 里的一份运行配置，转成我们的写法了
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaRunDto {
    /// IDEA 里起的名字
    pub name: String,
    pub profiles: String,
    pub vm_options: String,
    pub program_args: String,
    pub env: Vec<EnvVarDto>,
    pub jdk: String,
    /// 带不过来、或者带过来意思会变的那几处
    pub notes: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EnvVarDto {
    pub name: String,
    pub value: String,
}

/// 「重新加载改过的类」的结果（docs/IDEA.md 第 26 节）。`kind`：nosession / noagent / unchanged / done / failed / noreply
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugHotswapDto {
    pub kind: String,
    /// done：换了的类；failed：这次要换的类
    pub classes: Vec<String>,
    /// failed：为什么（人话）
    pub message: String,
    /// done：断点这次调试不再停的源文件（换进去的类行号表变了）
    pub lost: Vec<String>,
}

impl DebugHotswapDto {
    pub fn of(o: crate::debug::HotOutcome) -> Self {
        use crate::debug::HotOutcome as O;
        let (kind, classes, message, lost) = match o {
            O::NoSession => ("nosession", vec![], String::new(), vec![]),
            O::NoAgent => ("noagent", vec![], String::new(), vec![]),
            O::Unchanged => ("unchanged", vec![], String::new(), vec![]),
            O::Done(c, l) => ("done", c, String::new(), l),
            O::Failed(m, c) => ("failed", c, m, vec![]),
            O::NoReply => ("noreply", vec![], String::new(), vec![]),
        };
        DebugHotswapDto { kind: kind.into(), classes, message, lost }
    }
}

/// 一个响应头（docs/IDEA.md 第 19 节）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpHeaderDto {
    pub name: String,
    pub value: String,
}

/// 一次 HTTP 请求的响应。`body` 是响应体存在哪（应用数据目录里；JSON 的话是排好的那份），`bytes` 是收到的真字节数（`truncated` 时比存下的多）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponseDto {
    pub version: String,
    pub status: u16,
    pub reason: String,
    pub headers: Vec<HttpHeaderDto>,
    pub ms: u64,
    pub bytes: u64,
    pub body: String,
    pub truncated: bool,
    pub pretty: bool,
    pub redirects: u32,
    pub final_url: String,
}

impl From<httpsvc::Response> for HttpResponseDto {
    fn from(r: httpsvc::Response) -> Self {
        HttpResponseDto {
            version: r.version,
            status: r.status,
            reason: r.reason,
            headers: r.headers.into_iter().map(|(name, value)| HttpHeaderDto { name, value }).collect(),
            ms: r.ms,
            bytes: r.bytes,
            body: r.body.display().to_string(),
            truncated: r.truncated,
            pretty: r.pretty,
            redirects: r.redirects,
            final_url: r.final_url,
        }
    }
}

/// `.http` 文件能用的环境。`json`：环境名 → 变量（合并好的，值可能是对象），原样的 JSON 文本 —— 取值（`{{a.b}}` 按路径）在前端做；
/// `files`：读了哪几个环境文件；`problems`：读坏了的（照实说）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpEnvsDto {
    pub names: Vec<String>,
    pub json: String,
    pub files: Vec<String>,
    pub problems: Vec<String>,
}

/// 参数提示里一个参数在签名里的位置（字符下标，左闭右开）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaParamDto {
    pub start: u32,
    pub end: u32,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaSignatureDto {
    pub label: String,
    pub params: Vec<JavaParamDto>,
    pub doc: String,
}

/// 参数提示（敲 `(` / `,` 时）：几个重载、jdtls 认为是哪个、光标在第几个参数；`total` 是真有几个（比 `signatures` 多就是截了）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaSignaturesDto {
    pub signatures: Vec<JavaSignatureDto>,
    pub active: u32,
    pub param: u32,
    pub total: u32,
}

/// 本地历史的一版（docs/IDEA.md 第 21 节）。`hash` 是十六进制字符串（64 位整数过不了 JS 的 number）；`kind`：saved = 在这儿保存的，before = 保存前盘上那份（外面改过 / 改之前的样子）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryDto {
    pub ms: u64,
    pub hash: String,
    pub size: u64,
    pub kind: String,
}

