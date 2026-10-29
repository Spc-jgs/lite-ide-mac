# 资源测量：REVIEW

场景：full；HEAD：b094ada043c6df4b81acbeef8c799de4a1a69b95；dirty：True

| 场景 | 已知 footprint MiB | 单核 CPU % | logical writes KiB | 主进程 FD | WebKit 归属 |
|---|---:|---:|---:|---:|---|
| baseline_idle | 105.80 | 0.09 | 100.00 | 11.00 | fresh-coalition |
| opened-1 | 588.27 | 3.73 | 36.00 | 11.00 | fresh-coalition |
| closed-1 | 248.07 | 0.17 | 0.00 | 11.00 | fresh-coalition |
| opened-2 | 575.82 | 3.37 | 40.00 | 11.00 | fresh-coalition |
| closed-2 | 251.88 | 0.16 | 0.00 | 11.00 | fresh-coalition |
| opened-3 | 587.05 | 3.52 | 24.00 | 11.00 | fresh-coalition |
| closed-3 | 259.79 | 0.15 | 4.00 | 11.00 | fresh-coalition |
| opened-4 | 593.13 | 3.11 | 40.00 | 11.00 | fresh-coalition |
| closed-4 | 267.96 | 0.32 | 20.00 | 11.00 | fresh-coalition |
| opened-5 | 592.86 | 3.05 | 36.00 | 11.00 | fresh-coalition |
| closed-5 | 258.63 | 0.15 | 4.00 | 11.00 | fresh-coalition |
| opened-6 | 603.43 | 1.64 | 20.00 | 11.00 | fresh-coalition |
| closed-6 | 265.83 | 0.15 | 4.00 | 11.00 | fresh-coalition |
| opened-7 | 599.39 | 1.64 | 40.00 | 11.00 | fresh-coalition |
| closed-7 | 276.07 | 0.07 | 0.00 | 11.00 | fresh-coalition |
| opened-8 | 617.77 | 1.40 | 36.00 | 11.00 | fresh-coalition |
| closed-8 | 285.32 | 0.11 | 0.00 | 11.00 | fresh-coalition |
| log_open | 428.72 | 0.79 | 24.00 | 11.00 | fresh-coalition |
| log_filtered | 359.38 | 3.84 | 4.00 | 11.00 | fresh-coalition |
| recovered_idle | 287.79 | 0.22 | 84.00 | 11.00 | fresh-coalition |

## 线索 / 缺失

- {"status": "WARN", "metric": "retained_memory", "reason": "预热后关完序列持续增长；需长测 / Allocations 确认，不直接判内存泄漏"}
- {"status": "WARN", "metric": "opened_memory_budget", "value": 668307728, "limit": 314572800, "reason": "快速打开 12 文件的采样峰值超过 300 MiB 参考预算，需定位瞬时分配/缓存；不能用关闭后不增长代替峰值预算验收"}

有限场景未发现增长线索不等于证明无泄漏
