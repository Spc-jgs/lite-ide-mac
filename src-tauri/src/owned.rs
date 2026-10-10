//! 按窗口登记的资源表（rust.md「进程级的资源要记着它属于哪个窗口」）。
//!
//! 原来 `AppState` 里每张这样的表都手写一遍：`Mutex<HashMap<…>>`、owner 放在值里或键里（各表不一样）、`release_window` 里
//! 再手写一遍「过滤出这个窗口的 → 收集键 → 逐个删 → 锁外析构」。rust.md 的说法是「以后再加一张，照同一个形状：登记时记 owner、
//! `release_window` 里加一段、测试里加一组断言」—— **规矩写在文档里，靠每次都记得**（2026-10-10 整体审核时收成这个类型）。
//!
//! 现在 owner 由这张表自己存，`take_owner` 一次摘出这个窗口的全部；`state.rs` 里有一条读源码的测试：
//! `AppState` 里每个 `Owned` 字段，`release_window` 里都得 `take_owner` 过它 —— 新加一张忘了收，测试红。
//!
//! **析构一律在锁外**：会删东西的方法都把删掉的值交回给调用方，调用方在锁放掉之后再处理（`Session::drop` 要杀进程、
//! `Watch::drop` 要等线程 —— 持着锁做这些，卡住的是所有窗口的同类操作，issue #2）。
//! 锁中毒照常用（`into_inner`）：release 是 `panic = abort`，真有线程持锁 panic，进程已经没了。

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, MutexGuard};

pub struct Owned<K, V> {
    map: Mutex<HashMap<K, (String, V)>>,
}

impl<K, V> Default for Owned<K, V> {
    fn default() -> Self {
        Owned { map: Mutex::new(HashMap::new()) }
    }
}

impl<K: Eq + Hash + Clone, V> Owned<K, V> {
    fn lock(&self) -> MutexGuard<'_, HashMap<K, (String, V)>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 登记在 `owner` 名下。同一个键原来那份交回来（调用方在锁外处理）
    pub fn insert(&self, owner: &str, k: K, v: V) -> Option<V> {
        self.lock().insert(k, (owner.to_string(), v)).map(|(_, v)| v)
    }

    /// 摘掉一项，交回给调用方在锁外处理
    pub fn remove(&self, k: &K) -> Option<V> {
        self.lock().remove(k).map(|(_, v)| v)
    }

    /// 在锁里看一眼某一项（取出 `Arc` 之类的克隆）。**别在 `f` 里做慢事**
    pub fn get<R>(&self, k: &K, f: impl FnOnce(&V) -> R) -> Option<R> {
        self.lock().get(k).map(|(_, v)| f(v))
    }

    pub fn get_mut<R>(&self, k: &K, f: impl FnOnce(&mut V) -> R) -> Option<R> {
        self.lock().get_mut(k).map(|(_, v)| f(v))
    }

    /// 找第一个 `f` 说「是」的
    pub fn find<R>(&self, mut f: impl FnMut(&str, &K, &V) -> Option<R>) -> Option<R> {
        self.lock().iter().find_map(|(k, (o, v))| f(o, k, v))
    }

    /// 每一项映射一下（取出要在锁外用的东西）
    pub fn map<R>(&self, mut f: impl FnMut(&V) -> R) -> Vec<R> {
        self.lock().values().map(|(_, v)| f(v)).collect()
    }

    /// 全部摘出来（进程要结束了）
    pub fn drain(&self) -> Vec<V> {
        self.lock().drain().map(|(_, (_, v))| v).collect()
    }

    /// **这个窗口名下的全部摘出来**（窗口关了）。交回给调用方在锁外收尾
    pub fn take_owner(&self, owner: &str) -> Vec<(K, V)> {
        let mut m = self.lock();
        let keys: Vec<K> = m.iter().filter(|(_, (o, _))| o == owner).map(|(k, _)| k.clone()).collect();
        keys.into_iter().filter_map(|k| m.remove(&k).map(|(_, v)| (k, v))).collect()
    }

    /// 要在同一把锁里先查再改的（上限、撞号）：直接拿到底下那张表。用完立刻放
    pub fn with_map<R>(&self, f: impl FnOnce(&mut HashMap<K, (String, V)>) -> R) -> R {
        f(&mut self.lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 按窗口摘_只摘它自己的() {
        let t: Owned<u32, &str> = Owned::default();
        t.insert("a", 1, "a1");
        t.insert("b", 2, "b2");
        t.insert("a", 3, "a3");
        let mut got = t.take_owner("a");
        got.sort();
        assert_eq!(got, [(1, "a1"), (3, "a3")]);
        assert_eq!(t.map(|v| *v), ["b2"], "b 的不能跟着没");
        assert!(t.take_owner("a").is_empty(), "摘过了就没了");
    }

    #[test]
    fn 同一个键再登记_旧的交回来() {
        let t: Owned<u32, &str> = Owned::default();
        assert_eq!(t.insert("a", 1, "old"), None);
        assert_eq!(t.insert("a", 1, "new"), Some("old"));
        assert_eq!(t.get(&1, |v| *v), Some("new"));
    }
}
