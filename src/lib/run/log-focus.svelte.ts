/**
 * 「按某个 ID 过滤某次运行的日志」的请求（#77：响应头里的 traceId → 服务日志）。只有运行窗、Services、HTTP 这几个懒块引它，入口包一个字节不多。
 * 运行窗的日志视图挂载时拿它当初始过滤（`RunView`），序号一变就按新条件重挂；Services 看见请求的是哪个服务，就把它选中
 */
class LogFocus {
  req = $state<{ id: number; pattern: string; nonce: number } | null>(null);
  #n = 0;

  ask(id: number, pattern: string) {
    this.req = { id, pattern, nonce: ++this.#n };
  }
}

export const logFocus = new LogFocus();
