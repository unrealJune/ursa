# S2 results: uploads throttled to 100/50/40/20/200/8 Mbit/s, +15/30/25/50/10/90 ms

6 nodes, 4-of-6, 8 MiB stripes (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 0.1 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | direct Ip(10.77.0.20:39759) | direct Ip(10.77.0.20:39759) |
| n1 | direct Ip(10.77.0.21:47521) | direct Ip(10.77.0.21:47521) |
| n2 | direct Ip(10.77.0.22:38716) | direct Ip(10.77.0.22:38716) |
| n3 | direct Ip(10.77.0.23:46158) | direct Ip(10.77.0.23:46158) |
| n4 | direct Ip(10.77.0.24:56879) | direct Ip(10.77.0.24:56879) |
| n5 | direct Ip(10.77.0.25:34682) | direct Ip(10.77.0.25:34682) |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| single n0 | 89 | 95 | 6% | 1419 / 1422 / 1430 | 0 | n0 95 |
| single n1 | 45 | 47 | 6% | 2829 / 2837 / 2858 | 0 | n1 47 |
| single n2 | 36 | 38 | 5% | 3558 / 3560 / 3560 | 0 | n2 38 |
| single n3 | 18 | 19 | 5% | 7106 / 7120 / 7120 | 0 | n3 19 |
| single n4 | 188 | 189 | 0% | 710 / 714 / 718 | 0 | n4 189 |
| single n5 | 0 | 8 | 100% | 0 / 0 / 0 | 0 | n5 8 |
| fixed 4-of-6 | 74 | 101 | 27% | 12862 / 14150 / 14150 | 0 | n0 27 · n1 27 · n2 27 · n3 19 · n4 0 · n5 0 |
| race 4-of-6 | 148 | 176 | 16% | 7074 / 7074 / 7074 | 0 | n0 46 · n1 46 · n2 38 · n3 0 · n4 46 · n5 0 |
| pull 4-of-6 | 197 | 219 | 10% | 4421 / 5484 / 20594 | 2 | n0 58 · n1 47 · n2 38 · n3 14 · n4 58 · n5 3 |

