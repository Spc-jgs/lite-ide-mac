"""
生成三张分割线方向 + 一张现状 + 一张弹层对照的画板。

色值、尺寸全部取自 src/app.css 与组件源码（--border 9%、--border-soft 5%、
--content-bg rgba(30,31,34,.94)、--elevated #232326、--r-sm/md 6/10、标签栏 38、
侧边栏头 30、面板头 32、导轨 34、状态栏 24、标题栏 38）。窗口缩成 960×620，
比例照真机。玻璃用一层渐变模拟壁纸透上来的样子。
"""
from pathlib import Path

HERE = Path(__file__).parent

HEAD = """<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
  <style>
    body { margin: 0; background: #2b2d30; }
    a { color: #6ba1e8; } a:hover { color: #8fbcf0; }
    .lt { font-family: -apple-system, "SF Pro Text", "PingFang SC", system-ui, sans-serif; }
    .mono { font-family: "SF Mono", Menlo, ui-monospace, monospace; }
%(css)s
  </style>
</helmet>
"""
TAIL = """
</x-dc>
</body>
</html>
"""

# 玻璃：真机上是 NSVisualEffectView 把壁纸模糊后透上来，这里用一块偏冷的深灰渐变代替
GLASS = "linear-gradient(135deg, #3a3c44 0%, #2a2b31 55%, #26272c 100%)"
CONTENT = "rgba(30, 31, 34, 0.94)"
SCRIM = "rgba(0, 0, 0, 0.20)"
BORDER = "rgba(255, 255, 255, 0.09)"
SOFT = "rgba(255, 255, 255, 0.05)"

ICON_RAIL = """
<div style="display:flex;flex-direction:column;align-items:center;gap:2px;padding:5px 0 6px;height:100%%;box-sizing:border-box;%(extra)s">
  <div style="width:26px;height:26px;border-radius:10px;background:rgba(255,255,255,.11);display:grid;place-content:center;">
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="#fff" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="12" height="10" rx="2"/><path d="M6.4 3V13"/></svg>
  </div>
  <div style="width:26px;height:26px;border-radius:10px;background:rgba(255,255,255,.11);display:grid;place-content:center;">
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="#fff" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 12.4V4.6a1.2 1.2 0 0 1 1.2-1.2h2.3l1.3 1.6h4.8a1.2 1.2 0 0 1 1.2 1.2v6.2a1.2 1.2 0 0 1-1.2 1.2H3.8a1.2 1.2 0 0 1-1.2-1.2Z"/></svg>
  </div>
  <div style="width:26px;height:26px;display:grid;place-content:center;color:#8a8a8a;">
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><circle cx="4.6" cy="3.4" r="1.7"/><circle cx="4.6" cy="12.6" r="1.7"/><circle cx="11.4" cy="3.4" r="1.7"/><path d="M4.6 5.1V10.9"/><path d="M11.4 5.1V6.6a2.4 2.4 0 0 1-2.4 2.4H4.6"/></svg>
  </div>
  <div style="width:26px;height:26px;display:grid;place-content:center;color:#8a8a8a;">
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"><circle cx="7" cy="7" r="4"/><path d="M9.95 9.95L13.2 13.2"/></svg>
  </div>
  <div style="width:26px;height:26px;display:grid;place-content:center;color:#8a8a8a;">
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><path d="M4.2 2.6H9.6L12.6 5.6V12.2a1.2 1.2 0 0 1-1.2 1.2H4.2a1.2 1.2 0 0 1-1.2-1.2V3.8a1.2 1.2 0 0 1 1.2-1.2Z"/><path d="M9.4 2.8V5.8H12.4"/><path d="M5.6 8.2H10.4"/><path d="M5.6 10.6H8.8"/></svg>
  </div>
  <div style="flex:1"></div>
  <div style="width:26px;height:26px;display:grid;place-content:center;color:#8a8a8a;">
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="12" height="10" rx="2"/><path d="M5.1 6.5L7.2 8.5L5.1 10.5"/><path d="M8.6 10.6H11.1"/></svg>
  </div>
</div>
"""

