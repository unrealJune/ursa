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

Nothing left behind: a switch killed twice part-way is finished by the next run, which also
deletes what the killed runs wrote; an upload killed part-way leaves pieces that a sweep keeps
while young and deletes after (grace 0 here); a deleted file's pieces are swept too. After each,
the pool holds n/k of the files' bytes and every file still reads.

  python -I scripts/protection-test.py --bin target/debug --work <dir>
"""
import argparse, atexit, hashlib, json, os, re, shutil, subprocess, time

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


@atexit.register
def stop_all():
    """Stops this run's nodes however the run ends, so a failure does not leave them running."""
    for p in procs.values():
        if p.poll() is None:
            p.kill()


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


def unique_bytes():
    """Bytes of distinct file content: identical files share chunks, stored once."""
    seen, total = set(), 0
    for p, digest in files.items():
        if digest not in seen:
            seen.add(digest)
            total += os.path.getsize(p)
    return total


def check_pool(k, n, label, slack=1.10, extra=3_000_000):
    """The pool holds about unique bytes x n/k of alice's: nothing left behind."""
    st, raw = pool_bytes()
    want = unique_bytes() * n / k
    assert want * 0.97 <= raw <= want * slack + extra, f"{label}: pool holds {raw}, expected about {want:.0f}"
    log(f"{label}: pool holds {raw / 1e6:.1f} MB for {unique_bytes() / 1e6:.1f} MB of files ({raw / unique_bytes():.2f}x)")
    return st


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
    check_pool(k, n, f"{k}-of-{n}")
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

def killed(*a, after):
    """Runs `ursa alice ...` and kills it after `after` seconds (the app closing mid-way)."""
    p = subprocess.Popen([URSA, "--profile", os.path.join(work, "profiles", "alice"), "--device-name", "alice", *a],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=dict(os.environ, RUST_LOG="warn"))
    time.sleep(after)
    p.kill(); p.wait()


# A switch cut short, twice, at different points: the next run finishes it, and clears what the
# stopped runs wrote (this device's journal), with no sweep needed.
ursa("alice", "protection", "--k", "4", "--n", "6")
for after in (2.5, 2.0):
    killed("recode", after=after)
    st = storage()
    log(f"switch to 4-of-6 killed after {after}s: {st['pending_files']} files to move, {st['leftover_pieces']} uncommitted pieces in the journal")
st = storage()
assert st["leftover_pieces"] > 0 or st["pending_files"] == 0, "the stopped runs wrote nothing: kill later"
r = json.loads(ursa("alice", "recode"))
st = storage()
assert st["pending_files"] == 0 and list(st["codes"]) == ["4-of-6"], st
assert st["leftover_pieces"] == 0, st["leftover_pieces"]
log(f"resumed: re-coded {r['recoded_files']} files, deleted {r['shards_deleted']} pieces (old code and the stopped runs' leftovers)")
check_pool(4, 6, "after the resumed switch")
check_all("resumed 4-of-6")

switch(3, 5)

# An upload killed part-way leaves pieces nothing references. A sweep keeps them while they are
# young (another device could still be about to commit them) and deletes them after.
big = os.path.join(work, "big.bin")
with open(big, "wb") as f:
    f.write(os.urandom(300_000_000))
_, before = pool_bytes()
killed("upload", big, after=4.0)
_, during = pool_bytes()
assert during > before, "the killed upload wrote nothing: kill later"
assert "big.bin" not in ursa("alice", "ls"), "the upload finished before it was killed: use a bigger file"
g = json.loads(ursa("alice", "gc"))
assert g["deleted"] == 0 and g["kept_young"] > 0, g
log(f"killed upload left {(during - before) / 1e6:.1f} MB; gc with 72 h grace kept {g['kept_young']} young pieces")
g = json.loads(ursa("alice", "gc", "--grace-hours", "0"))
assert g["deleted"] > 0 and not g["nodes_skipped"], g
log(f"gc with no grace: deleted {g['deleted']} pieces, {g['bytes_freed'] / 1e6:.1f} MB")
check_pool(3, 5, "after the sweep", slack=1.03, extra=1_000_000)
check_all("after the sweep")

# A deleted file's pieces go too.
gone = os.path.join(src, "file-4.bin")
ursa("alice", "rm", uploaded[os.path.basename(gone)])
del files[gone]
g = json.loads(ursa("alice", "gc", "--grace-hours", "0"))
log(f"deleted file-4: gc deleted {g['deleted']} pieces, {g['bytes_freed'] / 1e6:.1f} MB")
check_pool(3, 5, "after deleting a file", slack=1.03, extra=1_000_000)
check_all("after deleting a file")
st = storage()
assert st["files"] == len(files) and st["leftover_pieces"] == 0, st

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
