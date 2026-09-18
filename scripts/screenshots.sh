#!/usr/bin/env bash
#
# 给 README 拍截图。造一个中性的示例项目（orderflow，一个假的 Java 订单服务），
# 起真 .app，用 AX 把界面摆成四个画面，逐个 `screencapture`，缩到 1600 宽放进
# docs/screenshots/。
#
# 为什么是脚本不是手拍：截图会过时（岛、草稿、图标这几轮每一轮都让旧图作废），
# 手拍的图没人知道当时开的是什么、窗口多大；脚本一跑就是同一批画面、同一个尺寸，
# README 里的图和 .app 里的样子对不上时，重跑一遍就是了。
#
# 起的是 Contents/MacOS 里那个裸二进制而不是 `open -a`：两者的 WebKit 存储目录不同
# （见 rules/frontend.md「驱动真 .app 的两个坑」），裸二进制那份是 smoke 用的，
# 不会把示例项目写进用户自己的「最近打开」。
#
# 用法：先 `pnpm app:bundle`，然后 `./scripts/screenshots.sh`。会占用键盘十几秒。
set -uo pipefail
export LANG=${LANG:-en_US.UTF-8}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${ROOT}/src-tauri/target/release/bundle/macos/lite-ide.app/Contents/MacOS/lite-ide"
AXLIB="${ROOT}/scripts/lib/ax.applescript"
OUT="${ROOT}/docs/screenshots"
[ -x "${APP}" ] || { echo "找不到 .app —— 先跑 pnpm app:bundle"; exit 2; }
mkdir -p "${OUT}"

WORK=$(mktemp -d /tmp/lite-ide-shots.XXXXXX)
DEMO="${WORK}/orderflow"
CLIP="${WORK}/clipboard.bak"
pbpaste > "${CLIP}" 2>/dev/null
cleanup() {
  pkill -f "MacOS/lite-ide" 2>/dev/null
  pbcopy < "${CLIP}" 2>/dev/null
  rm -rf "${WORK}"
}
trap cleanup EXIT INT TERM