TREE_ROWS = [("src", True), ("src-tauri", True), ("docs", True), ("AGENTS.md", False), ("README.md", False), ("package.json", False)]

def tree(active_row="README.md"):
    out = []
    for name, is_dir in TREE_ROWS:
        bg = "rgba(255,255,255,.11)" if name == active_row else "transparent"
        color = "#fff" if is_dir or name == active_row else "#cdcdcd"
        chev = '<span style="width:12px;color:#8a8a8a;font-size:9px;">›</span>' if is_dir else '<span style="width:12px"></span>'
        out.append(
            f'<div style="display:flex;align-items:center;gap:3px;height:24px;padding:0 8px 0 6px;margin:0 6px;border-radius:10px;background:{bg};color:{color};font-size:12.5px;">{chev}<span>{name}</span></div>'
        )
    return "\n".join(out)

CODE_LINES = [
    ('<span style="color:#5b8def;font-weight:600;">#</span> <span style="color:#5b8def;font-weight:600;">lite-ide</span>', True),
    ("", False),
    ('macOS 上 1 秒打开的个人工作台。GB 级日志秒开不卡，代码高亮够用就停。', False),
    ("", False),
    ('Tauri 2 + Svelte 5 + CodeMirror 6，日志引擎自研（mmap + 稀疏索引）。', False),
    ('<b style="color:#e8e8ea">没有插件系统、没有遥测、没有更新器。</b>', False),
    ("", False),
    ('<span style="color:#8a8a8a;font-style:italic;">这是给自己用的工具，公开出来是因为里面几个决定可能对别人有用。</span>', False),
]

def editor():
    rows = []
    for i, (html, big) in enumerate(CODE_LINES, 1):
        fs = "17px" if big else "12.5px"
        rows.append(
            f'<div style="display:flex;gap:14px;height:20px;align-items:center;"><span class="mono" style="width:28px;text-align:right;color:#4a4a4d;font-size:11.5px;">{i}</span><span class="mono" style="color:#cdcdcd;font-size:{fs};white-space:nowrap;">{html}</span></div>'
        )
    return "\n".join(rows)

LOG_ROWS = [("main", "编辑器字号 ⌘= / ⌘- / ⌘0", "7 minutes ago"), ("", "编辑手感：⌘N 后光标在编辑器里", "13 minutes ago"), ("", "空态按「有没有项目」说两套话", "25 minutes ago"), ("", "入口包瘦身：139,115 → 133,492 B", "20 hours ago")]

def gitlog():
    out = []
    for tag, subj, when in LOG_ROWS:
        t = f'<span class="mono" style="font-size:10.5px;padding:1px 6px;border-radius:6px;background:rgba(255,255,255,.11);color:#cdcdcd;margin-right:6px;">{tag}</span>' if tag else ""
        out.append(
            f'<div style="display:flex;align-items:center;gap:8px;height:26px;padding:0 10px;font-size:12px;color:#cdcdcd;"><span style="width:8px;height:8px;border-radius:50%;border:1.5px solid #5b8def;margin-right:4px;"></span>{t}<span style="flex:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">{subj}</span><span style="color:#8a8a8a;font-size:11.5px;">pc shao</span><span style="color:#8a8a8a;font-size:11.5px;width:96px;text-align:right;">{when}</span></div>'
        )
    return "\n".join(out)

def tabs_bar(height=38, extra=""):
    return f"""
<div style="display:flex;align-items:center;gap:2px;height:{height}px;padding:0 6px;box-sizing:border-box;{extra}">
  <div style="display:flex;align-items:center;gap:6px;height:28px;padding:0 8px 0 10px;border-radius:6px;background:rgba(255,255,255,.11);color:#fff;font-size:12.5px;">
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="#8a8a8a" stroke-width="1.25"><rect x="2.5" y="3" width="11" height="10" rx="1.5"/><path d="M5 6.5h6M5 9.5h4"/></svg>README.md
    <span style="width:16px;text-align:center;color:#8a8a8a;">×</span>
  </div>
  <div style="display:flex;align-items:center;gap:6px;height:28px;padding:0 8px 0 10px;border-radius:6px;color:#cdcdcd;font-size:12.5px;">
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="#8a8a8a" stroke-width="1.25"><rect x="2.5" y="3" width="11" height="10" rx="1.5"/><path d="M5 6.5h6M5 9.5h4"/></svg>AGENTS.md
    <span style="width:6px;height:6px;border-radius:50%;background:#cdcdcd;margin:0 5px;"></span>
  </div>
  <span style="width:22px;height:22px;display:grid;place-content:center;color:#8a8a8a;font-size:15px;">+</span>
</div>"""

