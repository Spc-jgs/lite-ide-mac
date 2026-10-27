//! Spring 注入点 → Bean 的跳转（docs/TAKEOVER.md G30 的 Bean 导航，IDEA Ultimate 行号旁那个「跳到被注入的 Bean」）：
//! `@Autowired private OrderService orders;` 旁边一个图标，点了跳到 `@Service class OrderServiceImpl implements OrderService`。
//!
//! **不靠语言服务器**（同 MyBatis 跳转，docs/IDEA.md 第 3 节「秒开不等」）：读文本，用 `javalite` 的分词。认的是 Spring 项目里最常见的写法：
//! - 注入点：带 `@Autowired` / `@Resource` / `@Inject` 的字段；Bean 类的构造器参数；Lombok `@RequiredArgsConstructor` / `@AllArgsConstructor` 的 Bean 类里非 static 的 `final` 字段
//! - Bean：`@Component` / `@Service` / `@Repository` / `@Controller` / `@RestController` / `@Configuration` 的类（类名就是那个类型、或者直接
//!   `implements` / `extends` 它）；`@Bean` 方法（返回类型是它）；MyBatis 的 `@Mapper` 接口
//!
//! 不认的照实不画（画一个点了不对的图标比不画糟）：只按简单类名配（同名不同包的会一起列出来，让人挑）；隔了一层的继承、`@Qualifier` 的挑选、
//! 组件扫描范围、`@ConditionalOn…` 都不算 —— 那要真的跑一遍 Spring 的容器。测试代码（`src/test`）里的 Bean 不列
//!
//! 项目里的 Bean 按文件的（路径, 改动时间）缓存：第一次把项目的 `.java` 读一遍（先按字面筛：不含那几个注解名的不分词），之后只重读改过的

use crate::javalite::{after_type, tokens, Tok};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

const STEREOTYPES: [&str; 6] = ["Component", "Service", "Repository", "Controller", "RestController", "Configuration"];
const INJECT: [&str; 3] = ["Autowired", "Resource", "Inject"];
const MODIFIERS: [&str; 8] = ["private", "protected", "public", "final", "static", "transient", "volatile", "abstract"];

/// 一个 Bean：跳过去的地方
#[derive(Clone, Debug, PartialEq)]
pub struct Bean {
    /// 界面上的名字：类名，或者 `配置类.方法()`
    pub label: String,
    /// 类名（类 / `@Mapper`）或者返回类型（`@Bean` 方法）—— 按它配
    pub ty: String,
    /// 类直接 `implements` / `extends` 的（简单类名）
    pub supers: Vec<String>,
    /// 从 1 数
    pub line: u32,
}

/// 一个注入点：在哪一行、要的是什么类型（简单类名）
#[derive(Clone, Debug, PartialEq)]
pub struct Inject {
    pub ty: String,
    pub line: u32,
}

/// `toks[i]` 开始的一个类型（`List<Foo>`、`a.b.Foo`、`Foo[]`）：返回简单类名（泛型的外层，`Map<K, V>` 给 `Map`）和类型之后的下标
fn read_type(toks: &[Tok], mut i: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    while let Some(t) = toks.get(i) {
        if !t.ident {
            break;
        }
        name = t.text.clone();
        i += 1;
        if toks.get(i).is_some_and(|x| x.text == ".") {
            i += 1;
            continue;
        }
        break;
    }
    if name.is_empty() {
        return None;
    }
    if toks.get(i).is_some_and(|x| x.text == "<") {
        let mut d = 0;
        while let Some(t) = toks.get(i) {
            match t.text.as_str() {
                "<" => d += 1,
                ">" => d -= 1,
                _ => {}
            }
            i += 1;
            if d == 0 {
                break;
            }
        }
    }
    while toks.get(i).is_some_and(|x| x.text == "[" || x.text == "]") {
        i += 1;
    }
    Some((name, i))
}

/// 类型声明的头：顶层（花括号第 0 层）的 `class` / `interface` / `enum` / `record` + 名字，和它前面的注解、后面直到 `{` 的 extends / implements
struct Head {
    name: String,
    kind: String,
    anns: Vec<String>,
    supers: Vec<String>,
    line: u32,
}

