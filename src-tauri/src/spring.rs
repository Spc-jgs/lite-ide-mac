//! Spring 的两样导航（docs/IDEA.md 第 25 节，IDEA Ultimate 的 Endpoints 和配置跳转；Lithe-IDEA 也做了）：
//! - **接口**：`/api/orders/{id}` → 处理它的 Controller 方法。随处搜索（⇧⇧）里的「接口」范围
//! - **配置**：`@Value("${demo.greeting}")` 上 ⌘B → `application.yml` 里那个键（每个 profile 的文件都列）
//!
//! 都不靠语言服务器（docs/IDEA.md 第 3 节「秒开不等」）：读文本。Java 用 `javalite` 的分词（认得出注解和第几层括号），
//! YAML / properties 按行读。认不全的照实说：路径写成常量的显示 `{常量名}`、不猜它的值；自定义的组合注解、继承来的映射不认（Lithe 也不认）。

use crate::javalite::{after_type, tokens, Tok};

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    /// GET / POST / … ；`@RequestMapping` 没写 method 的是 ANY
    pub methods: Vec<String>,
    pub path: String,
    pub controller: String,
    pub method: String,
    /// 方法名那一行，从 1 数
    pub line: u32,
}

/// 注解的参数按顶层逗号切开（字符串、花括号、圆括号里的逗号不算）
fn split_top(args: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut q) = (Vec::new(), String::new(), 0i32, None::<char>);
    let mut chars = args.chars().peekable();
    while let Some(ch) = chars.next() {
        match q {
            Some(open) => {
                cur.push(ch);
                if ch == '\\' {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                } else if ch == open {
                    q = None;
                }
            }
            None => match ch {
                '"' | '\'' => {
                    q = Some(ch);
                    cur.push(ch);
                }
                '{' | '(' => {
                    depth += 1;
                    cur.push(ch);
                }
                '}' | ')' => {
                    depth -= 1;
                    cur.push(ch);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            },
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// 一个路径值的原文 → 路径：`"/a"`、`{"/a", "/b"}`、`BASE + "/x"`（常量显示成 `{BASE}`，不猜它的值）
fn path_values(v: &str) -> Vec<String> {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
        return split_top(inner).iter().flat_map(|x| path_values(x)).collect();
    }
    let mut out = String::new();
    for part in v.split('+') {
        let p = part.trim();
        if let Some(s) = p.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            out.push_str(s);
        } else if !p.is_empty() {
            out.push_str(&format!("{{{}}}", p.rsplit('.').next().unwrap_or(p)));
        }
    }
    vec![out]
}

/// `@XxxMapping(…)` 的路径们：`value =` / `path =`，或者只写一个值（不带名字）。没写路径是一个空串（就是前缀本身）
fn mapping_paths(args: &str) -> Vec<String> {
    let parts = split_top(args);
    let named = |k: &str| parts.iter().find_map(|p| p.split_once('=').filter(|(n, _)| n.trim() == k).map(|(_, v)| v.to_string()));
    let raw = if parts.len() == 1 && !parts[0].contains('=') { Some(parts[0].clone()) } else { named("value").or_else(|| named("path")) };
    match raw {
        Some(r) => path_values(&r),
        None => vec![String::new()],
    }
}

/// 哪几种 HTTP 方法。`@RequestMapping` 看 `method = RequestMethod.GET` / `{GET, POST}`，没写是 ANY
fn http_methods(name: &str, args: &str) -> Option<Vec<String>> {
    let fixed = match name {
        "GetMapping" => "GET",
        "PostMapping" => "POST",
        "PutMapping" => "PUT",
        "DeleteMapping" => "DELETE",
        "PatchMapping" => "PATCH",
        "RequestMapping" => {
            let m = split_top(args).into_iter().find_map(|p| p.split_once('=').filter(|(n, _)| n.trim() == "method").map(|(_, v)| v.to_string()));
            return Some(match m {
                Some(v) => {
                    let v = v.trim().trim_start_matches('{').trim_end_matches('}');
                    v.split(',').map(|x| x.trim().rsplit('.').next().unwrap_or("").to_string()).filter(|x| !x.is_empty()).collect()
                }
                None => vec!["ANY".into()],
            });
        }
        _ => return None,
    };
    Some(vec![fixed.into()])
}

/// 前缀和方法上的那段拼起来：开头一个 `/`、中间不出现 `//`、末尾不带 `/`（根是 `/`）
fn join(prefix: &str, path: &str) -> String {
    let s = format!("/{}/{}", prefix.trim_matches('/'), path.trim_matches('/'));
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '/' && out.ends_with('/') {
            continue;
        }
        out.push(ch);
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// 一份 Java 源码里的接口：顶层的 `@RestController` / `@Controller` 类，类上 `@RequestMapping` 是前缀，第一层方法上的映射注解各是一个
pub fn endpoints(java: &str) -> Vec<Endpoint> {
    let toks = tokens(java);
    let mut out = Vec::new();
    let mut pending: Vec<&Tok> = Vec::new();
    let mut controller: Option<(String, Vec<String>)> = None;
    for (i, t) in toks.iter().enumerate() {
        if t.args.is_some() {
            // 层次卡两道（这里只收第 0、1 层的注解，下面只认第 1 层的方法）：方法体里匿名类上的映射不算接口。只拆一道测试不红，两道都拆才红（验过）
            if t.paren == 0 && t.depth <= 1 {
                pending.push(t);
            }
            continue;
        }
        if t.paren != 0 {
            continue;
        }
        // 顶层的类：上面挂着 Controller 才算；前缀来自类上的 @RequestMapping
        if t.depth == 0 && t.ident && (t.text == "class" || t.text == "interface") {
            let is_ctl = pending.iter().any(|a| a.text == "RestController" || a.text == "Controller");
            let prefixes = pending.iter().find(|a| a.text == "RequestMapping").map(|a| mapping_paths(a.args.as_deref().unwrap_or(""))).unwrap_or_else(|| vec![String::new()]);
            controller = if is_ctl { toks.get(i + 1).map(|n| (n.text.clone(), prefixes)) } else { None };
            pending.clear();
            continue;
        }
        // 第一层的方法：名字后面紧跟 `(`、前面是类型
        if t.depth == 1 && t.ident && toks.get(i + 1).is_some_and(|n| n.text == "(") && i > 0 && after_type(&toks[i - 1]) {
            if let Some((ctl, prefixes)) = &controller {
                for a in &pending {
                    let args = a.args.as_deref().unwrap_or("");
                    let Some(methods) = http_methods(&a.text, args) else { continue };
                    for p in prefixes {
                        for m in mapping_paths(args) {
                            out.push(Endpoint { methods: methods.clone(), path: join(p, &m), controller: ctl.clone(), method: t.text.clone(), line: t.line });
                        }
                    }
                }
            }
            pending.clear();
            continue;
        }
        // 一句声明完了（字段、方法体开头）：前面攒的注解不归后面的
        if t.text == ";" || t.text == "{" || t.text == "}" {
            pending.clear();
        }
    }
    out
}

/// Spring 的宽松绑定：`greetingText`、`greeting-text`、`greeting_text`、`GREETING_TEXT` 是同一个键 —— 比的时候小写、去掉 `-` 和 `_`
pub fn norm_key(k: &str) -> String {
    k.chars().filter(|c| *c != '-' && *c != '_').flat_map(char::to_lowercase).collect()
}

/// YAML 里每个键的完整名字和行号：按缩进拼（`demo:` 下面的 `greeting:` 是 `demo.greeting`）。`a.b: x` 一行写的也认；
/// `---` 分开的几份各自从头拼；列表项（`- `）和它下面的不算键；注释、空行跳过
pub fn yaml_keys(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut in_list: Option<usize> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if line.starts_with("---") {
            stack.clear();
            in_list = None;
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if let Some(li) = in_list {
            if indent > li || (indent == li && trimmed.starts_with("- ")) {
                continue;
            }
            in_list = None;
        }
        if trimmed.starts_with("- ") || trimmed == "-" {
            in_list = Some(indent);
            continue;
        }
        let Some((k, _)) = trimmed.split_once(':') else { continue };
        let k = k.trim().trim_matches(|c| c == '"' || c == '\'');
        if k.is_empty() || k.contains(' ') {
            continue;
        }
        while stack.last().is_some_and(|(i, _)| *i >= indent) {
            stack.pop();
        }
        let full = stack.iter().map(|(_, s)| s.as_str()).chain(std::iter::once(k)).collect::<Vec<_>>().join(".");
        out.push((full, n as u32 + 1));
        stack.push((indent, k.to_string()));
    }
    out
}

/// properties 里每个键和行号：`a.b=x` / `a.b: x` / `a.b x`；注释（`#` `!`）、续行跳过
pub fn properties_keys(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut cont = false;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let was_cont = cont;
        cont = line.ends_with('\\');
        if was_cont || line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let end = line.find(['=', ':', ' ', '\t']).unwrap_or(line.len());
        let k = line[..end].trim();
        if !k.is_empty() {
            out.push((k.to_string(), n as u32 + 1));
        }
    }
    out
}

/// 是不是 Spring Boot 的配置文件：`application*.yml|yaml|properties`、`bootstrap*` 同样
pub fn is_config_file(name: &str) -> bool {
    (name.starts_with("application") || name.starts_with("bootstrap")) && (name.ends_with(".yml") || name.ends_with(".yaml") || name.ends_with(".properties"))
}

/// 一份配置文件里这个键在哪几行（宽松绑定）。`${key:默认值}` 由调用方先去掉默认值
pub fn find_key(name: &str, text: &str, key: &str) -> Vec<u32> {
    let want = norm_key(key);
    let keys = if name.ends_with(".properties") { properties_keys(text) } else { yaml_keys(text) };
    keys.into_iter().filter(|(k, _)| norm_key(k) == want).map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTL: &str = r#"package com.demo.order.web;

import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/orders")
public class OrderController {
    private static final String BASE = "/v2";

    @Autowired
    private OrderService svc;

    @GetMapping("/{id}")
    public Order get(@PathVariable Long id) { return svc.find(id); }

    @PostMapping
    public Order create(@RequestBody Order o) { return svc.save(o); }

    @RequestMapping(value = {"/search", "/find"}, method = {RequestMethod.GET, RequestMethod.POST})
    public List<Order> search(@RequestParam("q") String q) { return List.of(); }

    @org.springframework.web.bind.annotation.DeleteMapping(path = BASE + "/{id}")
    public void delete(@PathVariable Long id) {
        Runnable r = new Runnable() {
            @GetMapping("/nope") public void run() {}
        };
    }

    @RequestMapping("/any/")
    public String any() { return "x"; }

    private void helper() {}
}
"#;

    #[test]
    fn 接口_前缀拼上方法的_几种写法都认_常量照原名_匿名类里的不算() {
        let e = endpoints(CTL);
        let got: Vec<(String, String, String, u32)> = e.iter().map(|x| (x.methods.join(","), x.path.clone(), x.method.clone(), x.line)).collect();
        assert_eq!(
            got,
            vec![
                ("GET".into(), "/api/orders/{id}".into(), "get".into(), 14),
                ("POST".into(), "/api/orders".into(), "create".into(), 17),
                ("GET,POST".into(), "/api/orders/search".into(), "search".into(), 20),
                ("GET,POST".into(), "/api/orders/find".into(), "search".into(), 20),
                ("DELETE".into(), "/api/orders/{BASE}/{id}".into(), "delete".into(), 23),
                ("ANY".into(), "/api/orders/any".into(), "any".into(), 30),
            ]
        );
        assert!(e.iter().all(|x| x.controller == "OrderController"));
        assert!(endpoints("@Service public class S { @GetMapping(\"/x\") public void f() {} }").is_empty(), "不是 Controller 的类不算");
        assert_eq!(endpoints("@Controller public class H { @GetMapping public String home() { return \"i\"; } }")[0].path, "/", "什么都没写是根");
    }

    #[test]
    fn yaml_按缩进拼出完整的键_一行写的点号也认_列表和注释不算_多份文档各自从头() {
        let y = "# 注释\ndemo:\n  greeting: hello\n  greeting-text: hi\n  servers:\n    - name: a\n      port: 1\n  timeout: 3\nspring.datasource.url: jdbc:x\n---\nspring:\n  config:\n    activate:\n      on-profile: dev\ndemo:\n  greeting: 你好\n";
        let k = yaml_keys(y);
        let names: Vec<&str> = k.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["demo", "demo.greeting", "demo.greeting-text", "demo.servers", "demo.timeout", "spring.datasource.url", "spring", "spring.config", "spring.config.activate", "spring.config.activate.on-profile", "demo", "demo.greeting"]);
        assert_eq!(find_key("application.yml", y, "demo.greeting"), vec![3, 16], "两份文档各有一处");
        assert_eq!(find_key("application.yml", y, "demo.greetingText"), vec![4], "宽松绑定：驼峰对得上 kebab");
        assert_eq!(find_key("application.yml", y, "demo.name"), Vec::<u32>::new(), "列表项里的不算键");
    }

    #[test]
    fn properties_的键_几种分隔符_注释和续行跳过() {
        let p = "# c\n! c\ndemo.greeting=hello\ndemo.long = a \\\n   still.not.a.key=x\nserver.port: 8080\nspring.application.name demo\n";
        assert_eq!(properties_keys(p).iter().map(|(k, l)| format!("{k}:{l}")).collect::<Vec<_>>(), vec!["demo.greeting:3", "demo.long:4", "server.port:6", "spring.application.name:7"]);
        assert!(is_config_file("application-dev.yml") && is_config_file("bootstrap.properties") && !is_config_file("logback.xml"));
    }
}
