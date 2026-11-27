#!/usr/bin/env bash
# Seven network namespaces on a bridge: `client` (unthrottled) and n0..n5, each with its upload
# throttled by netem to a home-like rate and delay. Needs root (CI runs it with sudo).
# Usage: netns.sh up | restore <i>
set -euo pipefail

RATES=(100 50 40 20 200 8)  # Mbit/s up
DELAYS=(15 30 25 50 10 90)  # ms one way

shape() { # i -> the node's normal shaping
  echo "delay ${DELAYS[$1]}ms rate ${RATES[$1]}mbit limit 100000"
}

case "${1:-}" in
  up)
    modprobe br_netfilter 2>/dev/null || true
    sysctl -qw net.bridge.bridge-nf-call-iptables=0 net.bridge.bridge-nf-call-ip6tables=0 || true
    ip link add br0 type bridge
    ip link set br0 up
    iptables -I FORWARD -i br0 -o br0 -j ACCEPT
    add() { # name address
      ip netns add "$1"
      ip link add "v-$1" type veth peer name eth0 netns "$1"
      ip link set "v-$1" master br0 up
      ip -n "$1" addr add "$2/24" dev eth0
      ip -n "$1" link set eth0 up
      ip -n "$1" link set lo up
    }
    add client 10.77.0.10
    for i in 0 1 2 3 4 5; do
      add "n$i" "10.77.0.2$i"
      # shellcheck disable=SC2046
      ip netns exec "n$i" tc qdisc add dev eth0 root netem $(shape "$i")
    done
    ip netns exec client ping -c 1 -W 2 10.77.0.25 >/dev/null
    ;;
  restore)
    # shellcheck disable=SC2046
    ip netns exec "n$2" tc qdisc change dev eth0 root netem $(shape "$2")
    ;;
  *)
    echo "usage: $0 up | restore <i>" >&2
    exit 2
    ;;
esac
