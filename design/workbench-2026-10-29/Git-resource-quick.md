# 资源测量：REVIEW

场景：quick；HEAD：b094ada043c6df4b81acbeef8c799de4a1a69b95；dirty：True

| 场景 | 已知 footprint MiB | 单核 CPU % | logical writes KiB | 主进程 FD | WebKit 归属 |
|---|---:|---:|---:|---:|---|
| baseline_idle | 93.87 | 1.96 | 120.00 | 11.00 | fresh-coalition |
| opened-1 | 395.36 | 2.95 | 24.00 | 11.00 | fresh-coalition |
| closed-1 | 188.58 | 0.09 | 0.00 | 11.00 | fresh-coalition |
| opened-2 | 429.77 | 2.22 | 28.00 | 11.00 | fresh-coalition |
| closed-2 | 217.00 | 0.09 | 0.00 | 11.00 | fresh-coalition |
| opened-3 | 449.30 | 2.47 | 24.00 | 11.00 | fresh-coalition |
| closed-3 | 260.32 | 0.09 | 0.00 | 11.00 | fresh-coalition |
| opened-4 | 479.79 | 1.76 | 44.00 | 11.00 | fresh-coalition |
| closed-4 | 302.33 | 0.32 | 0.00 | 11.00 | fresh-coalition |
| log_open | 357.41 | 0.50 | 20.00 | 11.00 | fresh-coalition |
| log_filtered | 364.99 | 1.89 | 4.00 | 11.00 | fresh-coalition |
| recovered_idle | 245.20 | 0.15 | 36.00 | 11.00 | fresh-coalition |

## 线索 / 缺失

- {"status": "WARN", "metric": "retained_memory", "reason": "预热后关完序列持续增长；需长测 / Allocations 确认，不直接判内存泄漏"}
- {"status": "WARN", "metric": "opened_memory_budget", "value": 503664552, "limit": 314572800, "reason": "快速打开 12 文件的采样峰值超过 300 MiB 参考预算，需定位瞬时分配/缓存；不能用关闭后不增长代替峰值预算验收"}

有限场景未发现增长线索不等于证明无泄漏
