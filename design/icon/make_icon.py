#!/usr/bin/env python3
"""生成 lite-ide 的应用图标：深墨蓝底上的代码括号和斜线（2026-10-29）。

用户要求换图标，优先照顾 macOS Dock 和 Finder 的小尺寸：三道实线在 16 / 32 px 下
仍保留分开的轮廓，不再用缩小后只剩色斑的模糊光晕。青绿括号与浅色斜线在深底上保持对比。

只改这个脚本，运行 `python3 design/icon/make_icon.py` 更新全部产物：
- AppIcon.icon → Assets.car：macOS 26+ 的边缘和材质由系统渲染；使用 Xcode 26+ 的 actool。
- 同样的图层压平，生成完整 16–1024 px 的 icon.icns 和 Tauri 使用的 PNG。
Assets.car 预编译后提交，CI 打包不依赖它的 Xcode 是否支持 .icon。
"""
import json, math, shutil, subprocess, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ICONS = HERE.parents[1] / "src-tauri" / "icons"
NAME = "AppIcon"               # 和 src-tauri/Info.plist 的 CFBundleIconName 一致

BG = "#142331"
MINT = "#63E6BE"
INK = "#F0F7FA"


def layers():
    # 端点到斜线之间留实空隙，缩到 Finder 的 16 px 时也不粘在一起。
    return [
        ("brackets", ("", f'<path d="M350 320 L180 512 L350 704 M674 320 L844 512 L674 704" '
                          f'fill="none" stroke="{MINT}" stroke-width="76" stroke-linecap="round" stroke-linejoin="round"/>')),
        ("slash", ("", f'<path d="M574 304 L450 720" fill="none" stroke="{INK}" stroke-width="70" stroke-linecap="round"/>')),
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
    """压平的一张：底色 + 全部图层，套进圆角方块。给 .icns 和 tauri.conf 里那几张 PNG 用。

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
                     # 压平版用一圈淡边界；新格式的边缘由系统画。
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