fn head(toks: &[Tok]) -> Option<Head> {
    let mut anns = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.depth != 0 {
            continue;
        }
        if t.args.is_some() {
            anns.push(t.text.clone());
            continue;
        }
        if t.text == ";" {
            anns.clear(); // package / import 之后
            continue;
        }
        if !["class", "interface", "enum", "record"].contains(&t.text.as_str()) {
            continue;
        }
        let name = toks.get(i + 1).filter(|n| n.ident)?;
        let mut supers = Vec::new();
        let mut j = i + 2;
        let mut collecting = false;
        while let Some(x) = toks.get(j) {
            if x.text == "{" {
                break;
            }
            if x.text == "extends" || x.text == "implements" {
                collecting = true;
                j += 1;
                continue;
            }
            if collecting && x.ident {
                if let Some((n, next)) = read_type(toks, j) {
                    supers.push(n);
                    j = next;
                    continue;
                }
            }
            j += 1;
        }
        return Some(Head { name: name.text.clone(), kind: t.text.clone(), anns, supers, line: name.line });
    }
    None
}

/// 一份源码里声明的 Bean：Bean 类本身、`@Mapper` 接口、`@Bean` 方法
pub fn beans(java: &str) -> Vec<Bean> {
    // 先按字面筛：Bean 文件里一定有这几个注解名之一，别的文件不分词（一个项目几千个 .java，大多数不是）
    if !STEREOTYPES.iter().chain(["Mapper", "Bean"].iter()).any(|a| java.contains(&format!("@{a}"))) {
        return Vec::new();
    }
    let toks = tokens(java);
    let Some(h) = head(&toks) else { return Vec::new() };
    let mut out = Vec::new();
    let stereo = h.anns.iter().any(|a| STEREOTYPES.contains(&a.as_str()));
    if (stereo && h.kind != "interface") || (h.kind == "interface" && h.anns.iter().any(|a| a == "Mapper")) {
        out.push(Bean { label: h.name.clone(), ty: h.name.clone(), supers: h.supers.clone(), line: h.line });
    }
    // @Bean 方法：类体第一层、注解 Bean 之后的第一个「类型 名字(」
    let mut pending = false;
    for (i, t) in toks.iter().enumerate() {
        if t.depth != 1 || t.paren != 0 {
            continue;
        }
        if t.args.is_some() {
            pending |= t.text == "Bean";
            continue;
        }
        if !pending {
            continue;
        }
        if t.text == ";" || t.text == "{" {
            pending = false;
            continue;
        }
        if t.ident && toks.get(i + 1).is_some_and(|n| n.text == "(") && i.checked_sub(1).is_some_and(|p| after_type(&toks[p])) {
            // 往回找返回类型的起点：名字前面是 `]`（数组）就跳过括号，是 `>`（泛型）就跳回配对的 `<`，再跨过全名里的 `a.b.`
            let start = (|| {
                let mut k = i.checked_sub(1)?;
                while toks[k].text == "]" || toks[k].text == "[" {
                    k = k.checked_sub(1)?;
                }
                if toks[k].text == ">" {
                    let mut d = 0;
                    loop {
                        match toks[k].text.as_str() {
                            ">" => d += 1,
                            "<" => d -= 1,
                            _ => {}
                        }
                        if d == 0 {
                            break;
                        }
                        k = k.checked_sub(1)?;
                    }
                    k = k.checked_sub(1)?;
                }
                while k >= 2 && toks[k - 1].text == "." && toks[k - 2].ident {
                    k -= 2;
                }
                toks[k].ident.then_some(k)
            })();
            if let Some((ty, _)) = start.and_then(|s| read_type(&toks, s)) {
                out.push(Bean { label: format!("{}.{}()", h.name, t.text), ty, supers: Vec::new(), line: t.line });
            }
            pending = false;
        }
    }
    out
}

