"""Folders: a tree goes up, comes back byte-identical, and renames, moves and deletes hold.

Two people with a node each, befriended so alice's pool has two nodes. Alice uploads a folder
tree (nested folders, empty folder, files of several sizes, a name that clashes ignoring case
with one already there), then:

  - the tree lists every folder and file at its path
  - the folder downloads byte-identical, layout included
  - renaming a folder moves everything under it; moving a file and a folder; renaming a file
  - a second device of alice's (linked) sees the same tree
  - deleting a folder deletes what is inside it; the rest stays and still downloads

  python -I scripts/folders-test.py --bin target/debug --work <dir>
"""
import argparse, atexit, hashlib, json, os, re, shutil, subprocess, threading, time

ap = argparse.ArgumentParser()
ap.add_argument("--bin", required=True, help="directory with ursa and ursa-node")
ap.add_argument("--work", required=True)
ap.add_argument("--port-base", type=int, default=47410)
args = ap.parse_args()

EXE = ".exe" if os.name == "nt" else ""
URSA = os.path.join(args.bin, "ursa" + EXE)
NODE = os.path.join(args.bin, "ursa-node" + EXE)
work = os.path.abspath(args.work)
shutil.rmtree(work, ignore_errors=True)
os.makedirs(work)
PEOPLE = ["alice", "bob"]
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
    for p in procs.values():
        if p.poll() is None:
            p.kill()


def start_node(name):
    d = os.path.join(work, "nodes", name)
    os.makedirs(d, exist_ok=True)
    log_path = os.path.join(d, "node.log")
    port = args.port_base + PEOPLE.index(name)
    p = subprocess.Popen([NODE, "--data-dir", d, "--health-port", "0", "--port", str(port)],
                         stdout=open(log_path, "w"), stderr=subprocess.STDOUT)
    procs[name] = p
    for _ in range(120):
        m = re.search(r"(ursa-claim:\S+)", open(log_path, encoding="utf-8", errors="replace").read())
        if m:
            return m.group(1)
        time.sleep(0.5)
    raise SystemExit(f"node {name} did not start")


claims = {who: start_node(who) for who in PEOPLE}
for who in PEOPLE:
    ursa(who, "create", "--name", who.capitalize())
    ursa(who, "claim", claims[who], "--label", f"{who}'s node")
invites = {who: ursa(who, "invite") for who in PEOPLE}
ursa("alice", "befriend", invites["bob"], "--quota-gb", "2")
ursa("bob", "befriend", invites["alice"], "--quota-gb", "2")
log("two nodes, alice and bob befriended")


