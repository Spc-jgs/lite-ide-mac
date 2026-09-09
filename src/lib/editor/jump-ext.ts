/**
 * ⌘Click / ⌘B 的 CM6 那一半：下划线、点击、键位。
 *
 * 解析逻辑全在 [`./jump`]（纯函数、可测），这里只管**什么时候问它**
 * 以及**问出来之后画什么**。
 *
 * # ⌘Click 和多光标是**共存**的，一开始以为要二选一
 *
 * `clickAddsSelectionRange` 的默认行为是「macOS 上检查 `event.metaKey`」——
 * 也就是说 ⌘Click 本来是「加一个光标」，看着像必须抢过来。
 *
 * 但不用抢：跳转只吃**有下划线的那个词**上的那一下（`mousedown` 命中才
 * `return true`），别处的 ⌘Click 一律 `return false` 交回 CM6，照旧加光标。
 * 所以多光标一点没少，只是「在一个能跳转的词上 ⌘Click」现在是跳转。
 *
 * **试过把它挪到 ⌥Click，那条路本来就走不通**：`rectangularSelection()`
 * 已经装着，而它默认就吃 `altKey`（`e => e.altKey && e.button == 0`），
 * 走的还是 `mouseSelectionStyle` —— 那条路在 `basicMouseSelection` **之前**，
 * 返回了 style 就轮不到 `clickAddsSelectionRange` 说话。也就是说 ⌥Click
 * 会被矩形选择整个接走。要走那条路就得把矩形选择改成 Shift+⌥
 * （VSCode 的列选手势），一次动两个已有手势，不值。
 *
 * # 下划线走 StateField，不在 DOM 事件里直接改 ViewPlugin
 *
 * 试过「ViewPlugin 自己存 decorations，事件handler 里改完调一次
 * `view.update([])` 逼它重画」—— 那条路能不能用取决于「此刻是不是正在
 * 一次 update 里」，而 CM6 对此是会抛的（`Calls to EditorView.update are not
 * allowed while an update is in progress`）。改成 StateEffect + StateField：
 * 事件里 `dispatch` 一个 effect，剩下的交给 CM6 自己的更新周期。
 *
 * 高频的 `mousemove` 不会因此变贵 —— 只有**下划线真的要变**时才 dispatch，
 * 还在同一个词上时直接返回。
 */

import { Decoration, type DecorationSet, EditorView, keymap } from "@codemirror/view";
import { Prec, StateEffect, StateField, type Extension } from "@codemirror/state";
import type { JumpHit } from "./jump";

export interface JumpHooks {
  /** 问一次「这个位置能不能跳」。同步，纯计算 —— 见 jump.ts 的说明 */
  resolve: (pos: number) => JumpHit | null;
  /** 真的跳。由 App 决定开哪个标签、跳到哪一行、以及把当前位置压进导航栈 */
  jump: (hit: JumpHit) => void;
}

/** null 表示把下划线撤掉 */
const setTarget = StateEffect.define<{ from: number; to: number } | null>();

const underline = Decoration.mark({ class: "cm-jump-target" });

const targetField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    for (const e of tr.effects) {
      if (e.is(setTarget)) {
        return e.value ? Decoration.set([underline.range(e.value.from, e.value.to)]) : Decoration.none;
      }
    }
    // 文档一改，位置就不作数了 —— 撤掉，让下一次 mousemove 重新算
    return tr.docChanged ? Decoration.none : deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/**
 * 装上 ⌘Click 跳转和 ⌘B 跳转。**不碰多光标**，理由见文件头。
 *
 * `Prec.highest` 给 ⌘B：CM6 的 `defaultKeymap` 里现在没有 `Mod-b`，
 * 但语言包和将来加的扩展可能有 —— 抢在最前面，省得哪天被静默截胡
 * （「装了不生效」这类 bug 不报错，只是按了没反应）。
 */
export function jumpExtension(hooks: JumpHooks): Extension {
  /**
   * 上一次画下划线的范围。
   *
   * 存在闭包里而不是 StateField 里：它只用来**省掉重复计算**，
   * 不参与渲染。放进 state 等于每次 mousemove 都要过一遍事务。
   */
  let at: { from: number; to: number } | null = null;

  const clear = (view: EditorView) => {
    if (!at) return;
    at = null;
    view.dispatch({ effects: setTarget.of(null) });
  };

  /** ⌘ 按着时算一次鼠标底下那个词 */
  const refresh = (view: EditorView, x: number, y: number) => {
    const pos = view.posAtCoords({ x, y });
    if (pos === null) return clear(view);
    // 还在同一个词上就什么都不做 —— mousemove 是高频事件
    if (at && pos >= at.from && pos <= at.to) return;
    const hit = hooks.resolve(pos);
    if (!hit) return clear(view);
    at = { from: hit.from, to: hit.to };
    view.dispatch({ effects: setTarget.of(at) });
  };

  /** ⌘ 松开之后鼠标停在哪儿 —— keyup 时没有坐标，得记着 */
  let xy: { x: number; y: number } | null = null;

  return [
    targetField,
    EditorView.domEventHandlers({
      mousemove(e, view) {
        xy = { x: e.clientX, y: e.clientY };
        if (e.metaKey) refresh(view, e.clientX, e.clientY);
        else clear(view);
        return false;
      },
      mouseleave(_e, view) {
        xy = null;
        clear(view);
        return false;
      },
      blur(_e, view) {
        clear(view);
        return false;
      },
      // 按住 / 松开 ⌘ 时鼠标不动，也要跟着亮 / 灭
      keydown(e, view) {
        if (e.metaKey && xy) refresh(view, xy.x, xy.y);
        return false;
      },
      keyup(e, view) {
        if (!e.metaKey) clear(view);
        return false;
      },
      mousedown(e, view) {
        if (!e.metaKey || e.button !== 0) return false;
        const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
        if (pos === null) return false;
        const hit = hooks.resolve(pos);
        /*
         * **没命中就把这一下原样交回 CM6**（`return false`）。
         *
         * 这一行就是「跳转和多光标共存」的全部实现：没有下划线的地方，
         * ⌘Click 照旧是 CM6 的「加一个光标」，跳转在那儿等于不存在。
         * 改成 `return true` 吞掉的话，一个用了很久的手势会在**看不出
         * 边界的地方**忽然失灵 —— 而下划线正是那条边界。
         */
        if (!hit) return false;
        e.preventDefault();
        clear(view);
        hooks.jump(hit);
        return true;
      },
    }),
    Prec.highest(
      keymap.of([
        {
          key: "Mod-b",
          preventDefault: true,
          run: (view) => {
            const hit = hooks.resolve(view.state.selection.main.head);
            if (!hit) return false;
            hooks.jump(hit);
            return true;
          },
        },
      ]),
    ),
    EditorView.theme({
      /*
       * 只有下划线，不换颜色。
       *
       * IDEA 那边 ⌘hover 是「变蓝 + 下划线」，但那套配色里标识符本来就是浅色；
       * 这里的高亮是按语法着色的，再压一层蓝会把「这是个类型 / 这是个方法」
       * 的信息盖掉。下划线是**加**上去的，不覆盖任何已有的信息。
       */
      ".cm-jump-target": {
        textDecoration: "underline",
        textDecorationThickness: "1px",
        textUnderlineOffset: "2px",
        cursor: "pointer",
      },
    }),
  ];
}
