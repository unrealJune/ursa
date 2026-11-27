# S2 results: loopback, one runner, no shaping

6 nodes, 4-of-6, 8 MiB stripes (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 0.0 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | direct Ip(172.17.0.1:36615) | direct Ip(172.17.0.1:36615) |
| n1 | direct Ip(172.17.0.1:59728) | direct Ip(172.17.0.1:59728) |
| n2 | direct Ip(10.1.0.234:43306) | direct Ip(10.1.0.234:43306) |
| n3 | direct Ip(10.1.0.234:51873) | direct Ip(10.1.0.234:51873) |
| n4 | direct Ip(10.1.0.234:34825) | direct Ip(10.1.0.234:34825) |
| n5 | direct Ip(10.1.0.234:58758) | direct Ip(10.1.0.234:58758) |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| single n0 | 2170 | 2175 | 0% | 60 / 64 / 116 | 0 | n0 2175 |
| single n1 | 2112 | 2114 | 0% | 62 / 67 / 111 | 0 | n1 2114 |
| single n2 | 2349 | 2352 | 0% | 57 / 60 / 70 | 0 | n2 2352 |
| single n3 | 2246 | 2251 | 0% | 59 / 63 / 79 | 0 | n3 2251 |
| single n4 | 2255 | 2262 | 0% | 59 / 63 / 91 | 0 | n4 2262 |
| single n5 | 2371 | 2373 | 0% | 56 / 59 / 66 | 0 | n5 2373 |
| fixed 4-of-6 | 2707 | 2723 | 1% | 368 / 430 / 488 | 0 | n0 679 · n1 677 · n2 685 · n3 681 · n4 0 · n5 0 |
| race 4-of-6 | 1897 | 2456 | 23% | 425 / 542 / 642 | 0 | n0 371 · n1 422 · n2 444 · n3 369 · n4 414 · n5 436 |
| pull 4-of-6 | 2727 | 2729 | 0% | 118 / 151 / 289 | 0 | n0 459 · n1 442 · n2 463 · n3 455 · n4 450 · n5 460 |

