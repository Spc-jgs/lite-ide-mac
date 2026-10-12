// 任务退出那句话（src/lib/run/exit-text.ts）：shell 定死的 127 / 126 说人话，别的照实报数字，不猜是哪个命令
import { exitText } from "../src/lib/run/exit-text.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

ok(exitText("dev", 127).includes("没找到") && exitText("dev", 127).includes("command not found"), `127：${exitText("dev", 127)}`);
ok(!exitText("dev", 127).includes("npm"), "不点名猜是哪个命令");
ok(exitText("build.sh", 126).includes("chmod +x"), `126：${exitText("build.sh", 126)}`);
ok(exitText("test", 1) === "「test」退出了（退出码 1）", "别的码照实报");
ok(exitText("x", null) === "「x」退出了", "没有退出码（被信号停的）");

console.log(`${fail === 0 ? "✅" : "❌"} 任务退出的提示：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
