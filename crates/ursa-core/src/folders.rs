//! Folders: the library as a tree, the one model every app shows.
//!
//! The index keeps folders by a random id (parent, name), and each file entry keeps the folder
//! it is in and, once renamed, its name. Moving or renaming a folder changes one record, never
//! its contents. A file with no folder, or whose folder is gone, is at the root, so libraries
//! from before folders open as one flat root.
//!
//! Every placement carries the time it was made and the newest wins a merge; a deletion is
//! final. Merges can leave what no single device would make (a folder inside a deleted one, a
//! cycle from two concurrent moves, two siblings with one name); [`LibraryIndex::tree`] resolves
//! all of it the same way on every device, so they all show the same tree.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::ObjectId;
use crate::head::LibraryIndex;

/// A folder's id: 16 random bytes, hex.
pub type FolderId = String;

pub fn new_folder_id() -> FolderId {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS randomness");
    hex::encode(b)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// `None`: at the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<FolderId>,
    pub name: String,
    /// When its parent or name last changed, ms; the newest wins a merge.
    #[serde(default)]
    pub changed_ms: i64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

impl Folder {
    /// Takes `other`'s placement if it is newer (ties broken by content, so both sides agree).
    pub(crate) fn merge(&mut self, other: &Folder) {
        self.deleted |= other.deleted;
        if (other.changed_ms, &other.parent, &other.name) > (self.changed_ms, &self.parent, &self.name) {
            self.parent = other.parent.clone();
            self.name = other.name.clone();
            self.changed_ms = other.changed_ms;
        }
    }
}

/// Characters no name may hold: path separators, and what Windows refuses in a file name, so a
/// name made on one device can be a file name on every other.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Checks a folder or file name someone typed. Returns it trimmed.
pub fn check_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("a name can't be empty".into());
    }
    if n == "." || n == ".." {
        return Err(format!("\"{n}\" can't be a name"));
    }
    if n.chars().count() > 255 {
        return Err("that name is too long".into());
    }
    if let Some(c) = n.chars().find(|c| FORBIDDEN.contains(c) || c.is_control()) {
        return Err(format!("a name can't contain {c:?}"));
    }
    if n.ends_with('.') {
        return Err("a name can't end with a dot".into());
    }
    Ok(n.to_string())
}

/// How two names compare for clashes: as Windows and most phones do, ignoring case.
pub fn name_key(name: &str) -> String {
    name.to_lowercase()
}

/// A folder as the tree shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeFolder {
    pub parent: Option<FolderId>,
    /// Unique among its siblings (ignoring case).
    pub name: String,
}

/// A file as the tree shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeFile {
    pub dir: Option<FolderId>,
    /// Unique among its siblings (ignoring case).
    pub name: String,
}

/// The library as one consistent tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub folders: BTreeMap<FolderId, TreeFolder>,
    pub files: BTreeMap<ObjectId, TreeFile>,
}

impl Tree {
    /// The folder's path from the root, `/`-separated; `""` for the root.
    pub fn folder_path(&self, id: Option<&FolderId>) -> String {
        let mut parts = Vec::new();
        let mut at = id;
        while let Some(f) = at.and_then(|i| self.folders.get(i)) {
            parts.push(f.name.as_str());
            at = f.parent.as_ref();
        }
        parts.reverse();
        parts.join("/")
    }

    pub fn file_path(&self, id: &ObjectId) -> Option<String> {
        let f = self.files.get(id)?;
        let dir = self.folder_path(f.dir.as_ref());
        Some(if dir.is_empty() { f.name.clone() } else { format!("{dir}/{}", f.name) })
    }

    /// The folder and every folder under it.
    pub fn subtree(&self, id: &FolderId) -> HashSet<FolderId> {
        let mut out: HashSet<FolderId> = HashSet::from([id.clone()]);
        loop {
            let before = out.len();
            for (fid, f) in &self.folders {
                if f.parent.as_ref().is_some_and(|p| out.contains(p)) {
                    out.insert(fid.clone());
                }
            }
            if out.len() == before {
                return out;
            }
        }
    }

