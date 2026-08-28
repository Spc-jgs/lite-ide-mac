# 设计稿

`*.dc.html` 是设计画布的画板源文件，`canvas.json` 是布局清单，
`lite-ide-interface-polish.html` 是打包后可发布的画布。

**这些是提案，不是实现。** 色值与尺寸取自 `src/app.css` 和各组件源码，
但画板里的 HTML 与 app 的真实代码没有任何关系 —— 改画板不会改 app，
改 app 也不会自动同步到画板。要改设计就改 `*.dc.html` 再重新打包：

```bash
node "<design skill 目录>/seed-canvas.mjs" \
  --template "<design skill 目录>/payload.template.html" \
  --out lite-ide-interface-polish.html \
  --title "lite-ide 界面打磨" \
  --artboard Main.dc.html --artboard GitReview.dc.html \
  --artboard GitLog.dc.html --artboard Chrome.dc.html \
  --canvas canvas.json
```

四张画板：

| 画板 | 是什么 |
|---|---|
| `Chrome.dc.html` | **提案本身** —— 六处改动的前后对照，每处写清为什么 |
| `Main.dc.html` | 主界面（编辑模式），六处改动放回真实布局 |
| `GitReview.dc.html` | Git 改动列表 + 双栏差异 |
| `GitLog.dc.html` | 底部 Git 日志工具窗（泳道图） |
