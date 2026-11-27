# S2 results: Buttercup, 5 pods on 3 machines, our relays; client: June's laptop on home Wi-Fi (same LAN)

5 of 5 nodes reachable, 4-of-5 stripes, 8 MiB (2048 KiB shards), window 16 stripes, 2 requests in flight per node. Connected in 0.4 s. Goodput counts completed stripes only; waste is bytes received that no completed stripe used.

| Node | path after warm-up | path at end |
| --- | --- | --- |
| n0 | relay https://iroh-wus.streetcrypt.id/ | relay https://iroh-wus.streetcrypt.id/ |
| n1 | relay https://iroh-wus.streetcrypt.id/ | relay https://iroh-wus.streetcrypt.id/ |
| n2 | relay https://iroh-wus.streetcrypt.id/ | relay https://iroh-wus.streetcrypt.id/ |
| n3 | relay https://iroh-wus.streetcrypt.id/ | relay https://iroh-wus.streetcrypt.id/ |
| n4 | relay https://iroh-wus.streetcrypt.id/ | relay https://iroh-wus.streetcrypt.id/ |

| Scenario | Goodput Mbit/s | Received Mbit/s | Waste | Stripe p50 / p90 / max ms | Hedges | Dropped | Per node Mbit/s |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| single n0 | 63 | 67 | 6% | 1862 / 2605 / 2627 | 0 | - | n0 67 |
| single n1 | 98 | 103 | 5% | 1169 / 1503 / 1714 | 0 | - | n1 103 |
| single n2 | 54 | 61 | 12% | 1921 / 4600 / 4600 | 0 | - | n2 61 |
| single n3 | 179 | 184 | 3% | 710 / 939 / 1302 | 0 | - | n3 184 |
| single n4 | 98 | 99 | 1% | 888 / 3216 / 4111 | 0 | - | n4 99 |
| fixed 4-of-5 | 49 | 52 | 6% | 3952 / 5350 / 5402 | 0 | - | n0 13 · n1 13 · n2 13 · n3 13 · n4 0 |
| race 4-of-5 | 54 | 76 | 29% | 3916 / 4984 / 5594 | 0 | - | n0 14 · n1 17 · n2 12 · n3 17 · n4 15 |
| pull 4-of-5 | 114 | 123 | 8% | 2535 / 4003 / 7817 | 1 | - | n0 21 · n1 31 · n2 18 · n3 33 · n4 20 |

Goodput Mbit/s by 5 s window:

| Scenario | 0-5 s | 5-10 s | 10-15 s | 15-20 s | 20-25 s | 25-30 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| fixed 4-of-5 | 27 | 54 | 27 | 54 | 54 | 81 |
| race 4-of-5 | 27 | 27 | 81 | 54 | 81 | 54 |
| pull 4-of-5 | 67 | 40 | 81 | 107 | 201 | 188 |