    /// Whether `name` is taken in `dir` by anything but `except`.
    pub fn taken(&self, dir: Option<&FolderId>, name: &str, except: Option<&str>) -> bool {
        let key = name_key(name);
        self.folders
            .iter()
            .any(|(id, f)| f.parent.as_ref() == dir && name_key(&f.name) == key && Some(id.as_str()) != except)
            || self.files.iter().any(|(id, f)| {
                f.dir.as_ref() == dir && name_key(&f.name) == key && Some(id.to_hex().as_str()) != except
            })
    }

    /// The folder at `path` (`/`-separated, matching names ignoring case); `Some(None)` is the root.
    pub fn find_folder(&self, path: &str) -> Option<Option<FolderId>> {
        let mut at: Option<FolderId> = None;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let key = name_key(part);
            let (id, _) = self.folders.iter().find(|(_, f)| f.parent == at && name_key(&f.name) == key)?;
            at = Some(id.clone());
        }
        Some(at)
    }
}

/// `"photo.jpg"`, 2 → `"photo (2).jpg"`; a folder (or a name with no extension) gets it at the end.
fn numbered(name: &str, n: usize, is_dir: bool) -> String {
    match name.rfind('.').filter(|&i| !is_dir && i > 0) {
        Some(i) => format!("{} ({n}){}", &name[..i], &name[i..]),
        None => format!("{name} ({n})"),
    }
}

impl LibraryIndex {
    /// Adds a folder or changes where it is and what it is called.
    pub fn place_folder(&mut self, id: &FolderId, parent: Option<FolderId>, name: &str, now_ms: i64) {
        let f = self.folders.entry(id.clone()).or_insert_with(|| Folder {
            parent: None,
            name: String::new(),
            changed_ms: 0,
            deleted: false,
        });
        f.parent = parent;
        f.name = name.to_string();
        f.changed_ms = now_ms.max(f.changed_ms + 1);
    }

    pub fn delete_folder(&mut self, id: &FolderId) {
        if let Some(f) = self.folders.get_mut(id) {
            f.deleted = true;
        }
    }

    /// Moves a file into `dir` (`None`: the root) and, with `name`, renames it.
    pub fn place_file(&mut self, id: &ObjectId, dir: Option<FolderId>, name: Option<String>, now_ms: i64) {
        if let Some(e) = self.entries.get_mut(id) {
            e.dir = dir;
            if name.is_some() {
                e.name = name;
            }
            e.placed_ms = now_ms.max(e.placed_ms + 1);
        }
    }

    /// Whether putting folder `id` inside `parent` would put it inside itself.
    pub fn would_cycle(&self, id: &FolderId, parent: Option<&FolderId>) -> bool {
        let mut at = parent.cloned();
        let mut seen = HashSet::new();
        while let Some(p) = at {
            if &p == id {
                return true;
            }
            if !seen.insert(p.clone()) {
                return false;
            }
            at = self.folders.get(&p).and_then(|f| f.parent.clone());
        }
        false
    }