def side_head(height=30, extra=""):
    return f"""
<div style="display:flex;align-items:center;height:{height}px;padding:0 10px;box-sizing:border-box;font-size:11px;letter-spacing:.06em;text-transform:uppercase;color:#cdcdcd;{extra}">LITE-IDE<span style="flex:1"></span>
<svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="#8a8a8a" stroke-width="1.25" stroke-linecap="round"><path d="M5 6l3-3 3 3M5 10l3 3 3-3"/></svg></div>"""

def panel_head(height=32, extra=""):
    return f"""
<div style="display:flex;align-items:center;gap:2px;height:{height}px;padding:0 4px 0 9px;box-sizing:border-box;{extra}">
  <span style="font-size:12px;color:#fff;padding-right:7px;">Git</span>
  <span style="display:flex;align-items:center;height:22px;padding:0 9px;border-radius:6px;background:rgba(255,255,255,.11);color:#fff;font-size:12px;">提交历史</span>
  <span style="display:flex;align-items:center;height:22px;padding:0 9px;border-radius:6px;color:#cdcdcd;font-size:12px;">控制台</span>
  <span style="flex:1"></span>
  <span style="width:22px;height:22px;display:grid;place-content:center;color:#8a8a8a;">—</span>
</div>"""

def filter_bar(extra=""):
    return f"""
<div style="display:flex;align-items:center;gap:10px;height:34px;padding:0 10px;box-sizing:border-box;font-size:12px;color:#cdcdcd;{extra}">
  <span style="display:flex;align-items:center;height:22px;width:220px;padding:0 8px;border-radius:6px;background:#2e2e31;color:#8a8a8a;">过滤标题 / 作者 / sha</span>
  <span>✓ 全部分支</span><span style="color:#8a8a8a;">☐ 只看当前文件</span><span style="flex:1"></span><span style="color:#8a8a8a;">220 条</span>
</div>"""

def titlebar(extra=""):
    return f"""
<div style="display:flex;align-items:center;gap:8px;height:38px;padding:0 0 0 78px;box-sizing:border-box;font-size:12.5px;color:#cdcdcd;background:{SCRIM};{extra}">
  <span style="position:absolute;left:12px;top:13px;display:flex;gap:8px;"><i style="width:12px;height:12px;border-radius:50%;background:#ff5f57;display:block"></i><i style="width:12px;height:12px;border-radius:50%;background:#febc2e;display:block"></i><i style="width:12px;height:12px;border-radius:50%;background:#28c840;display:block"></i></span>
  <span style="display:inline-grid;place-content:center;width:16px;height:16px;border-radius:4px;background:#5b8def;color:#fff;font-size:10px;font-weight:600;">L</span><span style="color:#fff;">lite-ide</span><span style="color:#8a8a8a;">⌄</span>
  <span style="margin-left:14px;color:#8a8a8a;">⑂</span><span>main</span><span style="color:#5b8def;">↑7</span><span style="color:#8a8a8a;">⌄</span>
</div>"""

def statusbar(extra=""):
    return f"""
<div style="display:flex;align-items:center;gap:14px;height:24px;padding:0 12px;box-sizing:border-box;font-size:11px;color:#8a8a8a;background:{SCRIM};{extra}">
  <span class="mono">lite-ide <span style="color:#4a4a4d;">›</span> README.md</span><span style="flex:1"></span>
  <span class="mono">1:1</span><span>Markdown</span><span>2 空格 · LF</span><span>UTF-8</span><span>无改动</span>
</div>"""


