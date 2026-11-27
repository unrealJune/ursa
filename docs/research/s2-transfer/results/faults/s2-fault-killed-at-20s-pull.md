# S2 results: fastest node (n4) killed-at-20s; pull; uploads 100/50/40/20/200/8 Mbit/s

6 of 6 nodes reachable, 4-of-6 stripes, 8 MiB (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 0.1 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

Fault injected 20 s into each striped scenario: `kill -9 2348`

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | direct Ip(10.77.0.20:54654) | direct Ip(10.77.0.20:54654) |
| n1 | direct Ip(10.77.0.21:38763) | direct Ip(10.77.0.21:38763) |
| n2 | direct Ip(10.77.0.22:59310) | direct Ip(10.77.0.22:59310) |
| n3 | direct Ip(10.77.0.23:55170) | direct Ip(10.77.0.23:55170) |
| n4 | direct Ip(10.77.0.24:49641) | direct Ip(10.77.0.24:49641) |
| n5 | direct Ip(10.77.0.25:46220) | direct Ip(10.77.0.25:46220) |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Dropped | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| pull 4-of-6 | 147 | 161 | 9% | 5262 / 10253 / 13272 | 19 | n4 | n0 41 · n1 38 · n2 34 · n3 19 · n4 21 · n5 8 |

Goodput Mbit/s by 5 s window:

| Scenario | 0-5 s | 5-10 s | 10-15 s | 15-20 s | 20-25 s | 25-30 s | 30-35 s | 35-40 s | 40-45 s | 45-50 s | 50-55 s | 55-60 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| pull 4-of-6 | 161 | 228 | 228 | 215 | 188 | 107 | 94 | 121 | 121 | 107 | 94 | 94 |

