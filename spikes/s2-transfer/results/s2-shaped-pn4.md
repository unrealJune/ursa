# S2 results: uploads throttled to 100/50/40/20/200/8 Mbit/s, +15/30/25/50/10/90 ms; 4 requests in flight per node

6 nodes, 4-of-6, 8 MiB stripes (2048 KiB shards), window 16 stripes, 4 requests in flight per node. Connected in 0.1 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

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
| race 4-of-6 | 150 | 177 | 15% | 7073 / 7074 / 7083 | 0 | n0 46 · n1 45 · n2 38 · n3 1 · n4 46 · n5 0 |
| pull 4-of-6 | 188 | 214 | 12% | 3868 / 8396 / 18884 | 4 | n0 56 · n1 47 · n2 37 · n3 14 · n4 56 · n5 5 |

