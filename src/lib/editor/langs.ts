/**
 * 语言的**识别**：文件名 → 语言 id → 显示名。
 *
 * 只有这一半是入口包需要的（状态栏要显示"Markdown"、大纲要判断支不支持），
 * 而真正加载 parser 的那 500 行 switch 在 [langs-load.ts](./langs-load.ts) ——
 * 它只被编辑器用，而编辑器是懒加载的。
 *
 * 分开之前两半都在一个文件里，于是那 500 行 switch 连同 67 个 `import()`
 * 的桩全都躺在入口包里：sourcemap 归因显示 langs.ts 一家占了 23,419 字节，
 * 是入口包里仅次于 App.svelte 的第二大块。**入口包是首屏之前必须解析执行完
 * 的那一段**，装着一堆只有打开文件才用得上的东西没有道理。
 */

export type LangId =
  | "clojure"
  | "cmake"
  | "cobol"
  | "coffee"
  | "cpp"
  | "crystal"
  | "csharp"
  | "css"
  | "d"
  | "dart"
  | "diff"
  | "dockerfile"
  | "elm"
  | "erlang"
  | "fortran"
  | "gherkin"
  | "go"
  | "groovy"
  | "haskell"
  | "haxe"
  | "html"
  | "http"
  | "java"
  | "javascript"
  | "jinja2"
  | "json"
  | "julia"
  | "kotlin"
  | "less"
  | "liquid"
  | "lisp"
  | "lua"
  | "markdown"
  | "mathematica"
  | "nginx"
  | "objc"
  | "octave"
  | "pascal"
  | "perl"
  | "php"
  | "powershell"
  | "properties"
  | "protobuf"
  | "pug"
  | "python"
  | "r"
  | "ruby"
  | "rust"
  | "sass"
  | "scala"
  | "scheme"
  | "shell"
  | "smalltalk"
  | "sparql"
  | "sql"
  | "stylus"
  | "swift"
  | "tcl"
  | "toml"
  | "turtle"
  | "typescript"
  | "vb"
  | "verilog"
  | "vhdl"
  | "vue"
  | "xml"
  | "yaml"
  | null;

/** 扩展名 → 语言 */
const BY_EXT: Record<string, LangId> = {
  "R": "r",
  "bas": "vb",
  "bash": "shell",
  "c": "cpp",
  "cbl": "cobol",
  "cc": "cpp",
  "cfg": "properties",
  "cjs": "javascript",
  "cl": "lisp",
  "clj": "clojure",
  "cljc": "clojure",
  "cljs": "clojure",
  "cmake": "cmake",
  "cob": "cobol",
  "coffee": "coffee",
  "command": "shell",
  "conf": "properties",
  "cpp": "cpp",
  "cr": "crystal",
  "cs": "csharp",
  "css": "css",
  "csx": "csharp",
  "cts": "typescript",
  "cxx": "cpp",
  "d": "d",
  "dart": "dart",
  "ddl": "sql",
  "diff": "diff",
  "dml": "sql",
  "dockerfile": "dockerfile",
  "edn": "clojure",
  "el": "lisp",
  "elm": "elm",
  "env": "properties",
  "erl": "erlang",
  "f": "fortran",
  "f90": "fortran",
  "f95": "fortran",
  "feature": "gherkin",
  "fish": "shell",
  "for": "fortran",
  "gemspec": "ruby",
  "go": "go",
  "gradle": "groovy",
  "groovy": "groovy",
  "gvy": "groovy",
  "h": "cpp",
  "hh": "cpp",
  "hpp": "cpp",
  "hrl": "erlang",
  "hs": "haskell",
  "htm": "html",
  "html": "html",
  "http": "http",
  "hx": "haxe",
  "ini": "properties",
  "ino": "cpp",
  "ipynb": "json",
  "j2": "jinja2",
  "jade": "pug",
  "java": "java",
  "jinja": "jinja2",
  "jinja2": "jinja2",
  "jl": "julia",
  "js": "javascript",
  "json": "json",
  "json5": "json",
  "jsonc": "json",
  "jsx": "javascript",
  "ksh": "shell",
  "kt": "kotlin",
  "kts": "kotlin",
  "less": "less",
  "lhs": "haskell",
  "liquid": "liquid",
  "lisp": "lisp",
  "lock": "toml",
  "lua": "lua",
  "m": "objc",
  "map": "json",
  "markdown": "markdown",
  "matlab": "octave",
  "md": "markdown",
  "mdx": "markdown",
  "mjs": "javascript",
  "mm": "objc",
  "mts": "typescript",
  "nb": "mathematica",
  "nginxconf": "nginx",
  "pas": "pascal",
  "patch": "diff",
  "php": "php",
  "php3": "php",
  "php4": "php",
  "php5": "php",
  "phtml": "php",
  "pl": "perl",
  "plist": "xml",
  "pm": "perl",
  "pom": "xml",
  "pp": "pascal",
  "properties": "properties",
  "proto": "protobuf",
  "ps1": "powershell",
  "psd1": "powershell",
  "psm1": "powershell",
  "pug": "pug",
  "py": "python",
  "pyi": "python",
  "pyw": "python",
  "r": "r",
  "rake": "ruby",
  "rb": "ruby",
  "rest": "http",
  "rq": "sparql",
  "rs": "rust",
  "sass": "sass",
  "sc": "scala",
  "scala": "scala",
  "scm": "scheme",
  "scss": "sass",
  "sh": "shell",
  "sparql": "sparql",
  "sql": "sql",
  "ss": "scheme",
  "st": "smalltalk",
  "styl": "stylus",
  "sv": "verilog",
  "svg": "xml",
  "svh": "verilog",
  "swift": "swift",
  "tcl": "tcl",
  "toml": "toml",
  "ts": "typescript",
  "tsx": "typescript",
  "ttl": "turtle",
  "v": "verilog",
  "vb": "vb",
  "vbs": "vb",
  "vhd": "vhdl",
  "vhdl": "vhdl",
  "vue": "vue",
  "wl": "mathematica",
  "wsdl": "xml",
  "xml": "xml",
  "xsd": "xml",
  "xsl": "xml",
  "yaml": "yaml",
  "yml": "yaml",
  "zsh": "shell",
};

