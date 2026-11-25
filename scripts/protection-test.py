"""Switching protection: alice moves her library between codes and nothing is lost.

Six people, each with a node; alice befriends the other five, so her pool has six failure
domains. She uploads files (two with identical content, so they share chunks), then moves the
library through 3-of-5 (automatic) -> 2 copies -> 4-of-6 -> 3 copies -> 2-of-4 -> 3-of-5.
After every switch:

  - every file downloads byte-identical
  - every chunk uses the new code, and a second re-code has nothing to do
  - the pool holds about stored x n/k bytes of alice's: the old pieces were deleted

Plus: at 4-of-6, two nodes are stopped and every file still reads; a switch that needs more
nodes than are up is refused and leaves the library readable; the storage report carries
uptime, limits and a recommendation; and `give` regrants friends' space.

  python -I scripts/protection-test.py --bin target/debug --work <dir>
"""
import argparse, hashlib, json, os, re, shutil, subprocess, time

ap = argparse.ArgumentParser()
ap.add_argument("--bin", required=True, help="directory with ursa and ursa-node")
ap.add_argument("--work", required=True)
ap.add_argument("--port-base", type=int, default=47310)
args = ap.parse_args()

EXE = ".exe" if os.name == "nt" else ""
URSA = os.path.join(args.bin, "ursa" + EXE)
NODE = os.path.join(args.bin, "ursa-node" + EXE)
work = os.path.abspath(args.work)
shutil.rmtree(work, ignore_errors=True)
os.makedirs(work)
PEOPLE = ["alice", "bob", "carol", "dave", "erin", "frank"]
t0 = time.time()


def log(msg):
    print(f"[{time.time() - t0:6.1f}s] {msg}", flush=True)


def ursa(profile, *a, check=True, timeout=600):
    r = subprocess.run([URSA, "--profile", os.path.join(work, "profiles", profile), "--device-name", profile, *a],
                       capture_output=True, text=True, timeout=timeout, env=dict(os.environ, RUST_LOG="warn"))
    if check and r.returncode != 0:
        print(r.stdout); print(r.stderr)
        raise SystemExit(f"ursa {profile} {a[0]} failed ({r.returncode})")
    return r if not check else r.stdout.strip()


procs = {}


def start_node(name, wait_claim=True):
    d = os.path.join(work, "nodes", name)
    os.makedirs(d, exist_ok=True)
    log_path = os.path.join(d, f"node-{int(time.time() * 1000)}.log")
    port = args.port_base + PEOPLE.index(name)
    p = subprocess.Popen([NODE, "--data-dir", d, "--health-port", "0", "--port", str(port)],
                         stdout=open(log_path, "w"), stderr=subprocess.STDOUT)
    procs[name] = p
    for _ in range(120):
        text = open(log_path, encoding="utf-8", errors="replace").read()
        m = re.search(r"(ursa-claim:\S+)", text)
        if wait_claim and m:
            return m.group(1)
        if not wait_claim and "ticket:" in text:
            return None
        time.sleep(0.5)
    raise SystemExit(f"node {name} did not start")


def stop_node(name):
    procs[name].kill()
    procs[name].wait()


claims = {who: start_node(who) for who in PEOPLE}
log("six nodes up")
for who in PEOPLE:
    ursa(who, "create", "--name", who.capitalize())
    ursa(who, "claim", claims[who], "--label", f"{who}'s node")
invites = {who: ursa(who, "invite") for who in PEOPLE}
for who in PEOPLE[1:]:
    ursa("alice", "befriend", invites[who], "--quota-gb", "2")
    ursa(who, "befriend", invites["alice"], "--quota-gb", "2")
log("alice and five friends, each granting the other 2 GB")


def storage():
    return json.loads(ursa("alice", "storage"))


# Files: a spread of sizes, and two with the same bytes (shared chunks).
src = os.path.join(work, "src")
os.makedirs(src)
files = {}
for i, n in enumerate([300, 90_000, 2_500_000, 6_000_000, 13_000_000]):
    p = os.path.join(src, f"file-{i}.bin")
    with open(p, "wb") as f:
        f.write(os.urandom(n))
shutil.copy(os.path.join(src, "file-3.bin"), os.path.join(src, "file-3-copy.bin"))
for name in sorted(os.listdir(src)):
    p = os.path.join(src, name)
    files[p] = hashlib.sha256(open(p, "rb").read()).hexdigest()
out = ursa("alice", "upload", *files.keys())
uploaded = dict(line.split("\t")[::-1] for line in out.splitlines())
log(f"uploaded {len(files)} files")


def check_all(label):
    out_dir = os.path.join(work, "out-" + label)
    os.makedirs(out_dir, exist_ok=True)
    for path, digest in files.items():
        name = os.path.basename(path)
        target = os.path.join(out_dir, name)
        ursa("alice", "get", uploaded[name], target)
        assert hashlib.sha256(open(target, "rb").read()).hexdigest() == digest, f"{label}: {name} differs"
    log(f"{label}: all {len(files)} files byte-identical")