ax() { osascript "${AXLIB}" "$1" "$2" "${3:-}" 2>&1 | tail -1; }
wait_has() {
  local role=$1 sub=$2 secs=${3:-8} i=0
  while [ $i -lt $((secs * 2)) ]; do
    [ "$(ax has "$role" "$sub")" = "OK" ] && return 0
    sleep 0.5; i=$((i + 1))
  done
  return 1
}
menu() { osascript -e "tell application \"System Events\" to tell process \"lite-ide\"
  set frontmost to true
  delay 0.2
  click menu item \"$2\" of menu 1 of menu bar item \"$1\" of menu bar 1
end tell" >/dev/null 2>&1; }
keys() { osascript -e "tell application \"System Events\" to tell process \"lite-ide\"
  set frontmost to true
  delay 0.2
  $1
end tell" >/dev/null 2>&1; }

# ─────────────────── 示例项目 ───────────────────
mkdir -p "${DEMO}/src/main/java/com/acme/orderflow" "${DEMO}/src/main/resources" "${DEMO}/logs" "${DEMO}/docs"
cd "${DEMO}"
cat > pom.xml <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>orderflow</artifactId>
  <version>1.4.0</version>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter-web</artifactId>
    </dependency>
  </dependencies>
</project>
XML
cat > src/main/java/com/acme/orderflow/OrderService.java <<'JAVA'
package com.acme.orderflow;

import java.time.Duration;
import java.util.List;
import java.util.Optional;
import org.springframework.stereotype.Service;

/**
 * 订单落库与重试。超时来自 {@link OrderRepository}，重试策略在 {@link #persist}。
 */
@Service
public class OrderService {
    private static final int MAX_ATTEMPTS = 3;
    private final OrderRepository repo;
    private final Metrics metrics;

    public OrderService(OrderRepository repo, Metrics metrics) {
        this.repo = repo;
        this.metrics = metrics;
    }

    public Optional<Order> find(long id) {
        return repo.findById(id);
    }

    public void persist(Order order) {
        int attempt = 0;
        while (true) {
            attempt++;
            try (var conn = repo.connection(Duration.ofMillis(300))) {
                repo.save(conn, order);
                metrics.record("order.persist", attempt);
                return;
            } catch (TimeoutException e) {
                if (attempt >= MAX_ATTEMPTS) {
                    throw new IllegalStateException("落库超时，已重试 " + attempt + " 次", e);
                }
            }
        }
    }

    public List<Order> pending() {
        return repo.findByStatus(Order.Status.PENDING);
    }
}
JAVA
cat > src/main/java/com/acme/orderflow/OrderController.java <<'JAVA'
package com.acme.orderflow;

import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/orders")
public class OrderController {
    private final OrderService service;

    public OrderController(OrderService service) {
        this.service = service;
    }

    @GetMapping("/{id}")
    public Order get(@PathVariable long id) {
        return service.find(id).orElseThrow();
    }

    @PostMapping
    public void create(@RequestBody Order order) {
        service.persist(order);
    }
}
JAVA
cat > src/main/java/com/acme/orderflow/Application.java <<'JAVA'
package com.acme.orderflow;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

@SpringBootApplication
public class Application {
    public static void main(String[] args) {
        SpringApplication.run(Application.class, args);
    }
}
JAVA
cat > src/main/resources/application.yml <<'YML'
server:
  port: 8080
orderflow:
  db:
    pool-size: 8
    timeout-ms: 300
YML
cat > README.md <<'MD'
# orderflow

订单服务。`logs/app.log` 是一天的运行日志。
MD
cat > docs/notes.md <<'MD'
# 8842013 为什么落库失败

- 14:03 起 HikariPool 拿不到连接，300ms 超时
- 重试 3 次全失败，见 `OrderService.persist`
MD
printf 'target/\nlogs/*.log\n' > .gitignore
# 日志：32 万行（会自动走日志模式），每一百行一段带堆栈的 ERROR
awk 'BEGIN {
  srand(7);
  for (i = 0; i < 320000; i++) {
    ts = sprintf("2026-09-18 14:%02d:%02d.%03d", int(i/5400)%60, int(i/90)%60, (i*37)%1000);
    if (i % 100 == 57) {
      printf "%s ERROR [scheduler-1] c.a.o.OrderService - 订单落库失败 orderId=%d，已重试 3 次\n", ts, 8842000 + i;
      printf "java.lang.IllegalStateException: 落库超时，已重试 3 次\n";
      printf "\tat com.acme.orderflow.OrderService.persist(OrderService.java:36)\n";
      printf "\tat com.acme.orderflow.OrderController.create(OrderController.java:22)\n";
      printf "Caused by: java.util.concurrent.TimeoutException: HikariPool-1 - Connection is not available, request timed out after 300ms\n";
      printf "\tat com.zaxxer.hikari.pool.HikariPool.createTimeoutException(HikariPool.java:696)\n";
    } else if (i % 23 == 0) {
      printf "%s WARN  [http-nio-8080-exec-%d] c.a.o.RetryPolicy - 重试 attempt=%d/3 backoff=%dms orderId=%d\n", ts, i%8+1, i%3+1, (i%3+1)*100, 8842000 + i;
    } else if (i % 7 == 0) {
      printf "%s DEBUG [pool-3-thread-1] c.a.o.CacheManager - evict key=order:%d\n", ts, 8842000 + i;
    } else {
      printf "%s INFO  [http-nio-8080-exec-%d] c.a.o.OrderService - 处理完成 orderId=%d elapsed=%dms\n", ts, i%8+1, 8842000 + i, (i*13)%180+4;
    }
  }
}' > logs/app.log
git init -q -b main
git -c user.name=acme -c user.email=dev@acme.example add -A
git -c user.name=acme -c user.email=dev@acme.example commit -q -m "订单服务：落库 + 重试"
# 第二个提交，泳道图上有点东西看
sed -i '' 's/pool-size: 8/pool-size: 16/' src/main/resources/application.yml
git -c user.name=acme -c user.email=dev@acme.example commit -qam "连接池 8 → 16"
git checkout -q -b fix/retry-backoff
sed -i '' 's/MAX_ATTEMPTS = 3/MAX_ATTEMPTS = 5/' src/main/java/com/acme/orderflow/OrderService.java
git -c user.name=acme -c user.email=dev@acme.example commit -qam "重试 3 → 5 次"
git checkout -q main
git -c user.name=acme -c user.email=dev@acme.example merge -q --no-ff -m "合并 fix/retry-backoff" fix/retry-backoff
# 工作区留几处改动：Git 改动面板和差异视图要有内容（改一处、删一处、加一段）
python3 - <<'PY'
p = "src/main/java/com/acme/orderflow/OrderService.java"
s = open(p).read()
s = s.replace("Duration.ofMillis(300)", "Duration.ofMillis(500)")
s = s.replace('                metrics.record("order.persist", attempt);\n', '                metrics.record("order.persist", attempt);\n                metrics.gauge("order.pending", pending().size());\n')
s = s.replace(" * 订单落库与重试。超时来自 {@link OrderRepository}，重试策略在 {@link #persist}。\n", " * 订单落库与重试。超时来自 {@link OrderRepository}，重试策略在 {@link #persist}，\n * 超过次数的进 {@link #retryLater} 排队。\n")
s = s.replace("""                if (attempt >= MAX_ATTEMPTS) {
                    throw new IllegalStateException("落库超时，已重试 " + attempt + " 次", e);
                }""", """                if (attempt >= MAX_ATTEMPTS) {
                    retryLater(order, e);
                    return;
                }""")
s = s.replace("""    public List<Order> pending() {""", """    /** 重试用尽的订单进延迟队列，不再抛给调用方 —— 调用方是 HTTP 线程，抛了就是 500 */
    private void retryLater(Order order, Exception cause) {
        metrics.record("order.retry_later", 1);
        repo.enqueue(order, Duration.ofSeconds(30), cause.getMessage());
    }

    public List<Order> pending() {""")
open(p, "w").write(s)
PY
printf 'package com.acme.orderflow;\n\npublic record Metrics(String prefix) {\n    public void record(String key, int v) {}\n    public void gauge(String key, int v) {}\n}\n' > src/main/java/com/acme/orderflow/Metrics.java
cd "${ROOT}"
echo "示例项目：${DEMO}（日志 $(du -h "${DEMO}/logs/app.log" | cut -f1)）"

# ─────────────────── 起应用、摆窗口 ───────────────────
pkill -f "MacOS/lite-ide" 2>/dev/null; sleep 0.5
LITE_IDE_POS=120,80 "${APP}" "${DEMO}" >/dev/null 2>&1 &
disown
sleep 3
osascript -e 'tell application "System Events" to tell process "lite-ide"
  set frontmost to true
  set size of window 1 to {1280, 800}
  set position of window 1 to {120, 80}
end tell' >/dev/null 2>&1
sleep 1

# 窗口的 CGWindowID：screencapture -l 要它。System Events 给的 id 不是这个
cat > "${WORK}/winid.swift" <<'SWIFT'
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list {
  if (w["kCGWindowOwnerName"] as? String) == "lite-ide", (w["kCGWindowLayer"] as? Int) == 0,
     let n = w["kCGWindowNumber"] as? Int { print(n); break }
}
SWIFT
WIN=$(swift "${WORK}/winid.swift" 2>/dev/null | tail -1)
[ -n "${WIN}" ] || { echo "拿不到窗口 id"; exit 1; }

shot() {
  local name=$1
  sleep 1.2
  screencapture -o -x -l "${WIN}" "${WORK}/${name}.png"
  # 2x 的 2560 宽缩到 1600：README 上够清楚，仓库里不背 1MB 一张的图
  sips -Z 1600 "${WORK}/${name}.png" --out "${OUT}/${name}.png" >/dev/null
  echo "  ${OUT}/${name}.png ($(du -h "${OUT}/${name}.png" | cut -f1))"
}

# ① 主画面：文件树 + Java 编辑器 + 底部 Git 工具窗（提交历史）
[ "$(ax click AXButton "文件树")" = "OK" ] || true
sleep 0.5
ax row "" "src" >/dev/null; sleep 0.6
ax row "" "main" >/dev/null; sleep 0.6
ax row "" "java" >/dev/null; sleep 0.6
ax row "" "com" >/dev/null; sleep 0.6
ax row "" "acme" >/dev/null; sleep 0.6
ax row "" "orderflow" >/dev/null; sleep 0.6
ax row "" "OrderService.java" >/dev/null
wait_has AXStaticText "MAX_ATTEMPTS" 6 || echo "  （编辑器没读到内容）"
# 启动时自动落进来的那份空草稿，这张图不要它
ax "click~" AXButton "关闭 2026" >/dev/null; sleep 0.5
# 底部工具窗是懒加载的，第一次点要等 chunk；「提交历史」那个标签页按钮出来才算开了
[ "$(ax click AXButton "Git")" = "OK" ] || true
wait_has AXButton "提交历史" 10 || { ax click AXButton "Git" >/dev/null; wait_has AXButton "提交历史" 10 || echo "  （提交历史没出来）"; }
sleep 2
shot editor

# ② 日志：32 万行，只看 ERROR + 关键字
# 收起底部工具窗：点面板头上那个「—」，不点导轨（导轨是开关，AX 点击偶尔落空就翻错方向）
for _ in 1 2 3; do
  [ "$(ax click AXButton "收起面板")" = "OK" ] || break
  sleep 1
done
ax row "" "logs" >/dev/null; sleep 0.6
ax row "" "app.log" >/dev/null
wait_has AXStaticText "Logback" 8 || echo "  （日志没开出来）"
# 只看 ERROR 那一级：3,200 条，堆栈跟着各自的那条走
[ "$(ax "click~" AXButton "ERROR")" = "OK" ] || echo "  （没点到 ERROR）"
sleep 2.5
shot log

# ③ Git 改动 + 双栏差异
[ "$(ax click AXButton "Git 改动")" = "OK" ] || true
sleep 0.8
ax "click~" AXButton "OrderService.java" >/dev/null
wait_has AXStaticText "ofMillis(500)" 6 || echo "  （差异没出来）"
shot git

# ④ 草稿：没项目、没工具窗，一座岛；Markdown 所见即所得
#
# 草稿目录是用户真实的那一个（app_data_dir 不分裸二进制和 .app），拍完要把这份草稿
# 收干净 —— 同 smoke ⑭ 的做法：记下跑之前有什么，多出来的删掉。
SCRATCHES="${HOME}/Library/Application Support/com.liteide.app/scratches"
ls "${SCRATCHES}" 2>/dev/null | sort > "${WORK}/scratch.before"
menu "文件" "关闭项目"; sleep 0.8
menu "文件" "关闭所有标签"; sleep 0.8
menu "文件" "新建草稿"; sleep 1.2
printf '# 8842013 为什么落库失败\n\n从 `app.log` 里抠出来的：\n\n- 14:03 起 HikariPool 拿不到连接，**300ms 超时**\n- 重试 3 次全失败，见 `OrderService.persist`\n- 连接池 8 → 16 之后还是这样，不是池的问题\n\n下一步：查 14:00 那批慢 SQL' | pbcopy
ax paste AXTextArea "" >/dev/null
sleep 1.5
shot scratch
menu "文件" "关闭所有标签"; sleep 1
ls "${SCRATCHES}" 2>/dev/null | sort > "${WORK}/scratch.after"
comm -13 "${WORK}/scratch.before" "${WORK}/scratch.after" | while IFS= read -r f; do
  [ -n "$f" ] && rm -f "${SCRATCHES}/${f}" && echo "  收走了拍图用的草稿 ${f}"
done

echo "四张都在 ${OUT}/。README 引用的是这四个文件名，改了名字记得同步。"
