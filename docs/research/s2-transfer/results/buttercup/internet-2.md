# S2 results: Buttercup, 5 pods on 3 machines behind Cilium NAT and the home router, our relays; client: GitHub runner (Azure, US)

5 of 5 nodes reachable, 4-of-5 stripes, 8 MiB (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 0.2 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | direct, public IPv4 | direct, public IPv4 |
| n1 | direct, public IPv4 | direct, public IPv4 |
| n2 | direct, public IPv4 | direct, public IPv4 |
| n3 | direct, public IPv4 | direct, public IPv4 |
| n4 | direct, public IPv4 | direct, public IPv4 |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Dropped | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| single n0 | 349 | 355 | 2% | 377 / 440 / 473 | 0 | - | n0 355 |
| single n1 | 251 | 259 | 3% | 548 / 657 / 710 | 0 | - | n1 259 |
| single n2 | 259 | 265 | 2% | 484 / 577 / 703 | 0 | - | n2 265 |
| single n3 | 349 | 351 | 1% | 378 / 436 / 462 | 0 | - | n3 351 |
| single n4 | 125 | 130 | 4% | 1054 / 1191 / 1240 | 0 | - | n4 130 |
| fixed 4-of-5 | 615 | 643 | 4% | 1767 / 1949 / 2275 | 0 | - | n0 163 · n1 163 · n2 163 · n3 155 · n4 0 |
| race 4-of-5 | 723 | 820 | 12% | 1444 / 1556 / 1703 | 0 | - | n0 186 · n1 189 · n2 189 · n3 179 · n4 77 |
| pull 4-of-5 | 841 | 860 | 2% | 1145 / 1536 / 1979 | 0 | - | n0 185 · n1 217 · n2 218 · n3 173 · n4 66 |

Goodput Mbit/s by 5 s window:

| Scenario | 0-5 s | 5-10 s | 10-15 s | 15-20 s | 20-25 s | 25-30 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| fixed 4-of-5 | 685 | 671 | 631 | 523 | 577 | 604 |
| race 4-of-5 | 644 | 752 | 765 | 711 | 738 | 725 |
| pull 4-of-5 | 832 | 872 | 832 | 805 | 859 | 846 |

