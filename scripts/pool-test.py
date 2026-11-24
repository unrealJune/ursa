"""The five-person pool test, against real nodes.

Five people, each with their own account and their own node. Everyone befriends everyone, so
each account's files are coded 3-of-5 across all five nodes. Then:

  1. alice uploads files and adds a second device by link code
  2. alice's node *and its storage* are destroyed
  3. alice recovers on a fresh device from nothing but her key file (the account is found
     through its pkarr locator record, the way iroh finds endpoints)
  4. every file must download byte-identical, read from the four surviving nodes
  5. alice claims a new node and repairs: the pool is back to five shards per chunk

Local mode starts five `ursa-node` processes (disk store) itself:

  python -I scripts/pool-test.py --bin target/debug --work <dir>

Cluster mode is given the five nodes' claim links (a JSON file, person -> ursa-claim:...) and a
shell command that destroys alice's node; her replacement node is started locally:

  python -I scripts/pool-test.py --bin target/release --work <dir> \
      --claims claims.json --destroy "kubectl -n ursa scale deploy/ursa-test-alice-ursa-node --replicas 0"
"""
import argparse, hashlib, json, os, re, shutil, subprocess, sys, time

ap = argparse.ArgumentParser()
ap.add_argument("--bin", required=True, help="directory with ursa and ursa-node")
ap.add_argument("--work", required=True)
ap.add_argument("--files", type=int, default=6)
ap.add_argument("--claims", help="JSON: person -> claim link of a running node (cluster mode)")
ap.add_argument("--destroy", help="shell command that destroys alice's node (cluster mode)")
args = ap.parse_args()

EXE = ".exe" if os.name == "nt" else ""
URSA = os.path.join(args.bin, "ursa" + EXE)
NODE = os.path.join(args.bin, "ursa-node" + EXE)
work = os.path.abspath(args.work)
shutil.rmtree(work, ignore_errors=True)
os.makedirs(work)
PEOPLE = ["alice", "bob", "carol", "dave", "erin"]
PASS = "correct horse battery staple"
t0 = time.time()

def log(msg):
    print(f"[{time.time() - t0:6.1f}s] {msg}", flush=True)

def ursa(profile, *a, check=True, timeout=300):
    r = subprocess.run([URSA, "--profile", os.path.join(work, "profiles", profile), "--device-name", profile, *a],
                       capture_output=True, text=True, timeout=timeout, env=dict(os.environ, RUST_LOG="warn"))
    if check and r.returncode != 0:
        print(r.stdout); print(r.stderr)
        raise SystemExit(f"ursa {profile} {a[0]} failed ({r.returncode})")
    return r.stdout.strip()

def start_node(name):
    d = os.path.join(work, "nodes", name)
    os.makedirs(d, exist_ok=True)
    log_path = os.path.join(d, "node.log")
    p = subprocess.Popen([NODE, "--data-dir", d, "--health-port", "0"], stdout=open(log_path, "w"), stderr=subprocess.STDOUT)
    for _ in range(120):
        m = re.search(r"(ursa-claim:\S+)", open(log_path, encoding="utf-8", errors="replace").read())
        if m:
            return p, m.group(1)
        time.sleep(0.5)
    raise SystemExit(f"node {name} printed no claim link")

nodes = {}
if args.claims:
    claims = json.load(open(args.claims))
    nodes = {who: (None, claims[who]) for who in PEOPLE}
    log("five cluster nodes")
else:
    for who in PEOPLE:
        nodes[who] = start_node(who)
    log("five nodes up")

# 1. Accounts and home nodes.
ids = {}
for who in PEOPLE:
    extra = ["--key-file", os.path.join(work, f"{who}.ursa-key"), "--passphrase", PASS] if who == "alice" else []
    ids[who] = ursa(who, "create", "--name", who.capitalize(), *extra)
    ursa(who, "claim", nodes[who][1], "--label", f"{who}'s node")
log("five accounts, each with its own node")

