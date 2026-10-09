//! 内容指纹：长度 + FNV-1a 64 位哈希。用来回答「这份内容和那时候的还是不是同一份」。
//!
//! **不用 `std::collections::hash_map::DefaultHasher`**：它的算法不保证跨 Rust 版本一致，而替换日志要跨重启、
//! 甚至跨应用升级读回来 —— 升级之后指纹全对不上，撤销会把每个文件都判成「替换之后又改过」。
//! FNV 不是密码学哈希，这里也不需要：比的是「同一个文件有没有被改过」，不防有人故意造碰撞。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fp {
    pub len: u64,
    pub hash: u64,
}

pub fn of(bytes: &[u8]) -> Fp {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Fp { len: bytes.len() as u64, hash: h }
}

#[cfg(test)]
mod tests {
    #[test]
    fn 指纹是固定的() {
        // FNV-1a 64 的公开测试向量：算法一变（比如有人换成 DefaultHasher），这条就红
        assert_eq!(super::of(b"").hash, 0xcbf2_9ce4_8422_2325);
        assert_eq!(super::of(b"a").hash, 0xaf63_dc4c_8601_ec8c);
        assert_ne!(super::of(b"ab"), super::of(b"ba"));
    }
}
