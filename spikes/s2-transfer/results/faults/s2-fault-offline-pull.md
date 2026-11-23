# S2 results: fastest node (n4) offline; pull; uploads 100/50/40/20/200/8 Mbit/s

5 of 6 nodes reachable, 4-of-6 stripes, 8 MiB (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 23.0 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | direct Ip(10.77.0.20:54654) | direct Ip(10.77.0.20:54654) |
| n1 | direct Ip(10.77.0.21:38763) | direct Ip(10.77.0.21:38763) |
| n2 | direct Ip(10.77.0.22:59310) | direct Ip(10.77.0.22:59310) |
| n3 | direct Ip(10.77.0.23:55170) | direct Ip(10.77.0.23:55170) |
| n5 | direct Ip(10.77.0.25:46220) | direct Ip(10.77.0.25:46220) |
| n4 | offline | offline |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Dropped | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| pull 4-of-5 | 103 | 117 | 12% | 8984 / 12449 / 13084 | 0 | - | n0 30 · n1 30 · n2 30 · n3 19 · n5 8 |

Goodput Mbit/s by 5 s window:

| Scenario | 0-5 s | 5-10 s | 10-15 s | 15-20 s | 20-25 s | 25-30 s | 30-35 s | 35-40 s | 40-45 s | 45-50 s | 50-55 s | 55-60 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| pull 4-of-5 | 81 | 107 | 107 | 107 | 94 | 94 | 107 | 107 | 134 | 107 | 94 | 94 |