/** 没有扩展名、但一眼能认出来的文件 */
const BY_NAME: Record<string, LangId> = {
  ".bash_profile": "shell",
  ".bashrc": "shell",
  ".editorconfig": "properties",
  ".env": "properties",
  ".gitconfig": "properties",
  ".profile": "shell",
  ".zprofile": "shell",
  ".zshenv": "shell",
  ".zshrc": "shell",
  "brewfile": "ruby",
  "cargo.lock": "toml",
  "cmakelists.txt": "cmake",
  "dockerfile": "dockerfile",
  "gemfile": "ruby",
  "gnumakefile": "shell",
  "justfile": "shell",
  "makefile": "shell",
  "nginx.conf": "nginx",
  "pnpm-lock.yaml": "yaml",
  "podfile": "ruby",
  "rakefile": "ruby",
  "vagrantfile": "ruby",
};

/** 给状态栏显示的名字 */
const LABELS: Record<string, string> = {
  clojure: "Clojure",
  cmake: "CMake",
  cobol: "COBOL",
  coffee: "CoffeeScript",
  cpp: "C/C++",
  crystal: "Crystal",
  csharp: "C#",
  css: "CSS",
  d: "D",
  dart: "Dart",
  diff: "Diff",
  dockerfile: "Dockerfile",
  elm: "Elm",
  erlang: "Erlang",
  fortran: "Fortran",
  gherkin: "Gherkin",
  go: "Go",
  groovy: "Groovy",
  haskell: "Haskell",
  haxe: "Haxe",
  html: "HTML",
  http: "HTTP",
  java: "Java",
  javascript: "JavaScript",
  jinja2: "Jinja2",
  json: "JSON",
  julia: "Julia",
  kotlin: "Kotlin",
  less: "Less",
  liquid: "Liquid",
  lisp: "Common Lisp",
  lua: "Lua",
  markdown: "Markdown",
  mathematica: "Mathematica",
  nginx: "Nginx",
  objc: "Objective-C",
  octave: "Octave",
  pascal: "Pascal",
  perl: "Perl",
  php: "PHP",
  powershell: "PowerShell",
  properties: "Properties",
  protobuf: "Protobuf",
  pug: "Pug",
  python: "Python",
  r: "R",
  ruby: "Ruby",
  rust: "Rust",
  sass: "Sass",
  scala: "Scala",
  scheme: "Scheme",
  shell: "Shell",
  smalltalk: "Smalltalk",
  sparql: "SPARQL",
  sql: "SQL",
  stylus: "Stylus",
  swift: "Swift",
  tcl: "Tcl",
  toml: "TOML",
  turtle: "Turtle",
  typescript: "TypeScript",
  vb: "Visual Basic",
  verilog: "Verilog",
  vhdl: "VHDL",
  vue: "Vue",
  xml: "XML",
  yaml: "YAML",
};

/** 认出文件用什么语言。认不出来就纯文本，不猜。 */
export function langOf(filename: string): LangId {
  const base = filename.slice(filename.lastIndexOf("/") + 1);
  const lower = base.toLowerCase();

  const byName = BY_NAME[lower];
  if (byName) return byName;

  const dot = base.lastIndexOf(".");
  // 没有点，或点在开头（.gitignore 这种）——上面按整名查过了，这里就认输
  if (dot <= 0) return null;
  return BY_EXT[base.slice(dot + 1).toLowerCase()] ?? null;
}

export function langLabel(id: LangId): string {
  return id ? (LABELS[id] ?? id) : "纯文本";
}