/// 一份源码里的注入点
pub fn injects(java: &str) -> Vec<Inject> {
    let toks = tokens(java);
    let Some(h) = head(&toks) else { return Vec::new() };
    let bean_class = h.anns.iter().any(|a| STEREOTYPES.contains(&a.as_str()));
    let lombok = h.anns.iter().any(|a| a == "RequiredArgsConstructor" || a == "AllArgsConstructor");
    let mut out = Vec::new();
    let mut annotated = false;
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if t.depth != 1 || t.paren != 0 {
            i += 1;
            continue;
        }
        if t.args.is_some() {
            annotated |= INJECT.contains(&t.text.as_str());
            i += 1;
            continue;
        }
        // 构造器：`类名(` —— Bean 类的构造器参数都是注入点
        if t.ident && t.text == h.name && toks.get(i + 1).is_some_and(|n| n.text == "(") {
            if bean_class {
                let mut j = i + 2;
                while let Some(x) = toks.get(j) {
                    if x.paren == 0 {
                        break;
                    }
                    if x.ident && x.paren == 1 && !MODIFIERS.contains(&x.text.as_str()) && x.args.is_none() {
                        if let Some((ty, next)) = read_type(&toks, j) {
                            if toks.get(next).is_some_and(|n| n.ident) {
                                out.push(Inject { ty, line: x.line });
                                j = next + 1;
                                continue;
                            }
                        }
                    }
                    j += 1;
                }
            }
            annotated = false;
            i += 2;
            continue;
        }
        // 字段：一条到 `;` 为止的声明（中间没有 `(`：不是方法）
        if t.ident {
            let mut j = i;
            let mut mods = Vec::new();
            while toks.get(j).is_some_and(|x| MODIFIERS.contains(&x.text.as_str())) {
                mods.push(toks[j].text.as_str());
                j += 1;
            }
            if let Some((ty, next)) = read_type(&toks, j) {
                let name = toks.get(next).filter(|n| n.ident);
                let end = toks.get(next + 1).map(|n| n.text.as_str());
                if name.is_some() && matches!(end, Some(";") | Some("=")) && j == i + mods.len() {
                    let lombok_field = bean_class && lombok && mods.contains(&"final") && !mods.contains(&"static");
                    if annotated || lombok_field {
                        out.push(Inject { ty, line: name.map_or(t.line, |n| n.line) });
                    }
                    annotated = false;
                    // 跳到这条声明的 `;`
                    let mut k = next + 1;
                    while toks.get(k).is_some_and(|x| !(x.text == ";" && x.depth == 1 && x.paren == 0)) {
                        k += 1;
                    }
                    i = k + 1;
                    continue;
                }
            }
            if toks.get(i + 1).is_some_and(|n| n.text == "(") || t.text == "{" {
                annotated = false;
            }
        }
        if t.text == ";" || t.text == "{" || t.text == "}" {
            annotated = false;
        }
        i += 1;
    }
    out
}

/// 一个类型（简单类名）的候选 Bean：类名就是它的、直接实现 / 继承它的、返回它的 `@Bean` 方法
pub fn matches(b: &Bean, ty: &str) -> bool {
    b.ty == ty || b.supers.iter().any(|s| s == ty)
}

type Cached = (Option<SystemTime>, Vec<Bean>);
static CACHE: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);

