/** 只在本轮结束后安排下一轮。停止也使在途回包失效；调用方 await 后须查 live。 */
export function poll(ms: number, run: (live: () => boolean) => Promise<boolean>, fail: (error: unknown) => void, immediate = false): () => void {
  let active = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const tick = async () => {
    try {
      if (!active) return;
      const again = await run(() => active);
      if (active && again) timer = setTimeout(() => void tick(), ms);
      else active = false;
    } catch (e) {
      if (active) {
        active = false;
        fail(e);
      }
    }
  };
  if (immediate) void tick();
  else timer = setTimeout(() => void tick(), ms);
  return () => {
    active = false;
    clearTimeout(timer);
  };
}
