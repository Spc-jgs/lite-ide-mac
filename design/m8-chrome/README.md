# M8 · 外壳分割线与弹层

`gen.py` 生成五张画板（改它，别手改 `.dc.html`），`canvas.json` 是布局，
`lite-ide-chrome-dividers.html` 是打包后的画布（发布在 claude.ai/artifact/URmYyfYo4HRcUUTKuW4EJ8）。

| 画板 | 是什么 |
|---|---|
| `Current.dc.html` | 现状：三条竖线四条横线两种亮度，侧边栏头 30 和标签栏 38 接不上 |
| `Main.dc.html` | 方向 A · 浮岛（推荐）：内容层收成内缩 6px 的圆角岛，外壳之间不画线 |
| `DirectionB.dc.html` | 方向 B · 一根线：布局不动，线统一成 `--border-soft` 并对齐 |
| `DirectionC.dc.html` | 方向 C · 只靠明暗：一条线不画，全靠色差 |
| `Popups.dc.html` | 确认条改成浮在内容区顶部的卡片；右键菜单边线降一档、分隔线内缩 |

重新打包：

```bash
python3 gen.py
node "<design skill 目录>/seed-canvas.mjs" \
  --template "<design skill 目录>/payload.template.html" \
  --out lite-ide-chrome-dividers.html --title "lite-ide 外壳分割线与弹层" \
  --artboard Main.dc.html --artboard Current.dc.html --artboard DirectionB.dc.html \
  --artboard DirectionC.dc.html --artboard Popups.dc.html --canvas canvas.json
```