st = storage()
assert st["protection"] == {"k": 3, "n": 5} and not st["chosen"], st["protection"]
assert list(st["codes"]) == ["3-of-5"], st["codes"]
assert st["pending_files"] == 0
assert len(st["members"]) == 6 and all(m.get("up_30d", 0) > 0.9 for m in st["members"]), st["members"]
assert st["stored_bytes"] == sum(os.path.getsize(p) for p in files), st["stored_bytes"]
rec = [c for c in st["choices"] if c["recommended"]]
assert len(rec) == 1, st["choices"]
assert all(c["available"] == (c["n"] <= 6) for c in st["choices"]), st["choices"]
log(f"storage report: 3-of-5 automatic, limit {st['limit_bytes'] / 1e9:.2f} GB, recommended {rec[0]['k']}-of-{rec[0]['n']}")
check_all("initial")
stored = st["stored_bytes"]


def pool_bytes():
    st = storage()
    return st, st["home_bytes"] + st["friends_bytes"]


def switch(k, n):
    ursa("alice", "protection", "--k", str(k), "--n", str(n))
    st = storage()
    assert st["protection"] == {"k": k, "n": n} and st["chosen"]
    assert st["pending_files"] == len(files), st["pending_files"]
    t = time.time()
    r = json.loads(ursa("alice", "recode"))
    log(f"{k}-of-{n}: re-coded {r['recoded_files']} files, {r['objects_recoded']} objects, deleted {r['shards_deleted']} old pieces ({time.time() - t:.1f}s)")
    assert not r["failures"], r
    st, raw = pool_bytes()
    assert st["pending_files"] == 0, st
    assert list(st["codes"]) == [f"{k}-of-{n}"], st["codes"]
    # Shared chunks are stored once, so expect the unique bytes times n/k. (After the cut-short
    # switch, pieces written but never committed linger until leases exist: allow for them.)
    unique = stored - os.path.getsize(os.path.join(src, "file-3.bin"))
    want = unique * n / k
    slack = 1.6 if (k, n) == (3, 5) else 1.10
    assert want * 0.97 <= raw <= want * slack + 3_000_000, f"{k}-of-{n}: pool holds {raw}, expected about {want:.0f}"
    log(f"{k}-of-{n}: pool holds {raw / 1e6:.1f} MB for {unique / 1e6:.1f} MB of files ({raw / unique:.2f}x)")
    again = json.loads(ursa("alice", "recode"))
    assert again["recoded_files"] == 0 and again["shards_deleted"] == 0, again
    check_all(f"{k}-of-{n}")


switch(1, 2)
switch(4, 6)

# 4-of-6 survives two nodes down.
stop_node("erin"); stop_node("frank")
log("erin's and frank's nodes stopped")
check_all("4-of-6 with two nodes down")

# A switch the pool cannot take right now is refused, and changes nothing.
ursa("alice", "protection", "--k", "3", "--n", "5")
r = ursa("alice", "recode", check=False)
assert r.returncode != 0 and "needs 5 nodes; 4 are reachable" in r.stderr, (r.returncode, r.stderr)
assert storage()["pending_files"] == len(files)
check_all("refused switch")
log("3-of-5 with four nodes up: refused, library untouched")

start_node("erin", wait_claim=False); start_node("frank", wait_claim=False)
time.sleep(2)
log("erin and frank back")
switch(1, 3)
switch(2, 4)

# A switch cut short (the app closed mid-way) finishes when it runs again.
ursa("alice", "protection", "--k", "4", "--n", "6")
cut = subprocess.Popen([URSA, "--profile", os.path.join(work, "profiles", "alice"), "--device-name", "alice", "recode"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(2.5)
cut.kill(); cut.wait()
st = storage()
log(f"switch to 4-of-6 cut short: {st['pending_files']} of {len(files)} files still to move, codes {st['codes']}")
r = json.loads(ursa("alice", "recode"))
st = storage()
assert st["pending_files"] == 0 and list(st["codes"]) == ["4-of-6"], st
log(f"resumed: re-coded {r['recoded_files']} more files; every chunk now 4-of-6")
check_all("resumed 4-of-6")

switch(3, 5)

# Giving: 7.5 GB split over five friends (each had 2 GB).
ursa("alice", "give", "7.5")
st = storage()
assert st["give_bytes"] == 7_500_000_000, st["give_bytes"]
assert sorted(h["quota_bytes"] for h in st["hosting"]) == [1_500_000_000] * 5, st["hosting"]
log("give 7.5 GB: each friend now has 1.5 GB on alice's node")

for p in procs.values():
    if p.poll() is None:
        p.kill()
log("PASS")
