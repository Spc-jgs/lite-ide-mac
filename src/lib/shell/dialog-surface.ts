/** 表单和按钮型确认共用焦点循环；跳过隐藏分类和被禁用的控件。 */
export function cycleFocus(d: HTMLElement, back: boolean) {
  const all = [...d.querySelectorAll<HTMLElement>("button, input, textarea, select, a[href], [tabindex]")]
    .filter((n) => n.tabIndex >= 0 && !n.matches(":disabled") && !n.closest("[inert]") && n.getClientRects().length > 0);
  const i = all.indexOf(document.activeElement as HTMLElement);
  const next = i < 0 ? (back ? all.length - 1 : 0) : (i + (back ? -1 : 1) + all.length) % all.length;
  (all[next] ?? d).focus({ preventScroll: true });
}

/** 只隔离对话框之外的祖先兄弟；不能 inert 对话框自己的祖先。 */
function isolateBackground(d: HTMLElement) {
  const changed: [HTMLElement, boolean][] = [];
  for (let n: HTMLElement | null = d; n?.parentElement; n = n.parentElement) {
    for (const sibling of n.parentElement.children) {
      if (sibling === n || !(sibling instanceof HTMLElement) || sibling.tagName === "SCRIPT" || sibling.tagName === "STYLE") continue;
      changed.push([sibling, sibling.inert]);
      sibling.inert = true;
    }
    if (n.parentElement === document.body) break;
  }
  const restore = () => { for (const [n, inert] of changed) n.inert = inert; };
  return restore;
}

// 克隆窗口会收到 Git 凭据弹窗。只隔离当前最上层，关闭顺序不能留下 inert。
const dialogs: HTMLElement[] = [];
let releaseBackground: (() => void) | undefined;
function refreshIsolation() {
  releaseBackground?.();
  const top = dialogs[dialogs.length - 1];
  releaseBackground = top ? isolateBackground(top) : undefined;
}
export function isolateDialog(d: HTMLElement) {
  dialogs.push(d);
  refreshIsolation();
  return () => {
    dialogs.splice(dialogs.indexOf(d), 1);
    refreshIsolation();
  };
}

/** 确认层必须与它拦截的整个窗口同范围，避开编辑岛的裁剪和布局包含。 */
export function windowLayer(node: HTMLElement) {
  document.body.append(node);
  return { destroy: () => node.remove() };
}
