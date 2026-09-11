#!/usr/bin/env bash
#
# 把 app.log 里那些预算行拉出来对比（issue #28）。
#
# # 这个脚本回答的问题
#
# 不是「现在占多少内存」——`vmmap` 就能回答，而且更准。
# 是**「和上个月的自己比，变胖了没有」**。issue #10 那条内存超标是手工
# vmmap 才发现的，也就是说这类问题今天只有在有人特意去量时才暴露；
# 而「慢慢变胖」恰恰是最容易在没人注意时发生的一类。
#
# # 为什么按「版本 + 标签数」分组，而不是画一条总的曲线
#
# 单次的读数说明不了什么：开 3 个标签和开 10 个标签差一倍，跑没跑终端
# 又差一截。**把它们混在一起画趋势，得到的是「用得多不多」，不是「胖没胖」。**
# 分组之后每一格里的数才是可比的，跨格比较没有意义 —— 所以这里
# 一个字都不说「涨了多少」，只把可比的那几行摆在一起让人自己看。
#
# 取中位数不取平均：`boot` 偶尔会因为冷缓存（刚开机的第一次）跳到几倍，
# 平均值会被那一次拖走，而中位数不会。
#
# 用法：
#   ./scripts/budget.sh            # 最近 20 行 + 分组汇总
#   ./scripts/budget.sh 60         # 最近 60 行
#   ./scripts/budget.sh 60 路径    # 看别人发来的一份 app.log
set -uo pipefail

N=${1:-20}
LOG=${2:-"${HOME}/Library/Logs/com.liteide.app/app.log"}

# 轮转出去的那一份在前（applog 只留一代，见 crates/applog）。
# 少了它，刚轮转过的那阵子这个脚本会说「只有两条记录」
ALL=$(cat "${LOG}.1" "${LOG}" 2>/dev/null | grep -F "[budget]")
[ -n "${ALL}" ] || {
  echo "在 ${LOG} 里没找到预算行。"
  echo "这一行是启动完成时写的 —— 用 pnpm app:bundle 打的包跑一次就有了。"
  exit 1
}

TOTAL=$(printf '%s\n' "${ALL}" | wc -l | tr -d ' ')
echo "${LOG}"
echo "共 ${TOTAL} 次启动，下面是最近 ${N} 次"
echo

# printf 的列宽按**字节**算，而一个中文字是 3 字节、在终端里占 2 格 ——
# 含中文的表头列宽要按 `想要的格数 + 3×汉字数 - 2×汉字数` 反推。
# 「时间」要占 20 格 → 20 + 2 = 22。眼睛对不齐的时候先想这件事
printf '%-22s %8s %8s %6s %6s %8s %8s %9s\n' \
  时间 boot self tabs terms editors nodes 版本
printf '%s\n' "${ALL}" | tail -n "${N}" | awk '
  {
    ts = substr($1, 6, 5) " " substr($2, 1, 8)      # 09-11 14:03:26
    delete f
    for (i = 4; i <= NF; i++) { split($i, kv, "="); f[kv[1]] = kv[2] }
    printf "%-20s %8s %8s %6s %6s %8s %8s %9s\n",
      ts, f["boot"], f["self"], f["tabs"], f["terms"], f["editors"], f["nodes"],
      f["v"] (f["devtools"] == "1" ? "+dev" : "")
  }'

echo
echo "按「版本 + 标签数」分组（同一格里的数才可比）："
echo
printf '%-14s %8s %8s %12s %12s %12s\n' 版本 标签 次数 boot中位 self中位 nodes中位
printf '%s\n' "${ALL}" | awk '
  # 后面那些参数是 awk 声明局部变量的唯一办法（不写就是全局，会串台）
  function med(arr,   i, j, v, tmp, c) {
    c = 0
    for (i in arr) { tmp[++c] = arr[i] }
    if (c == 0) return "?"
    # 插入排序：每格最多几十条，不值得为它引外部 sort
    for (i = 2; i <= c; i++) { v = tmp[i]; j = i - 1
      while (j > 0 && tmp[j] > v) { tmp[j+1] = tmp[j]; j-- }
      tmp[j+1] = v }
    return (c % 2) ? tmp[(c+1)/2] : int((tmp[c/2] + tmp[c/2+1]) / 2)
  }
  # 「没量到」的字段是 `?`，但它带着单位 —— `?ms` / `?MB`（见 budget.rs）。
  # **先去单位再判，顺序反了它就变成 0 混进中位数**：一条本来平稳的
  # 曲线会因为几次没量到而出现假的低谷，而那正是 `?` 这个约定要防的事。
  # （第一版就是反的，fixture 里那行 `?ms` 当场把 boot 中位数从 466 拉到 412。）
  function num(raw, unit,   v) {
    v = raw
    gsub(unit, "", v)
    return (v == "?") ? "" : v + 0
  }
  {
    delete f
    for (i = 4; i <= NF; i++) { split($i, kv, "="); f[kv[1]] = kv[2] }
    # 量不到的字段是 `?`（见 budget.rs）—— 它不是 0，不能进中位数
    k = f["v"] (f["devtools"] == "1" ? "+dev" : "") SUBSEP f["tabs"]
    n[k]++
    vb = num(f["boot"], "ms"); if (vb != "") b[k, ++bn[k]] = vb
    vs = num(f["self"], "MB"); if (vs != "") s[k, ++sn[k]] = vs
    vd = num(f["nodes"], "");  if (vd != "") d[k, ++dn[k]] = vd
    keys[k] = 1
  }
  END {
    for (k in keys) {
      split(k, p, SUBSEP)
      delete B; delete S; delete D
      for (i = 1; i <= bn[k]; i++) B[i] = b[k, i]
      for (i = 1; i <= sn[k]; i++) S[i] = s[k, i]
      for (i = 1; i <= dn[k]; i++) D[i] = d[k, i]
      printf "%-12s %6s %6s %10s %10s %10s\n", p[1], p[2], n[k],
        med(B) "ms", med(S) "MB", med(D)
    }
  }' | sort

cat <<'NOTE'

  self 只是主进程。WebKit 那三个（WebContent / GPU / Networking）占的是
  大头（issue #10 实测 123–206MB），从进程内部认不出来 —— 它们归
  ./scripts/mem.sh，那个脚本从外面用差分认领。
NOTE