/// 项目里全部的 Bean（绝对路径 → 那份文件里的）。`files` 是项目文件（相对路径）；测试代码不算
pub fn project_beans(root: &Path, files: &[String]) -> Vec<(PathBuf, Vec<Bean>)> {
    let mut out = Vec::new();
    for rel in files.iter().filter(|f| f.ends_with(".java") && !f.contains("src/test/")) {
        let p = root.join(rel);
        let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
        let hit = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&p)).filter(|(t, _)| *t == mtime).map(|(_, b)| b.clone());
        let bs = match hit {
            Some(b) => b,
            None => {
                // 2 MB 以上的不是手写的源码（同 MyBatis 那边的上限）
                let b = std::fs::metadata(&p).ok().filter(|m| m.len() <= 2 << 20).and_then(|_| std::fs::read_to_string(&p).ok()).map(|t| beans(&t)).unwrap_or_default();
                CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(p.clone(), (mtime, b.clone()));
                b
            }
        };
        if !bs.is_empty() {
            out.push((p, bs));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE: &str = r#"package com.demo.order;

import org.springframework.stereotype.Service;

@Service
@Transactional(readOnly = true)
public class OrderServiceImpl extends BaseService<Order> implements OrderService, java.io.Serializable {
    @Override
    public Order find(long id) { return null; }
}
"#;

    const CONFIG: &str = r#"package com.demo.cfg;

@Configuration
public class AppConfig {
    @Bean
    public RestTemplate restTemplate() { return new RestTemplate(); }

    @Bean(name = "pool")
    @Primary
    static Map<String, Executor> executors(Env env) { return null; }

    public String notABean() { return "x"; }
}
"#;

    const CONTROLLER: &str = r#"package com.demo.web;

@RestController
@RequiredArgsConstructor
public class OrderController {
    private final OrderService orders;
    private static final Logger LOG = null;
    private final List<String> names = new ArrayList<>();

    @Autowired
    private RestTemplate rest;

    @Resource(name = "pool") private Map<String, Executor> executors;

    private int counter;

    public OrderController(OrderMapper mapper, @Qualifier("x") Clock clock) {}

    @GetMapping("/a")
    public String get(@RequestParam String id) { Foo foo = null; return id; }
}
"#;

    #[test]
    fn bean_类_接口_mapper_和_bean_方法() {
        let s = beans(SERVICE);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].label.as_str(), s[0].line), ("OrderServiceImpl", 7));
        assert_eq!(s[0].supers, vec!["BaseService", "OrderService", "Serializable"], "泛型跳过、全名取最后一段");
        let c = beans(CONFIG);
        let got: Vec<(&str, &str, u32)> = c.iter().map(|b| (b.label.as_str(), b.ty.as_str(), b.line)).collect();
        assert_eq!(got, vec![("AppConfig", "AppConfig", 4), ("AppConfig.restTemplate()", "RestTemplate", 6), ("AppConfig.executors()", "Map", 10)], "带参数的 @Bean、static、多个注解都认；不带 @Bean 的方法不算");
        let m = beans("@Mapper\npublic interface OrderMapper {\n  Order find(long id);\n}\n");
        assert_eq!(m.iter().map(|b| b.label.as_str()).collect::<Vec<_>>(), vec!["OrderMapper"]);
        assert!(beans("public class Pojo { private String a; }").is_empty(), "不是 Bean");
        assert!(beans("@Service\npublic interface OrderService {}").is_empty(), "接口上的 @Service 不是 Bean（Spring 不实例化接口）");
    }

    #[test]
    fn 注入点_注解字段_lombok_final_构造器参数() {
        let got: Vec<(String, u32)> = injects(CONTROLLER).into_iter().map(|i| (i.ty, i.line)).collect();
        assert_eq!(
            got,
            vec![("OrderService".into(), 6), ("List".into(), 8), ("RestTemplate".into(), 11), ("Map".into(), 13), ("OrderMapper".into(), 17), ("Clock".into(), 17)],
            "static final 不算、没注解的普通字段不算、方法参数和方法体里的局部变量不算"
        );
        // 不是 Bean 类：@RequiredArgsConstructor 的 final 字段、构造器参数都不是注入点；@Autowired 的照算
        let pojo = "@Data\n@RequiredArgsConstructor\npublic class Pojo {\n  private final OrderService s;\n  @Autowired OrderMapper m;\n  public Pojo(Clock c) {}\n}\n";
        assert_eq!(injects(pojo).into_iter().map(|i| i.ty).collect::<Vec<_>>(), vec!["OrderMapper"]);
    }

    #[test]
    fn 注入点_验收里的那份() {
        let f = "package com.demo.order;\n\n@Service\n@RequiredArgsConstructor\npublic class OrderFacade {\n    private static final String NAME = \"facade\";\n    private final OrderService orders;\n    private final PayGateway pay;\n    private final OrderMapper mapper;\n    private String note;\n\n    @Autowired\n    private RestTemplate rest;\n}\n";
        let got: Vec<(String, u32)> = injects(f).into_iter().map(|i| (i.ty, i.line)).collect();
        assert_eq!(got, vec![("OrderService".into(), 7), ("PayGateway".into(), 8), ("OrderMapper".into(), 9), ("RestTemplate".into(), 13)]);
    }

    #[test]
    fn 配对_类名一样或者直接实现() {
        let s = &beans(SERVICE)[0];
        assert!(matches(s, "OrderService") && matches(s, "OrderServiceImpl") && matches(s, "BaseService"));
        assert!(!matches(s, "Order"));
    }
}
