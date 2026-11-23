# S2 results: Buttercup, 5 pods on 3 machines; client: June's laptop on home Wi-Fi (same LAN)

5 of 5 nodes reachable, 4-of-5 stripes, 8 MiB (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 0.4 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | relay https://usw1-1.relay.n0.iroh.link./ | relay https://usw1-1.relay.n0.iroh.link./ |
| n1 | relay https://usw1-1.relay.n0.iroh.link./ | relay https://usw1-1.relay.n0.iroh.link./ |
| n2 | relay https://usw1-1.relay.n0.iroh.link./ | relay https://usw1-1.relay.n0.iroh.link./ |
| n3 | relay https://usw1-1.relay.n0.iroh.link./ | relay https://usw1-1.relay.n0.iroh.link./ |
| n4 | relay https://usw1-1.relay.n0.iroh.link./ | relay https://usw1-1.relay.n0.iroh.link./ |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Dropped | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| single n0 | 0 | 0 | 100% | 0 / 0 / 0 | 0 | - | n0 0 |
| single n1 | 0 | 0 | 100% | 0 / 0 / 0 | 0 | - | n1 0 |
| single n2 | 0 | 0 | 100% | 0 / 0 / 0 | 0 | - | n2 0 |
| single n3 | 0 | 0 | 100% | 0 / 0 / 0 | 0 | - | n3 0 |
| single n4 | 0 | 0 | 100% | 0 / 0 / 0 | 0 | - | n4 0 |
| fixed 4-of-5 | 0 | 1 | 100% | 0 / 0 / 0 | 0 | - | n0 0 · n1 0 · n2 0 · n3 0 · n4 0 |
| race 4-of-5 | 0 | 1 | 100% | 0 / 0 / 0 | 0 | - | n0 0 · n1 0 · n2 0 · n3 0 · n4 0 |
| pull 4-of-5 | 0 | 1 | 100% | 0 / 0 / 0 | 0 | - | n0 0 · n1 0 · n2 0 · n3 0 · n4 0 |

Goodput Mbit/s by 5 s window:

| Scenario | 0-5 s | 5-10 s | 10-15 s | 15-20 s | 20-25 s | 25-30 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| fixed 4-of-5 | 0 | 0 | 0 | 0 | 0 | 0 |
| race 4-of-5 | 0 | 0 | 0 | 0 | 0 | 0 |
| pull 4-of-5 | 0 | 0 | 0 | 0 | 0 | 0 |