    /// The tree every device shows. `manifest_name` gives a file's name as uploaded (from its
    /// manifest); a live file it has no name for is left out.
    pub fn tree(&self, manifest_name: impl Fn(&ObjectId) -> Option<String>) -> Tree {
        let live: HashMap<&FolderId, &Folder> = self.folders.iter().filter(|(_, f)| !f.deleted).collect();
        // A parent that is gone puts a folder at the root.
        let mut parent: HashMap<&FolderId, Option<&FolderId>> = live
            .iter()
            .map(|(id, f)| (*id, f.parent.as_ref().and_then(|p| live.get_key_value(p).map(|(k, _)| *k))))
            .collect();
        // Break cycles: the smallest id in each goes to the root.
        let ids: Vec<&FolderId> = {
            let mut v: Vec<&FolderId> = live.keys().copied().collect();
            v.sort();
            v
        };
        for start in &ids {
            let mut path: Vec<&FolderId> = Vec::new();
            let mut at = Some(*start);
            while let Some(f) = at {
                if let Some(i) = path.iter().position(|p| *p == f) {
                    let smallest = path[i..].iter().min().copied().expect("non-empty");
                    parent.insert(smallest, None);
                    break;
                }
                path.push(f);
                at = parent[f];
            }
        }

        // Siblings, in the order that decides who keeps a clashing name: folders, then files,
        // each oldest-placed first, then by id.
        enum Node<'a> {
            Folder(&'a FolderId),
            File(&'a ObjectId),
        }
        // (kind, placed, id, name, node) per sibling.
        type Sibling<'a> = (u8, i64, String, String, Node<'a>);
        let mut children: BTreeMap<Option<&FolderId>, Vec<Sibling>> = BTreeMap::new();
        for id in &ids {
            let f = live[id];
            children.entry(parent[id]).or_default().push((
                0,
                f.changed_ms,
                (*id).clone(),
                f.name.clone(),
                Node::Folder(id),
            ));
        }
        for (id, e) in self.entries.iter().filter(|(_, e)| !e.deleted) {
            let Some(name) = e.name.clone().or_else(|| manifest_name(id)) else { continue };
            let dir = e.dir.as_ref().and_then(|d| live.get_key_value(d).map(|(k, _)| *k));
            children.entry(dir).or_default().push((1, e.placed_ms, id.to_hex(), name, Node::File(id)));
        }

        let mut tree = Tree::default();
        for (dir, mut kids) in children {
            kids.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
            let mut used: HashSet<String> = HashSet::new();
            for (_, _, _, name, node) in kids {
                let is_dir = matches!(node, Node::Folder(_));
                let mut shown = name.clone();
                let mut n = 2;
                while !used.insert(name_key(&shown)) {
                    shown = numbered(&name, n, is_dir);
                    n += 1;
                }
                match node {
                    Node::Folder(id) => {
                        tree.folders.insert(id.clone(), TreeFolder { parent: dir.cloned(), name: shown });
                    }
                    Node::File(id) => {
                        tree.files.insert(*id, TreeFile { dir: dir.cloned(), name: shown });
                    }
                }
            }
        }
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(idx: &mut LibraryIndex, tag: &str) -> ObjectId {
        let id = ObjectId::of(tag.as_bytes());
        idx.add(id, 1, None);
        id
    }

    fn names(tag: &ObjectId) -> Option<String> {
        Some(format!("{}.txt", &tag.to_hex()[..4]))
    }

    #[test]
    fn an_old_library_is_one_flat_root() {
        let mut idx = LibraryIndex::default();
        let a = file(&mut idx, "a");
        let t = idx.tree(names);
        assert!(t.folders.is_empty());
        assert_eq!(t.files[&a].dir, None);
        assert_eq!(t.file_path(&a).unwrap(), names(&a).unwrap());
    }

    #[test]
    fn old_index_json_still_reads_and_new_fields_round_trip() {
        let old = r#"{"entries":{}}"#;
        let idx: LibraryIndex = serde_json::from_str(old).unwrap();
        assert!(idx.folders.is_empty());
        let mut idx = LibraryIndex::default();
        let a = file(&mut idx, "a");
        let d = new_folder_id();
        idx.place_folder(&d, None, "Docs", 5);
        idx.place_file(&a, Some(d.clone()), Some("x.txt".into()), 6);
        let back: LibraryIndex = serde_json::from_slice(&serde_json::to_vec(&idx).unwrap()).unwrap();
        assert_eq!(back, idx);
    }

    #[test]
    fn unknown_fields_survive_a_rewrite() {
        // What a newer client adds, an older one must carry over when it writes the index.
        let json = r#"{"entries":{},"future":{"x":1}}"#;
        let idx: LibraryIndex = serde_json::from_str(json).unwrap();
        let out = serde_json::to_value(&idx).unwrap();
        assert_eq!(out["future"]["x"], 1);
    }

    #[test]
    fn paths_moves_and_renames() {
        let mut idx = LibraryIndex::default();
        let a = file(&mut idx, "a");
        let (docs, work) = (new_folder_id(), new_folder_id());
        idx.place_folder(&docs, None, "Docs", 1);
        idx.place_folder(&work, Some(docs.clone()), "Work", 1);
        idx.place_file(&a, Some(work.clone()), Some("plan.txt".into()), 2);
        assert_eq!(idx.tree(names).file_path(&a).unwrap(), "Docs/Work/plan.txt");
        // Renaming a folder changes one record and moves everything under it.
        idx.place_folder(&docs, None, "Papers", 3);
        let t = idx.tree(names);
        assert_eq!(t.file_path(&a).unwrap(), "Papers/Work/plan.txt");
        assert_eq!(t.find_folder("papers/WORK"), Some(Some(work.clone())));
        assert!(idx.would_cycle(&docs, Some(&work)));
        assert!(!idx.would_cycle(&work, None));
    }

    #[test]
    fn a_deleted_folder_leaves_its_concurrent_additions_at_the_root() {
        let mut base = LibraryIndex::default();
        let d = new_folder_id();
        base.place_folder(&d, None, "Gone", 1);
        let mut a = base.clone();
        a.delete_folder(&d);
        let mut b = base.clone();
        let f = file(&mut b, "new");
        b.place_file(&f, Some(d.clone()), None, 2);
        a.merge(&b);
        let t = a.tree(names);
        assert!(t.folders.is_empty());
        assert_eq!(t.files[&f].dir, None, "nothing is lost: it shows at the root");
    }

    #[test]
    fn concurrent_moves_that_make_a_cycle_resolve_the_same_everywhere() {
        let mut base = LibraryIndex::default();
        let (x, y) = (new_folder_id(), new_folder_id());
        base.place_folder(&x, None, "X", 1);
        base.place_folder(&y, None, "Y", 1);
        let mut a = base.clone();
        a.place_folder(&x, Some(y.clone()), "X", 2);
        let mut b = base.clone();
        b.place_folder(&y, Some(x.clone()), "Y", 3);
        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab, ba);
        let t = ab.tree(names);
        let roots = t.folders.values().filter(|f| f.parent.is_none()).count();
        assert_eq!(roots, 1, "one of them goes back to the root: {t:?}");
    }