def window(direction: str) -> str:
    """direction: now | a | b | c"""
    W, H = 960, 620
    if direction == "now":
        # 现状：--border 竖线三条、横线各画各的（侧边栏头 30 用 soft，标签栏 38 用 border）、内容层明暗跳变叠在线上
        rail = ICON_RAIL % {"extra": f"border-right:1px solid {BORDER};"}
        side = side_head(30, f"border-bottom:1px solid {SOFT};") + tree()
        side_wrap = f'<div style="width:240px;box-sizing:border-box;">{side}</div><div style="width:4px;position:relative;"><i style="position:absolute;left:1.5px;top:0;bottom:0;width:1px;background:{BORDER};display:block"></i></div>'
        main = (
            tabs_bar(38, f"border-bottom:1px solid {BORDER};")
            + f'<div style="flex:1;background:{CONTENT};padding:6px 0;">{editor()}</div>'
            + f'<div style="height:4px;position:relative;"><i style="position:absolute;left:0;right:0;top:1.5px;height:1px;background:{BORDER};display:block"></i></div>'
            + f'<div style="height:190px;display:flex;flex-direction:column;">{panel_head(32)}<div style="flex:1;background:{CONTENT};display:flex;flex-direction:column;">{filter_bar(f"border-bottom:1px solid {BORDER};")}{gitlog()}</div></div>'
        )
        tb = titlebar(f"border-bottom:1px solid {BORDER};")
        sb = statusbar(f"border-top:1px solid {BORDER};")
        body = f'<div style="display:flex;flex:1;min-height:0;">{rail_wrap(rail)}{side_wrap}<div style="flex:1;display:flex;flex-direction:column;min-width:0;">{main}</div></div>'
    elif direction == "a":
        # 浮岛：内容层是内缩 6px 的圆角岛，1px soft 描边；外壳之间不画线，缝就是分隔
        rail = ICON_RAIL % {"extra": ""}
        side = side_head(38) + tree()
        side_wrap = f'<div style="width:240px;box-sizing:border-box;">{side}</div><div style="width:6px;"></div>'
        island = f"background:{CONTENT};border:1px solid {SOFT};border-radius:8px;overflow:hidden;"
        main = (
            tabs_bar(38)
            + f'<div style="flex:1;margin:0 6px 0 0;{island}padding:6px 0;">{editor()}</div>'
            + '<div style="height:6px;"></div>'
            + f'<div style="height:190px;margin:0 6px 6px 0;display:flex;flex-direction:column;{island}">{panel_head(32, f"border-bottom:1px solid {SOFT};")}{filter_bar()}{gitlog()}</div>'
        )
        tb = titlebar()
        sb = statusbar()
        body = f'<div style="display:flex;flex:1;min-height:0;">{rail_wrap(rail)}{side_wrap}<div style="flex:1;display:flex;flex-direction:column;min-width:0;">{main}</div></div>'
    elif direction == "b":
        # 一根线：布局不动，所有分割线统一成 --border-soft，横线对齐（侧边栏头 38 = 标签栏 38），导轨不画线
        rail = ICON_RAIL % {"extra": ""}
        side = side_head(38, f"border-bottom:1px solid {SOFT};") + tree()
        side_wrap = f'<div style="width:240px;box-sizing:border-box;">{side}</div><div style="width:4px;position:relative;"><i style="position:absolute;left:1.5px;top:0;bottom:0;width:1px;background:{SOFT};display:block"></i></div>'
        main = (
            tabs_bar(38, f"border-bottom:1px solid {SOFT};")
            + f'<div style="flex:1;background:{CONTENT};padding:6px 0;">{editor()}</div>'
            + f'<div style="height:4px;position:relative;"><i style="position:absolute;left:0;right:0;top:1.5px;height:1px;background:{SOFT};display:block"></i></div>'
            + f'<div style="height:190px;display:flex;flex-direction:column;">{panel_head(32, f"border-bottom:1px solid {SOFT};")}<div style="flex:1;background:{CONTENT};display:flex;flex-direction:column;">{filter_bar()}{gitlog()}</div></div>'
        )
        tb = titlebar()
        sb = statusbar()
        body = f'<div style="display:flex;flex:1;min-height:0;">{rail_wrap(rail)}{side_wrap}<div style="flex:1;display:flex;flex-direction:column;min-width:0;">{main}</div></div>'
    else:
        # 只靠明暗：一条线都不画。边界 = 内容层的明暗跳变；侧边栏比导轨深 3%，两块玻璃才分得开
        rail = ICON_RAIL % {"extra": ""}
        side = side_head(38) + tree()
        side_wrap = f'<div style="width:240px;box-sizing:border-box;background:rgba(0,0,0,.10);">{side}</div><div style="width:4px;"></div>'
        main = (
            tabs_bar(38)
            + f'<div style="flex:1;background:{CONTENT};padding:6px 0;">{editor()}</div>'
            + '<div style="height:4px;"></div>'
            + f'<div style="height:190px;display:flex;flex-direction:column;">{panel_head(32)}<div style="flex:1;background:{CONTENT};display:flex;flex-direction:column;">{filter_bar()}{gitlog()}</div></div>'
        )
        tb = titlebar()
        sb = statusbar()
        body = f'<div style="display:flex;flex:1;min-height:0;">{rail_wrap(rail)}{side_wrap}<div style="flex:1;display:flex;flex-direction:column;min-width:0;">{main}</div></div>'

    return f"""
<div class="lt" style="position:relative;width:{W}px;height:{H}px;box-sizing:border-box;display:flex;flex-direction:column;border-radius:11px;overflow:hidden;background:{GLASS};color:#cdcdcd;box-shadow:0 20px 60px rgba(0,0,0,.5);">
{tb}
{body}
{sb}
</div>"""


