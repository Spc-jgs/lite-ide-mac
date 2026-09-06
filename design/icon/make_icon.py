#!/usr/bin/env python3
"""生成 lite-ide 的应用图标。

**为什么是脚本不是一张 PNG**：图标要出 7 个尺寸 + 一份 .icns，
手工导出一次就会有一个尺寸对不上；而且改一次配色要重来一遍。
颜色直接取自 `src/app.css` 的 token，改主题时两边不会分叉。

用法（产出物直接盖掉 src-tauri/icons/ 下那几个）：

    python3 design/icon/make_icon.py

图形是什么：**一条行号栏 + 三条日志行，中间那条是命中行**。
这就是 lite-ide 的主界面本身 —— 它不是「又一个文档图标」，
而是「打开的那份大日志」。挑这个形状还有一条硬理由：
32px 下只剩「一竖 + 三横」，那是仍然认得出的最少笔画。
"""
import subprocess, sys
from pathlib import Path
from PIL import Image, ImageDraw

# 4 倍超采样再缩回去 —— PIL 的圆角矩形没有抗锯齿，直接画 1024 边缘是锯齿状的
S = 4
CANVAS = 1024
# macOS 图标的内容框：1024 的画布里内容只占 824，四周留白是系统惯例
# （Dock 会按内容框对齐，画满会比旁边的图标大一圈）
BOX = 824
RADIUS = 185          # Big Sur squircle 的圆角，约为边长的 0.2237

# 取自 src/app.css
BG_TOP = (36, 37, 43)      # 比 --content-solid #1e1f22 亮一点，给渐变留头
BG_BOT = (18, 18, 21)      # --chrome-scrim #121215
ACCENT = (91, 141, 239)    # --accent #5b8def
LINE = (255, 255, 255)
GUTTER_A = 86              # 行号栏的白色透明度（0-255）。压暗，别让它抢主角
LINE_A = 236

def rr(d, box, r, fill):
    d.rounded_rectangle(box, radius=r, fill=fill)

def render(px: int) -> Image.Image:
    n = px * S
    k = n / CANVAS                      # 1024 坐标 → 实际像素
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))

    # ── 底：竖直渐变。一行一行画，比 PIL 的任何取巧写法都直白 ──
    grad = Image.new("RGBA", (1, CANVAS))
    gd = ImageDraw.Draw(grad)
    for y in range(CANVAS):
        t = y / (CANVAS - 1)
        gd.point((0, y), tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOT)) + (255,))
    grad = grad.resize((n, n))

    # 圆角方形当蒙版，把渐变抠出来
    mask = Image.new("L", (n, n), 0)
    off = (CANVAS - BOX) / 2
    rr(ImageDraw.Draw(mask), [off * k, off * k, (off + BOX) * k, (off + BOX) * k], RADIUS * k, 255)
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)

    # ── 顶部内高光。macOS 的图标几乎都有这一道，少了它在浅色壁纸上像贴纸 ──
    hi = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    hd = ImageDraw.Draw(hi)
    w = max(1, round(3 * k))
    hd.rounded_rectangle(
        [off * k + w / 2, off * k + w / 2, (off + BOX) * k - w / 2, (off + BOX) * k - w / 2],
        radius=RADIUS * k, outline=(255, 255, 255, 26), width=w,
    )
    img.alpha_composite(hi)

    # ── 内容：坐标写在 0..BOX 的内容框里，再平移到画布 ──
    #
    # **半透明的形状必须走 alpha_composite，不能直接 draw。**
    # ImageDraw 往 RGBA 上画是**替换**像素（连 alpha 一起替换），
    # 于是一个 alpha=46 的白条不是「压在深色底上的一道微光」，
    # 而是在图标上凿了个洞 —— 存成 PNG 之后那块是半透明的，
    # 在浅色壁纸上直接变成一条白杠。第一版就是这么翻的。
    layer = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ld = ImageDraw.Draw(layer)

    def put(x, y, w_, h_, color, alpha):
        x0 = (off + x) * k
        y0 = (off + y) * k
        rr(ld, [x0, y0, x0 + w_ * k, y0 + h_ * k], (h_ / 2) * k, color + (alpha,))

    H = 82                     # 行高。再细一档，32px 下三条线就并成一片灰
    GAP = 121                  # 行距 = 行高的 1.5 倍左右，疏密才读得出「这是文本」
    TOP = 250
    #
    # 行号栏**压暗**，而且比三条行矮一截。
    #
    # 第一版它是满高的实心白条，于是「一竖 + 三横」被读成一个大写的 E ——
    # 而这个图标要说的是「一份摊开的日志」，主角是那三条行，
    # 竖条只是告诉你左边那一列是行号。压到 0.34 之后主次就对了。
    # 整块左移 19：竖条到最长那条行的右端是 176..686，中心 431，
    # 而内容框中心是 412 —— 差着 19px，放大到 1024 的画布上是看得出来的偏右
    X = 176 - 19
    put(X, TOP + 26, 44, 2 * GAP + H - 52, LINE, GUTTER_A)
    put(X + 124, TOP, 326, H, LINE, LINE_A)
    put(X + 124, TOP + GAP, 246, H, ACCENT, 255)            # 命中行
    put(X + 124, TOP + 2 * GAP, 386, H, LINE, LINE_A)
    img.alpha_composite(layer)

    return img.resize((px, px), Image.LANCZOS)

def main():
    out = Path(__file__).resolve().parents[2] / "src-tauri" / "icons"
    master = Path(__file__).with_name("icon-1024.png")
    render(1024).save(master)

    # tauri.conf.json 的 icon 列表要这三张 PNG
    for name, px in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
        render(px).save(out / name)

    # .icns：iconutil 只吃 .iconset 目录，尺寸名字是写死的
    iconset = Path(__file__).with_name("lite-ide.iconset")
    iconset.mkdir(exist_ok=True)
    for px, name in [
        (16, "icon_16x16.png"), (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"), (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"), (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"), (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"), (1024, "icon_512x512@2x.png"),
    ]:
        render(px).save(iconset / name)
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(out / "icon.icns")], check=True)
    print(f"写好了：{out}/{{32x32,128x128,128x128@2x,icon}}.png + icon.icns")

if __name__ == "__main__":
    sys.exit(main())
