"""把 frontlog（前台切换）和 smoke 的段落标记按时间对上：测试应用每一次跑到前台，发生在哪一段、这段开始后多少毫秒、从谁手里拿走的、占了多久。

用法：frontlog-report.py <marks.tsv> <front.tsv> <测试应用的 bundle id>
  marks.tsv：<毫秒>\t<段名>（smoke 每段开头、lite_launch 每次启动各一行）
  front.tsv：<毫秒>\t<bundle id>\t<名字>\t<pid>（frontlog.swift 写的）

lite_launch 起应用那一下抢焦点是已知的（tao 无条件 activate，bridge.sh 里那段），之后立刻还回去 —— 标成「启动」，不算数。

**分不出「程序拉到前台」和「人点过去 / ⌘Tab 过去」**：系统的激活通知两种都发。所以每条都说「从谁手里拿走、之后给了谁」，
读的人对一下自己那时候在不在动电脑。
只读两份文本，不碰任何进程。
"""
import sys


def load(path, cols):
    rows = []
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if len(parts) >= cols and parts[0].isdigit():
                    rows.append((int(parts[0]), *parts[1:cols]))
    except FileNotFoundError:
        pass
    return rows


def main():
    marks, front, me = load(sys.argv[1], 2), load(sys.argv[2], 4), sys.argv[3]
    if not front:
        print("  （前台切换没有记录：frontlog 没起来？）")
        return
    launch, other = [], []
    for i, (t, bid, name, pid) in enumerate(front):
        if bid != me or i == 0:
            continue
        mark = max((m for m in marks if m[0] <= t), default=None, key=lambda m: m[0])
        prev = front[i - 1][2]
        nxt = front[i + 1] if i + 1 < len(front) else None
        held_s = f"占了 {nxt[0] - t} ms、之后给了「{nxt[2]}」" if nxt else "到结束一直在前台"
        where = f"{mark[1]}（开始后 {t - mark[0]} ms）" if mark else "第一段之前"
        line = f"从「{prev}」手里拿走，{held_s}；发生在 {where}"
        # 「启动那一下」只有**很快还回去**才算已知的：lite_launch 起来后会把焦点还给原来的应用，正常是零点几秒。
        # 占了好几秒、或者一直没还的，照样列出来 —— 第一版只看「是不是刚启动」，㉓ 重启后焦点一直没还回去，被归进了「已知」里（2026-10-10）
        quick = nxt is not None and nxt[0] - t < 2000
        is_launch = mark and mark[1] == "[启动测试应用]" and t - mark[0] < 5000
        (launch if is_launch and quick else other).append(line)
    print(f"  测试应用跑到前台 {len(launch) + len(other)} 次：启动那一下、很快还回去的 {len(launch)} 次（已知），别的 {len(other)} 次")
    for line in other:
        print(f"    · {line}")


main()