def rail_wrap(rail):
    return f'<div style="width:34px;box-sizing:border-box;">{rail}</div>'


def caption(title, sub, verdict=""):
    v = f'<div style="margin-top:6px;font-size:12px;color:#63b76c;">{verdict}</div>' if verdict else ""
    return f'<div class="lt" style="margin:0 0 14px;"><div style="font-size:18px;color:#dfe1e5;letter-spacing:-.01em;">{title}</div><div style="margin-top:4px;font-size:12.5px;line-height:1.65;color:#9da0a8;max-width:900px;">{sub}</div>{v}</div>'


def page(title, sub, direction, verdict="", w=1020, h=760):
    body = f'<div class="lt" style="width:{w}px;height:{h}px;box-sizing:border-box;padding:26px 30px;background:#2b2d30;">{caption(title, sub, verdict)}{window(direction)}</div>'
    return HEAD % {"css": ""} + body + TAIL


def popups():
    conf_now = f"""
<div style="width:560px;background:{CONTENT};border-radius:6px;overflow:hidden;">
  {tabs_bar(38, f"border-bottom:1px solid {BORDER};")}
  <div style="display:flex;align-items:center;gap:10px;padding:7px 12px;background:#232326;border-bottom:1px solid {BORDER};font-size:12px;color:#cdcdcd;">
    <span><b style="color:#fff;font-weight:600;">AGENTS.md</b> 有未保存的改动</span><span style="flex:1"></span>
    <span style="padding:3px 10px;border-radius:6px;background:#5b8def;color:#fff;font-size:11.5px;">保存并关闭</span>
    <span style="padding:3px 10px;border-radius:6px;border:1px solid {BORDER};font-size:11.5px;">丢弃改动</span>
    <span style="padding:3px 10px;border-radius:6px;border:1px solid {BORDER};font-size:11.5px;">取消</span>
  </div>
  <div style="padding:6px 0;">{editor()}</div>
</div>"""
    conf_new = f"""
<div style="width:560px;background:{CONTENT};border-radius:6px;overflow:hidden;position:relative;">
  {tabs_bar(38)}
  <div style="padding:6px 0;">{editor()}</div>
  <div style="position:absolute;left:50%;top:46px;transform:translateX(-50%);display:flex;align-items:center;gap:10px;padding:8px 10px 8px 14px;background:#232326;border:1px solid {BORDER};border-radius:10px;box-shadow:0 14px 34px rgba(0,0,0,.58);font-size:12px;color:#cdcdcd;white-space:nowrap;">
    <span><b style="color:#fff;font-weight:600;">AGENTS.md</b> 有未保存的改动</span>
    <span style="padding:3px 10px;border-radius:6px;background:#5b8def;color:#fff;font-size:11.5px;margin-left:4px;">保存并关闭</span>
    <span style="padding:3px 10px;border-radius:6px;border:1px solid {BORDER};font-size:11.5px;">丢弃改动</span>
    <span style="padding:3px 10px;border-radius:6px;border:1px solid {BORDER};font-size:11.5px;">取消</span>
  </div>
</div>"""
    menu_now = f"""
<div style="width:190px;padding:5px;background:#232326;border:1px solid {BORDER};border-radius:10px;box-shadow:0 14px 34px rgba(0,0,0,.58);font-size:12.5px;color:#fff;">
  <div style="padding:3px 9px 5px;margin-bottom:3px;border-bottom:1px solid {SOFT};color:#8a8a8a;font-size:11px;">AGENTS.md</div>
  <div style="padding:4px 9px;border-radius:6px;">关闭</div>
  <div style="padding:4px 9px;border-radius:6px;background:rgba(255,255,255,.11);">关闭其他</div>
  <div style="padding:4px 9px;border-radius:6px;">关闭右侧的</div>
  <div style="padding:4px 9px;border-radius:6px;">关闭全部</div>
  <div style="margin-top:4px;padding:6px 9px 4px;border-top:1px solid {SOFT};">在文件树中定位</div>
  <div style="padding:4px 9px;border-radius:6px;">在 Finder 中显示</div>
  <div style="padding:4px 9px;border-radius:6px;">复制路径</div>
</div>"""
    menu_new = f"""
<div style="width:190px;padding:4px;background:#232326;border:1px solid {SOFT};border-radius:10px;box-shadow:0 10px 30px rgba(0,0,0,.55), 0 0 0 0.5px rgba(255,255,255,.06) inset;font-size:12.5px;color:#fff;">
  <div style="padding:3px 9px 5px;margin-bottom:2px;color:#8a8a8a;font-size:11px;">AGENTS.md</div>
  <div style="padding:4px 9px;border-radius:6px;">关闭</div>
  <div style="padding:4px 9px;border-radius:6px;background:rgba(255,255,255,.11);">关闭其他</div>
  <div style="padding:4px 9px;border-radius:6px;">关闭右侧的</div>
  <div style="padding:4px 9px;border-radius:6px;">关闭全部</div>
  <div style="height:1px;margin:4px 6px;background:{SOFT};"></div>
  <div style="padding:4px 9px;border-radius:6px;">在文件树中定位</div>
  <div style="padding:4px 9px;border-radius:6px;">在 Finder 中显示</div>
  <div style="padding:4px 9px;border-radius:6px;">复制路径</div>
</div>"""
    body = f"""
<div class="lt" style="width:1240px;height:900px;box-sizing:border-box;padding:26px 30px;background:#2b2d30;color:#dfe1e5;">
  {caption("弹层：确认条、右键菜单", "同一条规矩：<em style='color:#dfe1e5;font-style:normal'>浮层是摞在玻璃上的实心卡片，抬起靠投影，不靠边线</em>。现在的确认条是一根通栏横条，贴着标签栏，两条 1px 边线叠在一起；右键菜单已经是卡片，只是边线偏亮、分隔线两侧到底。")}
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:28px;margin-top:10px;">
    <div><div style="font-size:10.5px;letter-spacing:.08em;text-transform:uppercase;color:#6f737b;margin-bottom:8px;">现在 · 确认条是通栏横条</div>{conf_now}</div>
    <div><div style="font-size:10.5px;letter-spacing:.08em;text-transform:uppercase;color:#63b76c;margin-bottom:8px;">提案 · 浮在内容区顶部的卡片</div>{conf_new}</div>
  </div>
  <div style="margin:22px 0 6px;font-size:12.5px;line-height:1.65;color:#9da0a8;max-width:900px;">卡片居中浮在标签栏下方 8px，圆角 10、投影 <span class="mono">--shadow-pop</span>，不再撑开内容区（编辑器不会因为一条确认而整体下移一行）。冲突 / 危险两种仍按现在的底色区分（琥珀 12%、红 10%），只是换成卡片。</div>
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:28px;margin-top:14px;">
    <div><div style="font-size:10.5px;letter-spacing:.08em;text-transform:uppercase;color:#6f737b;margin-bottom:8px;">现在 · 右键菜单</div><div style="padding:18px;background:{GLASS};border-radius:8px;">{menu_now}</div></div>
    <div><div style="font-size:10.5px;letter-spacing:.08em;text-transform:uppercase;color:#63b76c;margin-bottom:8px;">提案 · 边线降一档、分隔线内缩</div><div style="padding:18px;background:{GLASS};border-radius:8px;">{menu_new}</div></div>
  </div>
  <div style="margin:14px 0 0;font-size:12.5px;line-height:1.65;color:#9da0a8;max-width:900px;">菜单只动三处：外框 <span class="mono">--border</span>(9%) → <span class="mono">--border-soft</span>(5%)，投影上再叠一圈 0.5px 的内高光让卡片边缘在深壁纸上还能读出来；分隔线两侧各内缩 6px（macOS 原生菜单就是这么画的）；标题下面那条线去掉，靠字色和间距分开。分支浮层同套。</div>
</div>"""
    return HEAD % {"css": ""} + body + TAIL


