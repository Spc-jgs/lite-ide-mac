#!/usr/bin/env python3
"""生成 lite-ide 的应用图标：白底上一大一小两道流星划过的光痕（2026-10-14 定稿）。

用法（产出物直接盖掉 src-tauri/icons/ 下那几个；要装 Xcode 26 以上，用它的 actool）：

    python3 design/icon/make_icon.py

**两种格式，各管一段系统：**

- `AppIcon.icon` → `Assets.car`：macOS 26 起的图标格式（Icon Composer 打开的就是它）。
  只交「平的」图层，圆角方块的形状、边缘的玻璃高光由系统现场渲染 ——
  你挑的参照（VS Code、Chrome、Claude…）看起来是一块「东西」而不是一张「图」，差的就是这层。
  前几版自己画渐变、阴影、高光，出来都像网页插图（JOURNAL 2026-10-14）。
  Info.plist 的 `CFBundleIconName` 指向它（`src-tauri/Info.plist`）。
- `icon.icns`：更老的系统只认它。由同样的图层压平、套上圆角方块画出来。
  不用 actool 顺手吐出来的那份 —— 量过，那份只有 16 / 32 / 128 / 256 四档，缺 512 / 1024。

**`Assets.car` 是编好提交的**（约 2.3 MB），不在打包时现编：发版是 CI 在 GitHub 的机器上打，
那里的 Xcode 版本不归我们管；图标又很少改，和原来提交 `.icns` 是同一个做法。

**流星为什么这么画**（每一条都是用户否过一版之后才定的）：

- 它是光，不是有轮廓的色块：虚边的直线光带 + 外面一层带颜色的虚光。
  做成实心形状时，不管把尾巴修尖、修圆、修成甩尾，都还是「一块东西」。
- 颜色从头往后是 绿 → 青 → 紫，像流星烧完往下冷却。三个颜色在 OKLCH 里按同一套规则取
  （`MINT` / `CYAN` / `VIO`），中间不会掉进发灰的过渡色。
- **白底上没有单独的圆头。** 夜空版是「白热的核 + 光晕」，换到白底，亮度显不出来，人只看形状：
  比尾巴宽的实心圆头接一根细尾巴 = 大头针；核做成白点 = 缝衣针的针眼。头就是光带最宽最浓的那一端，
  圆头由模糊自然形成。夜空版的推导和样子在 JOURNAL 2026-10-14。
- 虚边靠 `feGaussianBlur`，图层**不开玻璃**：玻璃会把边缘做硬。

图层里的 SVG 是系统渲染的（`NSImage` / actool 都认 `feGaussianBlur`），所以这里只拼字符串、不依赖 PIL。
"""
import json, math, shutil, subprocess, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ICONS = HERE.parents[1] / "src-tauri" / "icons"
NAME = "AppIcon"               # 和 src-tauri/Info.plist 的 CFBundleIconName 一致

# OKLCH 取的三个颜色：薄荷绿 (.72 .17 165) / 青 (.62 .14 215) / 紫 (.55 .19 295)。
# 比夜空版深一档：白底上光显不出亮，只能靠颜色本身撑住
MINT, CYAN, VIO = "#00c589", "#009aba", "#7d51d2"
BG = "#ffffff"


def bar(hx, hy, tx, ty, w0, w1):
    """从头到尾逐渐变细的直线光带（四边形；模糊之后边缘自然是虚的）。"""
    L = math.hypot(tx - hx, ty - hy)
    nx, ny = -(ty - hy) / L, (tx - hx) / L
    pts = [(hx + nx * w0, hy + ny * w0), (tx + nx * w1, ty + ny * w1),
           (tx - nx * w1, ty - ny * w1), (hx - nx * w0, hy - ny * w0)]
    return " ".join(f"{x:.1f},{y:.1f}" for x, y in pts)


def meteor(p, hx, hy, tx, ty, k=1.0, a=1.0, w0=24):
    """一道流星的 <defs> 和图形。p 是 id 前缀（压平时两道在同一个 SVG 里，id 不能撞）。

    (hx, hy) 是运动方向上的前端，(tx, ty) 是尾巴消失的地方。头端再往前探 0.9 个半宽，
    让模糊出来的圆头正好落在 (hx, hy) 附近。
    """
    L = math.hypot(tx - hx, ty - hy)
    ux, uy = (tx - hx) / L, (ty - hy) / L
    hx, hy = hx - ux * w0 * k * .9, hy - uy * w0 * k * .9

    def blur(i, sd):
        return f'<filter id="{p}{i}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{sd:.1f}"/></filter>'

    def grad(i, stops):
        st = "".join(f'<stop offset="{o}" stop-color="{c}" stop-opacity="{al:.3f}"/>' for o, c, al in stops)
        return f'<linearGradient id="{p}{i}" gradientUnits="userSpaceOnUse" x1="{hx:.1f}" y1="{hy:.1f}" x2="{tx}" y2="{ty}">{st}</linearGradient>'

    defs = (blur("b1", 4 * k) + blur("b3", 34 * k)
            + grad("tc", [(0, MINT, a), (.08, MINT, a), (.45, CYAN, .85 * a), (.82, VIO, .4 * a), (1, VIO, 0)])
            + grad("tg", [(0, MINT, .3 * a), (.5, CYAN, .135 * a), (1, VIO, 0)]))
    return defs, (
        # 外面一层很宽很虚的光，再是光带本体，再把头端补圆
        f'<polygon points="{bar(hx, hy, tx, ty, 44 * k, 8 * k)}" fill="url(#{p}tg)" filter="url(#{p}b3)"/>'
        f'<polygon points="{bar(hx, hy, tx, ty, w0 * k, 1.5 * k)}" fill="url(#{p}tc)" filter="url(#{p}b1)"/>'
        f'<circle cx="{hx:.1f}" cy="{hy:.1f}" r="{w0 * k:.1f}" fill="{MINT}" opacity="{a:.3f}" filter="url(#{p}b1)"/>')