# 2. Everyone befriends everyone (both directions: each grants the other space).
invites = {who: ursa(who, "invite") for who in PEOPLE}
for a in PEOPLE:
    for b in PEOPLE:
        if a != b:
            ursa(a, "befriend", invites[b], "--quota-gb", "5")
log("everyone befriended everyone")
st = json.loads(ursa("alice", "status"))
writable = [p for p in st["pool"] if p["reachable"] and p["info"]["role"] != "none"]
log(f"alice's pool: {len(st['pool'])} nodes, {len(writable)} writable")
assert len(writable) == 5, st["pool"]

# 3. Upload.
src = os.path.join(work, "src")
os.makedirs(src)
sizes = [200, 70_000, 1_500_000, 3_000_000, 9_000_000, 17_000_000][: args.files]
files = {}
for i, n in enumerate(sizes):
    p = os.path.join(src, f"file-{i}.bin")
    with open(p, "wb") as f:
        f.write(os.urandom(n))
    files[p] = hashlib.sha256(open(p, "rb").read()).hexdigest()
t = time.time()
out = ursa("alice", "upload", *files.keys(), timeout=900)
uploaded = dict(line.split("\t")[::-1] for line in out.splitlines())  # name -> id
log(f"uploaded {len(files)} files ({sum(sizes) / 1e6:.1f} MB) in {time.time() - t:.1f}s")

# 4. A second device for alice, by link code.
code_file = os.path.join(work, "link-code.txt")
offer = subprocess.Popen([URSA, "--profile", os.path.join(work, "profiles", "alice"), "--device-name", "alice", "link-offer", "--yes", "--code-file", code_file],
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
for _ in range(60):
    if os.path.exists(code_file) and open(code_file).read().strip():
        break
    time.sleep(0.5)
joined = ursa("alice-phone", "link-join", open(code_file).read().strip())
offer.wait(timeout=60)
assert joined == ids["alice"], (joined, ids["alice"])
listed = ursa("alice-phone", "ls")
assert len(listed.splitlines()) == len(files), listed
log("alice-phone joined by link code and sees the whole library")

def check_all(profile, label):
    out_dir = os.path.join(work, "out-" + label)
    os.makedirs(out_dir, exist_ok=True)
    t = time.time()
    for path, digest in files.items():
        name = os.path.basename(path)
        out = os.path.join(out_dir, name)
        ursa(profile, "get", uploaded[name], out, timeout=900)
        got = hashlib.sha256(open(out, "rb").read()).hexdigest()
        assert got == digest, f"{label}: {name} differs"
    log(f"{label}: all {len(files)} files byte-identical ({time.time() - t:.1f}s)")

check_all("alice", "before")

# 5. Destroy alice's node and its storage.
proc, _ = nodes["alice"]
if proc is None:
    subprocess.run(args.destroy, shell=True, check=True)
else:
    proc.kill(); proc.wait()
    shutil.rmtree(os.path.join(work, "nodes", "alice"))
log("alice's node and all its storage destroyed")

# 6. Recover on a fresh device from the key file alone.
out = ursa("alice-new-laptop", "recover", "--key-file", os.path.join(work, "alice.ursa-key"), "--passphrase", PASS, timeout=300)
assert out == ids["alice"], out
log("alice recovered on a fresh device from her key file (found through the pkarr locator)")

# 7. Every file, from the four surviving nodes.
check_all("alice-new-laptop", "after-loss")

# 8. A new home node, then repair.
nodes["alice-2"] = start_node("alice-2")
ursa("alice-new-laptop", "claim", nodes["alice-2"][1], "--label", "alice's new node")
report = json.loads(ursa("alice-new-laptop", "repair", timeout=1800))
log(f"repair: {report['objects_rebuilt']} rebuilt of {report['objects_checked']} checked, {len(report['unrecoverable'])} lost")
assert not report["unrecoverable"]
check_all("alice-new-laptop", "after-repair")

for p, _ in nodes.values():
    if p is not None and p.poll() is None:
        p.kill()
log("PASS")