    #[test]
    fn newest_placement_wins_a_merge_either_way() {
        let mut base = LibraryIndex::default();
        let f = file(&mut base, "f");
        let (d1, d2) = (new_folder_id(), new_folder_id());
        base.place_folder(&d1, None, "One", 1);
        base.place_folder(&d2, None, "Two", 1);
        let mut a = base.clone();
        a.place_file(&f, Some(d1.clone()), None, 10);
        let mut b = base.clone();
        b.place_file(&f, Some(d2.clone()), Some("renamed.txt".into()), 20);
        for (mut x, y) in [(a.clone(), b.clone()), (b.clone(), a.clone())] {
            x.merge(&y);
            let e = &x.entries[&f];
            assert_eq!((e.dir.as_ref(), e.name.as_deref()), (Some(&d2), Some("renamed.txt")));
        }
    }

    #[test]
    fn clashing_names_are_numbered_the_same_everywhere() {
        let mut idx = LibraryIndex::default();
        let (a, b) = (file(&mut idx, "a"), file(&mut idx, "b"));
        idx.place_file(&a, None, Some("Same.txt".into()), 1);
        idx.place_file(&b, None, Some("same.txt".into()), 2);
        let d = new_folder_id();
        idx.place_folder(&d, None, "same.txt", 3);
        let t = idx.tree(names);
        // The folder keeps the name; files are numbered oldest first.
        assert_eq!(t.folders[&d].name, "same.txt");
        assert_eq!(t.files[&a].name, "Same (2).txt");
        assert_eq!(t.files[&b].name, "same (3).txt");
        assert!(t.taken(None, "SAME.TXT", None));
        assert!(!t.taken(None, "other", None));
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(check_name("  Holiday  ").unwrap(), "Holiday");
        for bad in ["", " ", "..", "a/b", "a\\b", "what?", "dot."] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(numbered("a.tar.gz", 2, false), "a.tar (2).gz");
        assert_eq!(numbered(".env", 2, false), ".env (2)");
        assert_eq!(numbered("v1.0", 2, true), "v1.0 (2)");
    }
}