NOW_SUB = "三条竖线、四条横线，两种亮度（<span class='mono'>--border</span> 9%、<span class='mono'>--border-soft</span> 5%）混用；侧边栏头 30px、标签栏 38px，两条横线在竖线两侧<em style='color:#dfe1e5;font-style:normal'>差 8px 接不上</em>；内容层本来就比外壳深一档，边上再压一条亮线，等于同一条边说了两遍。"
A_SUB = "内容层（编辑器、底栏）收成<em style='color:#dfe1e5;font-style:normal'>内缩 6px 的圆角岛</em>（8px 圆角、1px 5% 描边），外壳之间不再画任何线 —— 岛与岛之间那条 6px 的缝就是分隔，也正好是拖拽热区。侧边栏头拉到 38px 和标签栏齐平。这是 Xcode 15 / Warp 在玻璃材质上的做法：边界由「面」的形状表达，不由线表达。"
B_SUB = "布局一个像素不动，只做减法：所有分割线统一成 <span class='mono'>--border-soft</span>（5%），导轨和侧边栏之间那条去掉（两块都是玻璃，本来就分不出层），侧边栏头拉到 38px 让横线在竖线两侧对齐，标题栏 / 状态栏靠 scrim 分区不再画线。改动最小，代价是内容层和线仍然叠在同一条边上。"
C_SUB = "一条线都不画。边界全靠明暗：内容层本来就深一档，侧边栏再压 10% 的黑让它和导轨分开。拖拽手柄平时不可见，悬停时才亮 accent。最「材质」的一种，代价是分割位置全靠色差，浅色壁纸下侧边栏和导轨的 10% 差可能读不出来。"

(HERE / "Current.dc.html").write_text(page("现状", NOW_SUB, "now"))
(HERE / "Main.dc.html").write_text(page("方向 A · 浮岛", A_SUB, "a", "推荐。玻璃材质下唯一不用「线」来说边界的做法，也是这套材质分层文档里第 2/3 层关系的自然结果。"))
(HERE / "DirectionB.dc.html").write_text(page("方向 B · 一根线", B_SUB, "b"))
(HERE / "DirectionC.dc.html").write_text(page("方向 C · 只靠明暗", C_SUB, "c"))
(HERE / "Popups.dc.html").write_text(popups())
print("ok")
