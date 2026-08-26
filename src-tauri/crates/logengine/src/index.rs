//! 稀疏行索引 —— 本引擎的核心技巧。
//!
//! 不为每一行都存字节偏移，而是每 `stride` 行存一个 checkpoint，块内靠
//! `memchr` 线性扫描补齐。内存账（见 docs/ARCHITECTURE.md §3.2）：
//!
//! | 文件  | 行数     | 全量偏移 | 本方案(stride=1024) |
//! |-------|----------|----------|---------------------|
//! | 1 GB  | 约 800 万 | 64 MB    | 62 KB               |
//! | 10 GB | 约 8000 万| 640 MB   | 620 KB              |
//!
//! 代价是定位任意一行最坏要扫 `stride` 行（约 100KB，约 10μs），完全无感。

/// 每多少行落一个 checkpoint。
///
/// 1024 是内存与扫描代价的平衡点：再大内存省不了多少（已是 KB 级），
/// 再小则 checkpoint 数组本身开始变得可观。
pub const DEFAULT_STRIDE: u32 = 1024;

#[derive(Debug, Clone)]
pub struct LineIndex {
    /// `checkpoints[k]` = 第 `k * stride` 行的起始字节偏移；`checkpoints[0]` 恒为 0。
    checkpoints: Vec<u64>,
    stride: u32,
    /// 已确认的完整行数（不含末尾未以 \n 结束的残行，除非已 seal）。
    line_count: u64,
    /// 增量构建游标：已索引到此字节位置，且此位置总是落在行边界上。
    indexed_upto: u64,
    /// 是否已确认扫到文件末尾（末行残行已计入 line_count）。
    sealed: bool,
}

impl LineIndex {
    pub fn new(stride: u32) -> Self {
        assert!(stride > 0, "stride 必须为正");
        Self {
            checkpoints: vec![0],
            stride,
            line_count: 0,
            indexed_upto: 0,
            sealed: false,
        }
    }

    #[inline]
    pub fn line_count(&self) -> u64 {
        self.line_count
    }

    #[inline]
    pub fn indexed_upto(&self) -> u64 {
        self.indexed_upto
    }

    #[inline]
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 索引自身占用的字节数 —— 用来验证「内存与文件大小无关」这句承诺。
    #[inline]
    pub fn memory_footprint(&self) -> usize {
        self.checkpoints.capacity() * std::mem::size_of::<u64>()
    }

    /// 从 `indexed_upto` 增量扫描到 `data` 末尾的最后一个换行符。
    ///
    /// 停在行边界上：末尾不完整的一行留给下次（tail 追加时它会被补全）。
    /// 返回本次新增的完整行数。
    pub fn extend(&mut self, data: &[u8]) -> u64 {
        let start = self.indexed_upto as usize;
        if start >= data.len() {
            return 0;
        }
        let before = self.line_count;
        let mut line = self.line_count;
        let mut last_nl: Option<usize> = None;

        for nl in memchr::memchr_iter(b'\n', &data[start..]) {
            let abs = start + nl;
            line += 1;
            // 行 `line` 从 abs+1 开始；正好压在 stride 倍数上就落一个 checkpoint
            if line.is_multiple_of(self.stride as u64) {
                debug_assert_eq!(self.checkpoints.len() as u64, line / self.stride as u64);
                self.checkpoints.push(abs as u64 + 1);
            }
            last_nl = Some(abs);
        }

        if let Some(nl) = last_nl {
            self.indexed_upto = nl as u64 + 1;
            self.line_count = line;
        }
        self.line_count - before
    }

    /// 声明文件已到末尾：把末尾那行没有 \n 结尾的残行计入行数。
    ///
    /// 静态文件在索引跑完后调用；tail 模式下不调用。
    pub fn seal(&mut self, data: &[u8]) {
        if self.sealed {
            return;
        }
        if (self.indexed_upto as usize) < data.len() {
            // 末尾还有内容且不以 \n 结束 —— 它是完整的一行
            self.line_count += 1;
            if self.line_count.is_multiple_of(self.stride as u64) {
                self.checkpoints.push(data.len() as u64);
            }
            self.indexed_upto = data.len() as u64;
        }
        // Vec 的倍增策略会留出最多一倍余量，封口时收紧 —— 报出去的数字才诚实
        self.checkpoints.shrink_to_fit();
        self.sealed = true;
    }

