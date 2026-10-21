#!/bin/bash
# 打包前准备内置的 Java 智能（docs/IDEA.md 第 10 节）：jdtls（钉死的版本 + sha256）和一份裁剪过的 Java 21 运行时（jlink），
# 放到 src-tauri/java-bundle/，打包时由 src-tauri/tauri.java.json 当资源放进 .app 的 Contents/Resources/java/。
#
#   scripts/java-bundle.sh                 # 只准备这台机器的架构（本地 pnpm app:bundle）
#   scripts/java-bundle.sh aarch64 x86_64  # universal 包要两个架构（release.yml）
#
# 要的东西：
#   - 一个 JDK 21（带 jmods/）：用它的 jlink 裁运行时。默认 `/usr/libexec/java_home -v 21`，CI 里 setup-java 装的不在系统目录、
#     java_home 找不到，用 LITE_JDK21 指过来；别的架构的 jmods 用
#     LITE_JDK21_AARCH64 / LITE_JDK21_X86_64 指过来（**版本要和跑 jlink 的那个一模一样**，jlink 只认同版本的 jmods）
#   - jdtls 发行包：先找缓存（~/Library/Caches/lite-ide-build/），没有就从 download.eclipse.org 下（约 49 MB），核 sha256
#
# 输入没变（版本、模块表、架构）就什么都不做：每次 app:bundle 都跑它，第二次是零开销。
#
# 为什么资源不直接写进 tauri.conf.json：tauri-build 在**编译期**检查资源路径在不在，CI 的 cargo test 没有这个目录就编不过。
set -euo pipefail
cd "$(dirname "$0")/.."

JDTLS_VERSION=1.61.0
JDTLS_FILE=jdt-language-server-1.61.0-202609031315.tar.gz
JDTLS_URL="https://download.eclipse.org/jdtls/milestones/${JDTLS_VERSION}/${JDTLS_FILE}"
JDTLS_SHA256=338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64
# 调试（docs/IDEA.md 第 18 节）：java-debug 插件（VS Code 的 Java 调试用的同一个，EPL）。jdtls 起的时候当 bundle 带上，它给 jdtls 加「起一个调试会话」的命令。
# Maven Central 上最新的是 0.53.1（GitHub 上有 0.53.2 的 tag、没有构建好的 jar）。Central 只给 sha1；sha256 是核对过 sha1 之后算的、钉在这里
DEBUG_VERSION=0.53.1
DEBUG_FILE=com.microsoft.java.debug.plugin-${DEBUG_VERSION}.jar
DEBUG_URL="https://repo1.maven.org/maven2/com/microsoft/java/com.microsoft.java.debug.plugin/${DEBUG_VERSION}/${DEBUG_FILE}"
DEBUG_SHA256=4f4778d452a6a0665536f43ce4e32403a24be6593336b80dc85a322912859e24
# 运行时只给 jdtls 自己跑（项目编译用你机器上的 JDK：jdtls 自己用 `java_home -X` 认得到，lspsvc::jdtls::init_options 那段注释）。挑法和量出来的数在 IDEA.md 第 10 节：
# java.se 整套（项目找不到匹配的 JDK 时 jdtls 拿它当类库，缺一块就是假报错）+ jdtls 跑起来要的 + 第 5 步调试要的（jdi / jdwp / attach）
MODULES=java.se,jdk.unsupported,jdk.zipfs,jdk.crypto.ec,jdk.charsets,jdk.management,jdk.jdi,jdk.jdwp.agent,jdk.attach,jdk.net,jdk.naming.dns,jdk.security.auth,jdk.xml.dom,jdk.jfr

OUT=src-tauri/java-bundle
CACHE="${HOME}/Library/Caches/lite-ide-build"

