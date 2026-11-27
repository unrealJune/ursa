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
6. **Hedging on age rarely fired** (2 hedges in 30 s) and the slowest stripe took 20 s on the
   8 Mbit/s node. It now hedges on each node's measured rate instead (a request 2.5× past what
   its node's speed predicts is re-requested elsewhere), which is what makes the failure runs
   below recover.
7. **Relays cannot carry bulk data**, at least n0's public ones. Ursa needs its own relays and
   direct paths; the real-internet run between our own nodes measures both.

## When a node fails mid-read (run 45263499125)

The shaped network again, 60 s per run, one strategy per process. 20 s in, the **fastest node
(n4, 200 Mbit/s) fails** one of four ways: killed with `kill -9` (a crash, no goodbye on the wire),
black-holed (every packet dropped: a router or ISP failure, the connection just goes silent),
throttled to 2 Mbit/s (a node that is up but useless), or already offline before the read starts.
Files: [results/faults](results/faults/).

Goodput over the run and by 5 s window (Mbit/s; the fault lands at 20 s, the fourth column):

| Strategy, fault | Overall | 0-5 | 5-10 | 10-15 | 15-20 | 20-25 | 25-30 | 30-35 | 35-40 | ... 55-60 | Stripe p90 / max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| pull, healthy | **217** | 161 | 228 | 228 | 228 | 215 | 201 | 228 | 242 | 188 | 7.3 / 8.1 s |
| pull, killed | 147 | 161 | 228 | 228 | 215 | 188 | 107 | 94 | 121 | 94 | 10.3 / 13.3 s |
| pull, black-holed | 147 | 148 | 242 | 228 | 215 | 188 | 107 | 94 | 121 | 94 | 10.3 / 13.3 s |
| pull, slowed to 2 Mbit/s | 147 | 161 | 228 | 228 | 215 | 188 | 107 | 94 | 121 | 94 | 10.3 / 13.3 s |
| pull, offline from the start | 103 | 81 | 107 | 107 | 107 | 94 | 94 | 107 | 107 | 94 | 12.4 / 13.1 s |
| race, healthy | 150 | 134 | 161 | 148 | 148 | 161 | 134 | 161 | 161 | 134 | 7.1 / 7.2 s |
| race, killed | 103 | 134 | 161 | 148 | 148 | 161 | 54 | 54 | 67 | 81 | 15.1 / 18.8 s |
| race, offline from the start | 74 | 54 | 81 | 81 | 81 | 67 | 67 | 81 | 81 | 67 | 14.3 / 14.4 s |

What it shows:

1. **No stall.** With `pull`, every 5 s window after the failure still delivered 94-121 Mbit/s.
   The theoretical ceiling for the five uplinks that are left (100/50/40/20/8, each capped at a
   quarter) is 112 Mbit/s; `pull` runs at it within 5 seconds of losing the fastest node.
2. **A silent node costs the same as a crashed one.** Killed, black-holed and slowed are
   identical: the scheduler does not wait for QUIC to time out. A request overdue against its
   node's own measured rate is re-requested from another node (19-23 hedges per run), and the
   slowest stripe grew from 8 to 13 s, once.
3. **What is lost is capacity, not scheduling.** The 200 Mbit/s node supplied a quarter of the
   bytes; without it the pool is a 112 Mbit/s pool. `race` falls to ~70 Mbit/s (the fourth
   fastest node's rate × 4), half of what `pull` keeps.
4. **So the defences are breadth and the home node.** With twelve similar nodes, losing one
   costs a twelfth. The home node's hot cache keeps the most-read files off the pool entirely.

## Real nodes: five pods on Buttercup (2026-11-23)

Five `s2 serve` pods on Buttercup, June's three-machine k3s cluster at home (Cilium, so each
pod is behind the cluster's NAT and then the home router's). Two clients, one at a time. Files: [results/buttercup](results/buttercup/).

| Client | Relays | Paths | Best single node | fixed | race | **pull** |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| GitHub runner, open internet | ours (`iroh-euw`, `iroh-wus`) | **direct, all five** | 349 | 615 | 723 | **841 Mbit/s** |
| GitHub runner, open internet | n0's public ones | relay, all five | 0 | 0 | 0 | 0 |
| Laptop on the same home LAN | ours | relay (`iroh-wus`), all five | 179 | 49 | 54 | 114 (188-201 by the end) |
| Laptop on the same home LAN | n0's public ones | relay, all five | 0 | 0 | 0 | 0 |

1. **From the internet, iroh punched straight through two layers of NAT** (Cilium's and the
   router's) to every pod, and the striped read ran at **841 Mbit/s, 2.4× the best single
   pod**, steady across the run (805-872 in every 5 s window). Waste was 2%. That is what
   friends reading from a well-connected home will see.
2. **n0's public relays are unusable for this**, and not only as a data path: with them, the
   same nodes and the same runner never got a direct path at all, and the relay itself carried
   ~0.3 Mbit/s (a 2 MiB shard never finished in 30 s). Every Ursa build ships with our own
   relays; n0's must never be the default.
3. **On the home LAN there was no direct path**, so every byte went up to `iroh-wus` and back
   down the same connection. The pods advertise pod addresses the laptop cannot reach, and
   their replies come from the hosts' LAN addresses, which the laptop's firewall treats as
   unsolicited. The home node is meant to be the fast local cache, so **it must be directly
   reachable on its own LAN**: run it on the host network (or with a published UDP port that it
   advertises), and let clients find it on the LAN (iroh's local discovery). Behind the relay
   it still did 114-201 Mbit/s, so the relay is a usable fallback, not a dead end.
4. **Single-node speeds through the same uplink varied 125-349 Mbit/s** between pods on the
   same three machines; the pull scheduler took 66 Mbit/s from the slow one and ~200 from the
   others, as designed.

The relay token for these runs came from an Actions secret, not from this repository.

## Still owed

- Nodes in *several* homes (Buttercup is one uplink): friends' machines, per
  [REAL-RUN.md](REAL-RUN.md). The internet client and our relays are ready for it.
- The LAN case with a node on the host network, to confirm a direct local path.
- Longer runs (5 min) so end effects stop hiding the difference from theory.
