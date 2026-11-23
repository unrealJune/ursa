#!/usr/bin/env bash
# Put the static s2 binary into each waiting pod of the ursa-s2 StatefulSet.
# Usage: deploy.sh <path to x86_64-unknown-linux-musl s2>
set -euo pipefail
bin=${1:?path to the static s2 binary}
ns=ursa-s2

kubectl apply -f "$(dirname "$0")/buttercup.yaml"
# kubectl cp reads "C:" in a Windows path as a pod name, so copy from the binary's own directory.
cd "$(dirname "$bin")"
bin=$(basename "$bin")
for i in 0 1 2 3 4; do
  pod="s2-$i"
  kubectl -n "$ns" wait --for=jsonpath='{.status.initContainerStatuses[0].state.running}' "pod/$pod" --timeout=180s >/dev/null
  # Copy under a temporary name, then mark it ready: the init container must never see half a file.
  kubectl -n "$ns" cp "$bin" "$pod:/opt/s2/s2.part" -c wait-for-binary
  kubectl -n "$ns" exec "$pod" -c wait-for-binary -- sh -c 'chmod 755 /opt/s2/s2.part && mv /opt/s2/s2.part /opt/s2/s2 && touch /opt/s2/ready'
  echo "$pod: binary in place"
done
kubectl -n "$ns" wait --for=condition=Ready pod -l app.kubernetes.io/name=ursa-s2 --timeout=180s
kubectl -n "$ns" get pods -o wide