# 图层，从下到上。大的那道从右上往左下划过整个图标；小的在它左上方、平行，是「追光」的那一道
def layers():
    return [
        ("chaser", meteor("c", 540, 230, 240, 530, k=.9, a=.8)),
        ("meteor", meteor("m", 730, 290, 110, 910, k=1.7)),
    ]


def svg(defs, body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><defs>{defs}</defs>{body}</svg>\n'


def srgb(hex_):
    r, g, b = (int(hex_[i:i + 2], 16) / 255 for i in (1, 3, 5))
    return f"srgb:{r:.5f},{g:.5f},{b:.5f},1.00000"


def write_icon_source():
    """AppIcon.icon：Icon Composer 的格式 —— icon.json + Assets/ 下每层一个 SVG。"""
    src = HERE / f"{NAME}.icon"
    shutil.rmtree(src, ignore_errors=True)
    (src / "Assets").mkdir(parents=True)
    groups = []
    for name, (defs, body) in layers():
        (src / "Assets" / f"{name}.svg").write_text(svg(defs, body))
        groups.append({"layers": [{"image-name": f"{name}.svg", "name": name, "glass": False}],
                       "shadow": {"kind": "none", "opacity": 0},
                       "translucency": {"enabled": False, "value": 0}})
    spec = {
        "fill": {"solid": srgb(BG)},
        "groups": groups[::-1],          # icon.json 里第一组在最上面
        "supported-platforms": {"squares": ["macOS"]},
    }
    (src / "icon.json").write_text(json.dumps(spec, indent=2, ensure_ascii=False) + "\n")
    return src


def flat_svg():
    """压平的一张：白底 + 全部图层，套进圆角方块。给 .icns 和 tauri.conf 里那几张 PNG 用。

    老格式的图标要自己留白、自己切形状：1024 的画布里内容只占 824（系统惯例，
    画满了在 Dock 里会比旁边的大一圈），形状是 n=5 的超椭圆（近似 Apple 的连续圆角）。
    """
    a, n, k = 412, 5.0, 720
    pts = []
    for i in range(k):
        t = 2 * math.pi * i / k
        c, s = math.cos(t), math.sin(t)
        pts.append(f"{512 + a * math.copysign(abs(c) ** (2 / n), c):.2f},{512 + a * math.copysign(abs(s) ** (2 / n), s):.2f}")
    shape = "M" + " L".join(pts) + " Z"
    defs = f'<clipPath id="sq"><path d="{shape}"/></clipPath>'
    body = ""
    for _, (d, b) in layers():
        defs += d
        body += b
    s = 824 / 1024
    return svg(defs, f'<g clip-path="url(#sq)"><g transform="translate(100 100) scale({s})">'
                     f'<rect width="1024" height="1024" fill="{BG}"/>{body}</g></g>'
                     # 白底在白色桌面上会化掉：描一圈很淡的灰边（新格式的那圈由系统画）
                     f'<path d="{shape}" fill="none" stroke="#000" stroke-opacity=".10" stroke-width="2"/>')


def main():
    if not shutil.which("xcrun"):
        sys.exit("要 Xcode（xcrun actool / swiftc）")
    src = write_icon_source()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        # Assets.car：actool 编 .icon。partial plist 不要（CFBundleIconName 写死在 src-tauri/Info.plist）
        subprocess.run(["xcrun", "actool", str(src), "--compile", str(tmp), "--app-icon", NAME,
                        "--platform", "macosx", "--minimum-deployment-target", "26.0", "--target-device", "mac",
                        "--output-partial-info-plist", str(tmp / "partial.plist")],
                       check=True, stdout=subprocess.DEVNULL)
        shutil.copyfile(tmp / "Assets.car", ICONS / "Assets.car")

        # 兜底：压平的 SVG → 各尺寸 PNG → .icns
        exe = tmp / "svg2png"
        subprocess.run(["xcrun", "swiftc", "-O", str(HERE / "svg2png.swift"), "-o", str(exe)], check=True)
        flat = tmp / "flat.svg"
        flat.write_text(flat_svg())
        def png(px, out):
            subprocess.run([str(exe), str(flat), str(out), str(px)], check=True)
        png(1024, HERE / "icon-1024.png")
        for name, px in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
            png(px, ICONS / name)
        iconset = tmp / "lite-ide.iconset"
        iconset.mkdir()
        for px, name in [(16, "icon_16x16.png"), (32, "icon_16x16@2x.png"), (32, "icon_32x32.png"),
                         (64, "icon_32x32@2x.png"), (128, "icon_128x128.png"), (256, "icon_128x128@2x.png"),
                         (256, "icon_256x256.png"), (512, "icon_256x256@2x.png"), (512, "icon_512x512.png"),
                         (1024, "icon_512x512@2x.png")]:
            png(px, iconset / name)
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(ICONS / "icon.icns")], check=True)
    print(f"写好了：{src.relative_to(HERE.parents[1])}、src-tauri/icons/{{Assets.car,icon.icns,32x32,128x128,128x128@2x,icon}}.png")


if __name__ == "__main__":
    main()
