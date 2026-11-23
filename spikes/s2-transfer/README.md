# Spike S2: reading coded stripes from several nodes over iroh

**Question:** how fast can a client read 4-of-6 stripes from six friends' nodes over iroh, and
how should it schedule the requests?

```sh
s2 serve --seed RUN --index 0 [--local] [--ticket-out t0.txt]   # on each node
s2 fetch --seed RUN --nodes 6 [--seconds 30] [--json out.json]   # or --peer <ticket|id> ×6
```

Nodes serve pseudo-random bytes from memory (`ursa/s2/0`: stripe, length → bytes), so this
measures the network and the protocol, not a disk. `--local` turns relays off. Node keys are
derived from the seed, so a client finds nodes by seed through iroh's discovery.

## Strategies

| | How it asks | Expected limit |
| --- | --- | --- |
| `single` | whole stripes from one node | that node's uplink |
| `fixed` | shard `j` of every stripe from node `j < k` | `k` × the slowest of those `k` |
| `race` | every stripe from all `n`; first `k` win, the rest are cancelled (Storj) | `k` × the `k`-th fastest uplink |
| `pull` | each node keeps 2 requests in flight and takes the oldest stripe that still needs a shard from it; an idle node duplicates the shard holding up an overdue stripe (1.5 × median) | every uplink busy, each node capped at `1/k` of the bytes |

## Results (CI, 2026-11-23)

Files: [loopback](results/s2-loopback.md), [shaped](results/s2-shaped.md),
[shaped, 4 per node](results/s2-shaped-pn4.md), [shaped, window 4](results/s2-shaped-w4.md).

**Loopback** (one 2-vCPU runner, no shaping, all seven processes sharing it): one iroh
connection carries **2.1–2.4 Gbit/s**; `pull` and `fixed` reach 2.7 Gbit/s over six (CPU-bound);
`race` 1.9 Gbit/s with 23% of what it received thrown away.

**Shaped**: seven network namespaces on one runner; each node's upload throttled with netem to
100 / 50 / 40 / 20 / 200 / 8 Mbit/s with +15 / 30 / 25 / 50 / 10 / 90 ms; the client's link
unthrottled; direct paths, relays off. 8 MiB stripes, window 16, 30 s per strategy:

| Strategy | Goodput Mbit/s | Theory | Waste | Stripe p50 / p90 / max |
| --- | ---: | ---: | ---: | --- |
| best single node (n4) | 188 | 200 | 0% | 0.7 / 0.7 / 0.7 s |
| `fixed` 4-of-6 | 74 | 80 | 27% | 12.9 / 14.2 / 14.2 s |
| `race` 4-of-6 | 148 | 160 | 16% | 7.1 / 7.1 / 7.1 s |
| **`pull` 4-of-6** | **197** | 236 | 10% | 4.4 / 5.5 / 20.6 s |
| `pull`, 4 in flight per node | 188 | 236 | 12% | 3.9 / 8.4 / 18.9 s |
| `pull`, window 4 stripes | 150 | 236 | 7% | 1.7 / 3.1 / 4.5 s |

Waste here is mostly stripes still in flight when the 30 s ran out; a longer run moves goodput
toward the theory column.

**Separate runners** (run 45254098074): six nodes and a client on their own GitHub runners,
found by seed. Azure's NAT defeated hole punching, every path went through n0's public relays,
and those carried almost nothing (0–1 Mbit/s). Not a measurement of Ursa; it is why the test
moved into namespaces.

## Findings

1. **The protocol is not the bottleneck.** 2+ Gbit/s per iroh connection on loopback is 10–100×
   a home uplink.
2. **Pull beats race, and race hurts friends.** `pull` delivered 33% more than `race` and 2.7×
   `fixed`. Under `race`, the two slowest nodes (20 and 8 Mbit/s) never won a shard: everything
   they uploaded was thrown away. Friends' upload is the scarce resource in a pool; spending
   it on shards that lose is the wrong default.
3. **Each node is capped at `1/k` of the bytes.** A stripe needs `k` *different* nodes, so the
   200 Mbit/s node could give at most a quarter. Striping across six similar uplinks reads at
   nearly their sum; one fat uplink among thin ones barely beats reading from it alone
   (197 vs 188). Hence a **hot cache of whole copies on the home node**, which is usually the
   fat, close host.
4. **Keep at least 8 stripes in flight.** A window of 4 (32 MiB) cost a quarter of the
   throughput at these speeds. The window should follow the bandwidth-delay product and start
   small for video, where the first stripe's latency is what the viewer sees.
5. **Two requests in flight per node is enough**; four changed nothing.
6. **Hedging rarely fired** (2 hedges in 30 s) and the slowest stripe still took 20 s on the
   8 Mbit/s node. A smarter rule (skip a node whose estimated finish is later than the median,
   or hedge on estimated finish rather than age) is the next refinement for tail latency.
7. **Relays cannot carry bulk data**, at least n0's public ones. Ursa needs its own relays and
   direct paths; the real-internet run between our own nodes measures both.

## Still owed

- The same `s2 fetch` between our own nodes over the internet: NAT traversal between real home
  routers, relay fallback speed on our relays, and real uplinks.
- Longer runs (5 min) so end effects stop hiding the difference from theory.