host=$(uname -m); [ "${host}" = arm64 ] && host=aarch64
ARCHES=("$@"); [ ${#ARCHES[@]} -eq 0 ] && ARCHES=("${host}")

JDK="${LITE_JDK21:-$(/usr/libexec/java_home -v 21 2>/dev/null || true)}"
[ -n "${JDK}" ] && [ -x "${JDK}/bin/jlink" ] || { echo "没有 JDK 21（要用它的 jlink 裁运行时）：brew install --cask temurin@21" >&2; exit 1; }
jver() { sed -n 's/^JAVA_VERSION="\(.*\)"/\1/p' "$1/release"; }
VER=$(jver "${JDK}")

# 热替换的 agent（docs/IDEA.md 第 26 节）：我们自己的 Java 源码，打包时编。源码一改 stamp 就变、整份重来
AGENT_SRC=src-tauri/crates/hotswap/agent
AGENT_SUM=$(find "${AGENT_SRC}" -name '*.java' | sort | xargs cat | shasum -a 256 | cut -c1-16)
stamp="jdtls=${JDTLS_VERSION} ${JDTLS_SHA256} debug=${DEBUG_VERSION} agent=${AGENT_SUM} java=${VER} modules=${MODULES} arches=${ARCHES[*]}"
if [ -f "${OUT}/.stamp" ] && [ "$(cat "${OUT}/.stamp")" = "${stamp}" ]; then
  echo "java-bundle：没变（${stamp}）"
  exit 0
fi

# ── jdtls ──
mkdir -p "${CACHE}"
tgz="${CACHE}/${JDTLS_FILE}"
if [ ! -f "${tgz}" ] || [ "$(shasum -a 256 "${tgz}" | cut -d' ' -f1)" != "${JDTLS_SHA256}" ]; then
  echo "下载 jdtls ${JDTLS_VERSION}（约 49 MB）：${JDTLS_URL}"
  curl -fL --proto =https --proto-redir =https -o "${tgz}.download" "${JDTLS_URL}"
  got=$(shasum -a 256 "${tgz}.download" | cut -d' ' -f1)
  [ "${got}" = "${JDTLS_SHA256}" ] || { rm -f "${tgz}.download"; echo "jdtls 的 sha256 对不上：${got}" >&2; exit 1; }
  mv "${tgz}.download" "${tgz}"
fi

dbg="${CACHE}/${DEBUG_FILE}"
if [ ! -f "${dbg}" ] || [ "$(shasum -a 256 "${dbg}" | cut -d' ' -f1)" != "${DEBUG_SHA256}" ]; then
  echo "下载 java-debug ${DEBUG_VERSION}（约 3 MB）：${DEBUG_URL}"
  curl -fL --proto '=https' --proto-redir '=https' -o "${dbg}.download" "${DEBUG_URL}"
  got=$(shasum -a 256 "${dbg}.download" | cut -d' ' -f1)
  [ "${got}" = "${DEBUG_SHA256}" ] || { rm -f "${dbg}.download"; echo "java-debug 的 sha256 对不上：${got}" >&2; exit 1; }
  mv "${dbg}.download" "${dbg}"
fi

rm -rf "${OUT}.partial"
mkdir -p "${OUT}.partial/jdtls"
tar -xzf "${tgz}" -C "${OUT}.partial/jdtls"
# jdtls 自带的 Python 启动脚本和别的系统的配置用不上（我们自己拼命令行，只在 macOS 上跑），不进包
rm -rf "${OUT}.partial/jdtls/bin" "${OUT}.partial/jdtls"/config_{linux,linux_arm,win,ss_linux,ss_linux_arm,ss_win,ss_mac,ss_mac_arm}

mkdir -p "${OUT}.partial/java-debug"
cp "${dbg}" "${OUT}.partial/java-debug/"

# ── 热替换的 agent：--release 8，被调试的项目可能还在 Java 8 上（Instrumentation 从 5 就有）──
mkdir -p "${OUT}.partial/hotswap/classes"
"${JDK}/bin/javac" --release 8 -Xlint:-options -d "${OUT}.partial/hotswap/classes" $(find "${AGENT_SRC}" -name '*.java')
printf 'Premain-Class: liteide.hotswap.Agent\nCan-Redefine-Classes: true\n' > "${OUT}.partial/hotswap/MANIFEST.MF"
"${JDK}/bin/jar" cfm "${OUT}.partial/hotswap/lite-hotswap.jar" "${OUT}.partial/hotswap/MANIFEST.MF" -C "${OUT}.partial/hotswap/classes" .
rm -rf "${OUT}.partial/hotswap/classes" "${OUT}.partial/hotswap/MANIFEST.MF"

# ── 运行时：一个架构一份 ──
for arch in "${ARCHES[@]}"; do
  var="LITE_JDK21_$(echo "${arch}" | tr '[:lower:]' '[:upper:]')"
  src="${!var:-}"
  [ -z "${src}" ] && [ "${arch}" = "${host}" ] && src="${JDK}"
  [ -n "${src}" ] && [ -d "${src}/jmods" ] || { echo "没有 ${arch} 的 JDK 21 jmods：设 ${var}=<那个 JDK 的 Home>" >&2; exit 1; }
  [ "$(jver "${src}")" = "${VER}" ] || { echo "${var} 是 $(jver "${src}")，跑 jlink 的是 ${VER}：jlink 只认同版本的 jmods" >&2; exit 1; }
  rt="${OUT}.partial/runtime-${arch}"
  "${JDK}/bin/jlink" --module-path "${src}/jmods" --add-modules "${MODULES}" \
    --strip-debug --no-man-pages --no-header-files --compress zip-6 --output "${rt}.links" 2>&1 | grep -v 'incubator' || true
  # legal/ 里每个模块的许可证都是指向 java.base 的软链（jlink 的做法）。许可证要留（GPLv2 + Classpath Exception），
  # 软链换成实体：打包时资源拷贝对软链的处理靠不住
  cp -RL "${rt}.links" "${rt}"
  rm -rf "${rt}.links"
  "${rt}/bin/java" -version >/dev/null 2>&1 || [ "${arch}" != "${host}" ] || { echo "裁出来的 ${arch} 运行时起不来" >&2; exit 1; }
done

# jlink 产出的不少文件是只读的（lib/modules、legal/ 底下的）。tauri-build 每次构建把资源拷进 target/，第二次要覆盖上一次拷的那份 ——
# 只读的盖不了，报一句 `Permission denied (os error 13)`、整个构建失败（2026-10-15：第一次打包成功、第二次就挂）。给自己补上写权限
chmod -R u+w "${OUT}.partial"
[ -d "${OUT}" ] && chmod -R u+w "${OUT}"
rm -rf "${OUT}"
mv "${OUT}.partial" "${OUT}"
echo "${stamp}" > "${OUT}/.stamp"
du -sh "${OUT}"/* | sed 's|^|java-bundle：|'