def digest(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def snapshot(root):
    """Every folder and file under root: relative path -> hash (None for a folder)."""
    out = {}
    for d, dirs, files in os.walk(root):
        rel = os.path.relpath(d, root).replace("\\", "/")
        if rel != ".":
            out[rel + "/"] = None
        for f in files:
            out[(f if rel == "." else f"{rel}/{f}")] = digest(os.path.join(d, f))
    return out


def tree():
    return json.loads(ursa("alice", "tree"))


def paths(t):
    out = {f["path"] + "/" for f in t["folders"]}
    out |= {(i["path"] + "/" if i["path"] else "") + i["name"] for i in t["files"]}
    return out


# A loose file at the root first, whose name the upload's folder will not clash with.
src = os.path.join(work, "src", "Trip")
for d in ["Day 1/Photos", "Day 2", "Empty"]:
    os.makedirs(os.path.join(src, d))
sizes = {"notes.txt": 120, "Day 1/plan.pdf": 90_000, "Day 1/Photos/a.jpg": 2_500_000, "Day 1/Photos/b.jpg": 700_000,
         "Day 2/video.mp4": 6_000_000, "Day 2/README": 0}
for rel, n in sizes.items():
    with open(os.path.join(src, rel), "wb") as f:
        f.write(os.urandom(n))
before = snapshot(src)

loose = os.path.join(work, "src", "loose.txt")
open(loose, "w").write("at the root")
ursa("alice", "upload", loose)
ursa("alice", "upload-folder", src)
t = tree()
want = {"loose.txt", "Trip/"} | {"Trip/" + p for p in before}
assert paths(t) == want, f"tree differs:\n got {sorted(paths(t))}\nwant {sorted(want)}"
log(f"uploaded a tree of {len(sizes)} files in {len(t['folders'])} folders; it lists as on disk")

# A second copy of the folder: the library numbers the clash instead of merging.
ursa("alice", "upload-folder", src)
assert "Trip (2)/" in paths(tree()), sorted(paths(tree()))
ursa("alice", "del", "Trip (2)")
assert not any(p.startswith("Trip (2)") for p in paths(tree()))
log("a second upload of the same folder is numbered, and deletes cleanly")

out = os.path.join(work, "out1")
ursa("alice", "get-folder", "Trip", out)
assert snapshot(os.path.join(out, "Trip")) == before, "download differs from what went up"
log("the folder downloads byte-identical, empty folder included")

ursa("alice", "mv", "Trip", "Holiday")
ursa("alice", "mv", "Holiday/Day 2/video.mp4", "Holiday/Day 1")
ursa("alice", "mv", "Holiday/Day 1/plan.pdf", "Holiday/itinerary.pdf")
ursa("alice", "mkdir", "Archive/2024")
ursa("alice", "mv", "Holiday/Day 1/Photos", "Archive/2024")
ursa("alice", "mv", "loose.txt", "Archive/loose.txt")
expect = {
    "Holiday/", "Holiday/Day 1/", "Holiday/Day 2/", "Holiday/Empty/", "Holiday/notes.txt", "Holiday/itinerary.pdf",
    "Holiday/Day 1/video.mp4", "Holiday/Day 2/README", "Archive/", "Archive/2024/", "Archive/2024/Photos/",
    "Archive/2024/Photos/a.jpg", "Archive/2024/Photos/b.jpg", "Archive/loose.txt",
}
assert paths(tree()) == expect, f"after moves:\n got {sorted(paths(tree()))}\nwant {sorted(expect)}"
r = ursa("alice", "mv", "Archive", "Archive/2024", check=False)
assert r.returncode != 0 and "inside itself" in r.stderr, r.stderr
log("renames and moves of files and folders hold; a folder can't go inside itself")

out = os.path.join(work, "out2")
ursa("alice", "get-folder", "Archive", out)
got = snapshot(os.path.join(out, "Archive"))
assert got["2024/Photos/a.jpg"] == before["Day 1/Photos/a.jpg"] and got["loose.txt"] == digest(loose)
log("moved files still download byte-identical")

# A second device of alice's sees the same tree.
code_file = os.path.join(work, "code.txt")
offer = subprocess.Popen([URSA, "--profile", os.path.join(work, "profiles", "alice"), "--device-name", "alice", "link-offer", "--yes",
                          "--code-file", code_file], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
for _ in range(120):
    if os.path.exists(code_file) and open(code_file).read().strip():
        break
    time.sleep(0.5)
ursa("alice-phone", "link-join", open(code_file).read().strip())
offer.wait(timeout=120)
phone = json.loads(ursa("alice-phone", "tree"))
assert paths(phone) == expect, sorted(paths(phone))
log("a linked device sees the same tree")

ursa("alice", "del", "Holiday")
ursa("alice", "del", "Archive/loose.txt")
left = paths(tree())
assert left == {"Archive/", "Archive/2024/", "Archive/2024/Photos/", "Archive/2024/Photos/a.jpg", "Archive/2024/Photos/b.jpg"}, sorted(left)
ls = ursa("alice", "ls")
assert len(ls.splitlines()) == 2, ls
log("deleting a folder deletes what is in it; the rest stays")
log("PASS")
