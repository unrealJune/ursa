# S2 results: uploads throttled to 100/50/40/20/200/8 Mbit/s, +15/30/25/50/10/90 ms; window of 4 stripes (32 MiB, a phone's budget)

6 nodes, 4-of-6, 8 MiB stripes (2048 KiB shards), window 4 stripes, 2 requests in flight per node. Connected in 0.1 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

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
| pull 4-of-6 | 150 | 161 | 7% | 1715 / 3089 / 4508 | 23 | n0 40 · n1 37 · n2 31 · n3 18 · n4 30 · n5 5 |