    /// 定位第 `line` 行的起始字节偏移。
    ///
    /// O(1) 查 checkpoint + 最坏 `stride` 行的有界扫描。
    pub fn offset_of_line(&self, data: &[u8], line: u64) -> Option<u64> {
        if line >= self.line_count {
            return None;
        }
        let ck = (line / self.stride as u64) as usize;
        let mut pos = *self.checkpoints.get(ck)? as usize;
        let mut remain = line % self.stride as u64;

        while remain > 0 {
            let nl = memchr::memchr(b'\n', data.get(pos..)?)?;
            pos += nl + 1;
            remain -= 1;
        }
        Some(pos as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(data: &[u8], stride: u32) -> LineIndex {
        let mut i = LineIndex::new(stride);
        i.extend(data);
        i.seal(data);
        i
    }

    #[test]
    fn 空文件() {
        let i = idx(b"", 4);
        assert_eq!(i.line_count(), 0);
    }

    #[test]
    fn 末尾有换行() {
        let i = idx(b"a\nb\nc\n", 4);
        assert_eq!(i.line_count(), 3);
    }

    #[test]
    fn 末尾无换行的残行也算一行() {
        let i = idx(b"a\nb\nc", 4);
        assert_eq!(i.line_count(), 3);
    }

    #[test]
    fn 定位每一行都要准() {
        let data = b"aa\nbbb\nc\ndddd\ne\n";
        let i = idx(data, 2);
        let expect = [0u64, 3, 7, 9, 14];
        for (n, &want) in expect.iter().enumerate() {
            assert_eq!(i.offset_of_line(data, n as u64), Some(want), "第 {n} 行");
        }
        assert_eq!(i.offset_of_line(data, 5), None, "越界应返回 None");
    }

    #[test]
    fn 跨越多个_checkpoint_仍然准确() {
        let mut data = Vec::new();
        for n in 0..5000 {
            data.extend_from_slice(format!("line-{n}\n").as_bytes());
        }
        let i = idx(&data, DEFAULT_STRIDE);
        assert_eq!(i.line_count(), 5000);
        for n in [0u64, 1, 1023, 1024, 1025, 4095, 4096, 4999] {
            let off = i.offset_of_line(&data, n).expect("应能定位") as usize;
            let want = format!("line-{n}\n");
            assert!(
                data[off..].starts_with(want.as_bytes()),
                "第 {n} 行定位错误"
            );
        }
    }

    #[test]
    fn 增量扩展等价于一次扫完() {
        let data = b"one\ntwo\nthree\nfour\nfive\n";
        let mut inc = LineIndex::new(2);
        // 逐字节喂进去，模拟 tail 追加：结果必须与一次扫完一致
        for cut in 1..=data.len() {
            inc.extend(&data[..cut]);
        }
        inc.seal(data);
        let full = idx(data, 2);
        assert_eq!(inc.line_count(), full.line_count());
        for n in 0..full.line_count() {
            assert_eq!(
                inc.offset_of_line(data, n),
                full.offset_of_line(data, n),
                "第 {n} 行"
            );
        }
    }

    #[test]
    fn 索引内存必须是_kb_级() {
        // 100 万行 / stride 1024 => 约 977 个 checkpoint => 约 8KB
        let mut data = Vec::with_capacity(12_000_000);
        for n in 0..1_000_000u32 {
            data.extend_from_slice(format!("{n:010}\n").as_bytes());
        }
        let i = idx(&data, DEFAULT_STRIDE);
        assert_eq!(i.line_count(), 1_000_000);
        let kb = i.memory_footprint() / 1024;
        assert!(kb < 32, "100 万行的索引占了 {kb}KB，稀疏索引没生效");
    }
}
