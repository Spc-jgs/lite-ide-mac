-- lite-ide 的 AX 驱动库：smoke.sh 和 screenshots.sh 共用。
-- 用法：osascript ax.applescript <动作> <角色> <名字>
--   动作：click / click~（名字含）/ has / focus / row / rowfocus / paste / focuskey / caretjump
--   返回 "OK" 或 "NOTFOUND"。只有要敲键盘的动作才把应用拉到前台（见 run 里的注释）。

-- 递归找元素。**不能用 `entire contents`** —— 它返回的引用读不出 name
-- （实测 0 个有名字的元素），而一层层 `UI elements of` 出来的引用读得出。
on findIt(el, wantRole, wantName)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is wantRole then
        if wantName is "" then return el
        try
          if (value of attribute "AXTitle" of el) is wantName then return el
        end try
        try
          if (value of attribute "AXDescription" of el) is wantName then return el
        end try
      end if
    end try
    try
      repeat with c in (UI elements of el)
        set r to my findIt(c, wantRole, wantName)
        if r is not missing value then return r
      end repeat
    end try
  end tell
  return missing value
end findIt

-- 名字/取值里**包含**某段文字的元素。名字会变（「提交」→「提交 (1)」）、
-- 日志行会被拆成好几段 static text，所以精确匹配不够用
on findSub(el, wantRole, sub)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is wantRole then
        try
          if (value of attribute "AXTitle" of el) contains sub then return el
        end try
        try
          if (value of attribute "AXDescription" of el) contains sub then return el
        end try
        try
          if (value of attribute "AXValue" of el) contains sub then return el
        end try
        -- 没有 label 的 <input> 只有 placeholder，而它落在这个属性上，
        -- 上面三个都取不到 —— ⌘P 那个搜索框就是这种
        try
          if (value of attribute "AXPlaceholderValue" of el) contains sub then return el
        end try
      end if
    end try
    try
      repeat with c in (UI elements of el)
        set r to my findSub(c, wantRole, sub)
        if r is not missing value then return r
      end repeat
    end try
  end tell
  return missing value
end findSub

-- 文件树那一行：先找到名字对得上的 static text，再往上走到它所在的 AXRow
on findRow(el, nm)
  tell application "System Events"
    try
      if (value of attribute "AXRole" of el) is "AXRow" then
        if my findSub(el, "AXStaticText", nm) is not missing value then return el
      end if
    end try
    try
      repeat with c in (UI elements of el)
        set r to my findRow(c, nm)
        if r is not missing value then return r
      end repeat
    end try
  end tell
  return missing value
end findRow

on run argv
  set act to item 1 of argv
  set wantRole to item 2 of argv
  set wantName to item 3 of argv
  tell application "System Events"
    tell process "lite-ide"
      -- **只有要敲键盘的动作才抢焦点。**
      --
      -- `keystroke` 发给的是**当前前台应用**，不抢的话 paste 那两下
      -- ⌘A/⌘V 会打进用户正在用的那个应用里去 —— 那不只是没做成，
      -- 是在别人的文档上乱按。
      --
      -- 而 click / 读属性 / set focused **在后台完全生效**（实测：
      -- 前台停在 Finder，点「Git 改动」Git 面板照常出来、点文件树的行
      -- 文件照常打开）。原来这里无条件抢，于是跑一遍 smoke.sh
      -- 几百次 AX 调用全在抢焦点，人根本没法同时用电脑。
      -- `rowfocus` 也要抢：它设的键盘焦点是给紧接着那下 ⇧F10 用的，
      -- 而 `keys` 里那句 `set frontmost` 在激活应用时会让 webview
      -- **把焦点复位**（同 paste 分支的注释）——
      -- 于是行上的焦点没了，⇧F10 打空，菜单不出来。
      -- 先在这儿激活，`keys` 那次就成了 no-op，焦点保得住。
      if act is "paste" or act is "rowfocus" or act is "focuskey" or act is "caretjump" then set frontmost to true
      set w to window 1
    end tell
  end tell
  delay 0.2
  if act is "row" or act is "rowfocus" then
    set el to findRow(w, wantName)
  else if act is "click~" or act is "has" then
    set el to findSub(w, wantRole, wantName)
  else if act is "focuskey" or act is "caretjump" then
    -- 名字那一栏被借去装参数了，所以按角色找，不按名字
    set el to findIt(w, wantRole, "")
  else
    set el to findIt(w, wantRole, wantName)
  end if
  if el is missing value then return "NOTFOUND"
  tell application "System Events"
    if act is "click" or act is "click~" then
      click el
    else if act is "focus" then
      set focused of el to true
    else if act is "paste" then
      -- **聚焦和敲键必须在同一次 osascript 里**。分成两次的话，第二次那句
      -- `set frontmost to true` 会让 webview 把焦点复位，于是 ⌘A/⌘V 打空 ——
      -- 现象是提交框里还是占位符、编辑器被 ⌘A 清空后存成了 0 字节。
      set focused of el to true
      delay 0.4
      keystroke "a" using {command down}
      delay 0.2
      keystroke "v" using {command down}
      delay 0.5
    else if act is "caretjump" then
      -- **把光标送到某一行某一列，然后 ⌘B。全在一次调用里。**
      --
      -- 不用 ⌘F 定位是因为那条路要开查找面板、↵、再 Esc 关掉，三步里
      -- 任何一步的焦点没接上，后面的 ⌘B 就打空 —— 而打空和「跳转坏了」
      -- 在结果上一模一样。方向键是确定的：⌘↑ 回文档开头，再数格子。
      --
      -- `wantName` 装的是 "下几行:右几列"（借这一栏，见上面 focuskey）。
      set AppleScript's text item delimiters to ":"
      set nn to text items of wantName
      set AppleScript's text item delimiters to ""
      set downN to (item 1 of nn) as integer
      set rightN to (item 2 of nn) as integer
      set focused of el to true
      delay 0.4
      key code 126 using {command down}
      delay 0.3
      repeat downN times
        key code 125
      end repeat
      delay 0.2
      repeat rightN times
        key code 124
      end repeat
      delay 0.3
      keystroke "b" using {command down}
      delay 0.3
    else if act is "focuskey" then
      -- **聚焦 + 敲键必须在同一次 osascript 里**，理由同上面 paste 那条：
      -- 分成两次的话，第二次那句 `set frontmost to true` 会让 webview 把焦点
      -- 复位，于是那个 ⌘ 组合打空 —— 表现成「⌘F 按了查找框不出来」，
      -- 而焦点明明刚设过（`set focused` 返回 OK）。踩过一次。
      set focused of el to true
      delay 0.4
      keystroke wantName using {command down}
      delay 0.3
    else if act is "row" then
      set selected of el to true
      try
        click el
      end try
    else if act is "rowfocus" then
      -- **只聚焦，不点击。** ⇧F10 的 handler 绑在每一行上（onRowKey），
      -- 要那一行拿到**键盘焦点**才收得到；而 `row` 的 click 会把文件打开，
      -- 焦点跟着跑去编辑器，⇧F10 就再也到不了树上了。
      try
        set selected of el to true
      end try
      set focused of el to true
    end if
  end tell
  return "OK"
end run
