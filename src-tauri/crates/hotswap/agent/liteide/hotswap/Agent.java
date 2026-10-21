package liteide.hotswap;

import java.io.File;
import java.lang.instrument.ClassDefinition;
import java.lang.instrument.Instrumentation;
import java.lang.management.ManagementFactory;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * 热替换的代理（docs/IDEA.md 第 26 节）：调试时 lite-ide 让被调试的 JVM 多带一个 {@code -javaagent:lite-hotswap.jar=<控制目录>}。
 *
 * <p>为什么不走 java-debug 自己的热替换：它只在 jdtls 自己构建完（{@code POST_BUILD}）时收集改过的 class，而我们的构建是 Maven 做的，
 * jdtls 的自动构建关着（第 22 节：两个编译器写同一个 target/classes 会把坏 class 写进去）。JDWP 只能挂一个调试器（java-debug 占着），
 * 所以换类走 JVM 的另一个入口：{@link Instrumentation#redefineClasses}，和 JDWP 的 RedefineClasses 是同一个底层（JVMTI）、同样只能改方法体。
 *
 * <p>和 lite-ide 说话靠文件：控制目录（0700，在用户自己的临时目录里）下按 pid 开一个子目录，lite-ide 写 {@code req}、这里写回 {@code res}
 * （都是先写 .tmp 再改名，读的一方不会读到半截）。不开端口：开端口要把端口号告诉对面，而且本机别的进程也连得上。
 *
 * <p>一个守护线程每 200ms 看一眼有没有 {@code req}。java-debug 的断点只停撞上的那个线程（SUSPEND_EVENT_THREAD），所以停在断点时这条线程照样在跑；
 * 只有「暂停全部」时它也停着，那时 lite-ide 等不到回话、照实说。
 *
 * <p>按 Java 8 写、{@code --release 8} 编：被调试的项目可能还在 Java 8 上。
 */
public final class Agent {
    private Agent() {}

    public static void premain(String args, Instrumentation inst) {
        start(args, inst);
    }

    static void start(String args, final Instrumentation inst) {
        if (args == null || args.isEmpty() || !inst.isRedefineClassesSupported()) return;
        final File dir = new File(args, pid());
        if (!dir.mkdirs() && !dir.isDirectory()) return;
        Thread t = new Thread(new Runnable() {
            public void run() {
                loop(dir, inst);
            }
        }, "lite-hotswap");
        t.setDaemon(true);
        t.start();
    }

    /** Java 8 没有 ProcessHandle：RuntimeMXBean 的名字是「pid@主机名」 */
    private static String pid() {
        String n = ManagementFactory.getRuntimeMXBean().getName();
        int at = n.indexOf('@');
        return at > 0 ? n.substring(0, at) : n;
    }

    private static void loop(File dir, Instrumentation inst) {
        File req = new File(dir, "req");
        while (true) {
            try {
                Thread.sleep(200);
                if (!req.isFile()) continue;
                List<String> lines = Files.readAllLines(req.toPath(), StandardCharsets.UTF_8);
                req.delete();
                String res = handle(lines, inst);
                File tmp = new File(dir, "res.tmp");
                Files.write(tmp.toPath(), res.getBytes(StandardCharsets.UTF_8));
                Files.move(tmp.toPath(), new File(dir, "res").toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            } catch (InterruptedException e) {
                return;
            } catch (Throwable e) {
                // 一次没做成不能把线程弄死：下一次构建还要用它。对面等不到回话会照实说
            }
        }
    }

    /**
     * 请求：第一行 {@code id\t<n>}，之后每行 {@code <类全名>\t<class 文件的绝对路径>}。
     * 回话：第一行 {@code id\t<n>}，第二行 {@code ok} 或 {@code fail\t<JVM 的原话>}，之后 {@code redefined\t<类>} / {@code unloaded\t<类>}。
     *
     * <p>一批一起换（一次 redefineClasses）：JVM 保证要么全换、要么一个不换。逐个换的话，A 改成调 B 的新方法、B 却因为加了方法换不了，
     * A 换上了就是一个跑到那儿必抛 NoSuchMethodError 的程序。
     * 还没加载的类不用换：它第一次被用到时从盘上读，读到的就是新的。同名的类可能被几个类加载器各载一份，都换。
     */
    static String handle(List<String> lines, Instrumentation inst) {
        StringBuilder out = new StringBuilder();
        Map<String, String> want = new HashMap<String, String>();
        for (String l : lines) {
            int tab = l.indexOf('\t');
            if (tab <= 0) continue;
            String k = l.substring(0, tab), v = l.substring(tab + 1);
            if (k.equals("id")) out.append("id\t").append(v).append('\n');
            else want.put(k, v);
        }
        List<ClassDefinition> defs = new ArrayList<ClassDefinition>();
        List<String> done = new ArrayList<String>();
        Map<String, Boolean> seen = new HashMap<String, Boolean>();
        try {
            for (Class<?> c : inst.getAllLoadedClasses()) {
                String path = want.get(c.getName());
                if (path == null || !inst.isModifiableClass(c)) continue;
                defs.add(new ClassDefinition(c, Files.readAllBytes(new File(path).toPath())));
                if (seen.put(c.getName(), Boolean.TRUE) == null) done.add(c.getName());
            }
            if (!defs.isEmpty()) inst.redefineClasses(defs.toArray(new ClassDefinition[0]));
        } catch (Throwable e) {
            String msg = String.valueOf(e.getMessage() == null ? e.toString() : e.getMessage()).replace('\n', ' ').replace('\t', ' ');
            return out.append("fail\t").append(msg).append('\n').toString();
        }
        out.append("ok\n");
        for (String n : done) out.append("redefined\t").append(n).append('\n');
        for (String n : want.keySet()) if (!seen.containsKey(n)) out.append("unloaded\t").append(n).append('\n');
        return out.toString();
    }
}
