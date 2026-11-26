//! The library as folders: what every app browses, and the edits that change it. The model and
//! its merge rules are [`ursa_core::folders`]; this reads names from the manifests and writes
//! edits through the index like any other change.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use ursa_core::ObjectId;
use ursa_core::folders::{FolderId, Tree, check_name, new_folder_id};
use ursa_core::head::LibraryIndex;

use crate::library::{Item, UploadMeta};
use crate::{Session, now_ms};

/// A folder as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderItem {
    pub id: String,
    pub parent: Option<String>,
    pub name: String,
    /// `/`-separated from the root, its own name included.
    pub path: String,
}

/// Every folder and every file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryTree {
    pub folders: Vec<FolderItem>,
    pub files: Vec<Item>,
}

/// One change to the tree. Folder and file ids come from [`LibraryTree`]; `None` for a folder
/// means the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// `id` is made up when left out; give one to refer to the folder in later edits of the same batch.
    CreateFolder {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        parent: Option<String>,
        name: String,
    },
    RenameFolder { id: String, name: String },
    MoveFolder {
        id: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// Deletes the folder and, unless `only_folder`, everything in it.
    DeleteFolder {
        id: String,
        #[serde(default)]
        only_folder: bool,
    },
    RenameFile { file_id: String, name: String },
    MoveFile {
        file_id: String,
        #[serde(default)]
        dir: Option<String>,
    },
    DeleteFile { file_id: String },
}

fn file_id(s: &str) -> Result<ObjectId> {
    s.parse().map_err(|e| anyhow!("{s}: {e}"))
}

/// Applies `edits` to `idx`. Infallible: what was checked against one index may meet a newer one
/// in a merge, and the tree resolves whatever that leaves.
fn apply(idx: &mut LibraryIndex, edits: &[Edit], now: i64) {
    for e in edits {
        match e {
            Edit::CreateFolder { id, parent, name } => {
                idx.place_folder(id.as_ref().expect("ids are filled in before"), parent.clone(), name, now)
            }
            Edit::RenameFolder { id, name } => {
                if let Some(f) = idx.folders.get(id).cloned() {
                    idx.place_folder(id, f.parent, name, now);
                }
            }
            Edit::MoveFolder { id, parent } => {
                if let Some(f) = idx.folders.get(id).cloned() {
                    idx.place_folder(id, parent.clone(), &f.name, now);
                }
            }
            Edit::DeleteFolder { id, only_folder } => {
                let mut gone: HashSet<FolderId> = HashSet::from([id.clone()]);
                if !only_folder {
                    loop {
                        let before = gone.len();
                        for (fid, f) in &idx.folders {
                            if f.parent.as_ref().is_some_and(|p| gone.contains(p)) {
                                gone.insert(fid.clone());
                            }
                        }
                        if gone.len() == before {
                            break;
                        }
                    }
                    let files: Vec<ObjectId> =
                        idx.entries.iter().filter(|(_, e)| e.dir.as_ref().is_some_and(|d| gone.contains(d))).map(|(i, _)| *i).collect();
                    for f in files {
                        idx.delete(f);
                    }
                }
                for f in &gone {
                    idx.delete_folder(f);
                }
            }
            Edit::RenameFile { file_id: f, name } => {
                if let Ok(id) = file_id(f) {
                    let dir = idx.entries.get(&id).and_then(|e| e.dir.clone());
                    idx.place_file(&id, dir, Some(name.clone()), now);
                }
            }
            Edit::MoveFile { file_id: f, dir } => {
                if let Ok(id) = file_id(f) {
                    idx.place_file(&id, dir.clone(), None, now);
                }
            }
            Edit::DeleteFile { file_id: f } => {
                if let Ok(id) = file_id(f) {
                    idx.delete(id);
                }
            }
        }
    }
}

impl Session {
    /// The tree with the manifests' names, and the items for its files.
    pub(crate) async fn resolved(&self) -> Result<(LibraryIndex, Tree, HashMap<ObjectId, Item>)> {
        let (idx, files) = self.live_files().await?;
        let names: HashMap<ObjectId, String> = files.iter().map(|(id, m)| (*id, m.name.clone())).collect();
        let tree = idx.tree(|id| names.get(id).cloned());
        let items = files.iter().map(|(id, m)| (*id, Item::of(id, m))).collect();
        Ok((idx, tree, items))
    }

    pub async fn tree(&self) -> Result<LibraryTree> {
        let (_, tree, mut items) = self.resolved().await?;
        let mut folders: Vec<FolderItem> = tree
            .folders
            .iter()
            .map(|(id, f)| FolderItem { id: id.clone(), parent: f.parent.clone(), name: f.name.clone(), path: tree.folder_path(Some(id)) })
            .collect();
        folders.sort_by_key(|f| f.path.to_lowercase());
        let files = tree
            .files
            .iter()
            .filter_map(|(id, f)| {
                let mut item = items.remove(id)?;
                item.name = f.name.clone();
                item.dir = f.dir.clone();
                item.path = tree.folder_path(f.dir.as_ref());
                Some(item)
            })
            .collect();
        Ok(LibraryTree { folders, files })
    }

    /// Checks `edits` one after another against the tree (names, clashes, a folder going inside
    /// itself) and writes them in one index change. Returns the ids of the folders it created.
    pub async fn edit(&self, edits: Vec<Edit>) -> Result<Vec<String>> {
        self.edit_with(edits, true).await
    }

    /// `check: false` skips the clash checks, for a sync whose names are already unique on disk;
    /// the tree numbers any clash with what it has not seen yet.
    pub(crate) async fn edit_with(&self, mut edits: Vec<Edit>, check: bool) -> Result<Vec<String>> {
        if edits.is_empty() {
            return Ok(vec![]);
        }
        let (mut idx, _, items) = self.resolved().await?;
        let names: HashMap<ObjectId, String> = items.iter().map(|(id, i)| (*id, i.name.clone())).collect();
        let mut created = Vec::new();
        let now = now_ms();
        for e in &mut edits {
            let tree = idx.tree(|id| names.get(id).cloned());
            let live_folder = |id: &str| tree.folders.contains_key(id);
            let need_folder = |id: &Option<String>| -> Result<()> {
                match id {
                    Some(d) if !live_folder(d) => bail!("no such folder"),
                    _ => Ok(()),
                }
            };
            match e {
                Edit::CreateFolder { id, parent, name } => {
                    *name = check_name(name).map_err(|e| anyhow!(e))?;
                    need_folder(parent)?;
                    if check && tree.taken(parent.as_ref(), name, None) {
                        bail!("\"{name}\" is already there");
                    }
                    let id = id.get_or_insert_with(new_folder_id);
                    if idx.folders.contains_key(id.as_str()) {
                        bail!("folder {id} already exists");
                    }
                    created.push(id.clone());
                }
                Edit::RenameFolder { id, name } => {
                    *name = check_name(name).map_err(|e| anyhow!(e))?;
                    let f = tree.folders.get(id.as_str()).context("no such folder")?;
                    if check && tree.taken(f.parent.as_ref(), name, Some(id)) {
                        bail!("\"{name}\" is already there");
                    }
                }
                Edit::MoveFolder { id, parent } => {
                    let f = tree.folders.get(id.as_str()).context("no such folder")?;
                    need_folder(parent)?;
                    if idx.would_cycle(id, parent.as_ref()) {
                        bail!("a folder can't go inside itself");
                    }
                    if check && tree.taken(parent.as_ref(), &f.name, Some(id)) {
                        bail!("\"{}\" is already there", f.name);
                    }
                }
                Edit::DeleteFolder { id, .. } => {
                    tree.folders.get(id.as_str()).context("no such folder")?;
                }
                Edit::RenameFile { file_id: f, name } => {
                    *name = check_name(name).map_err(|e| anyhow!(e))?;
                    let t = tree.files.get(&file_id(f)?).context("no such file in the library")?;
                    if check && tree.taken(t.dir.as_ref(), name, Some(f)) {
                        bail!("\"{name}\" is already there");
                    }
                }
                Edit::MoveFile { file_id: f, dir } => {
                    let t = tree.files.get(&file_id(f)?).context("no such file in the library")?;
                    need_folder(dir)?;
                    if check && tree.taken(dir.as_ref(), &t.name, Some(f)) {
                        bail!("\"{}\" is already there", t.name);
                    }
                }
                Edit::DeleteFile { file_id: f } => {
                    let id = file_id(f)?;
                    if !idx.entries.get(&id).is_some_and(|e| !e.deleted) {
                        bail!("no such file in the library");
                    }
                }
            }
            apply(&mut idx, std::slice::from_ref(e), now);
        }
        self.change_index(move |idx| apply(idx, &edits, now)).await?;
        Ok(created)
    }

    pub async fn create_folder(&self, parent: Option<String>, name: &str) -> Result<String> {
        let ids = self.edit(vec![Edit::CreateFolder { id: None, parent, name: name.into() }]).await?;
        ids.into_iter().next().context("no folder made")
    }

    /// Makes every folder of `path` (`/`-separated) that is not there yet; returns the last one's id.
    pub async fn make_folders(&self, path: &str) -> Result<Option<String>> {
        let tree = self.resolved().await?.1;
        let mut at: Option<FolderId> = None;
        let mut edits = Vec::new();
        let mut walked = String::new();
        let mut creating = false;
        for part in path.split(['/', '\\']).filter(|p| !p.is_empty()) {
            walked = if walked.is_empty() { part.to_string() } else { format!("{walked}/{part}") };
            if !creating && let Some(found) = tree.find_folder(&walked) {
                at = found;
                continue;
            }
            creating = true;
            let id = new_folder_id();
            edits.push(Edit::CreateFolder { id: Some(id.clone()), parent: at.clone(), name: part.to_string() });
            at = Some(id);
        }
        self.edit(edits).await?;
        Ok(at)
    }

    /// Uploads a folder from this computer, everything in it, into `parent` (`None`: the root):
    /// its folders in one change, its files staged and written together. `progress` gets bytes.
    /// Returns the new folder's id and how many files went up.
    pub async fn upload_folder(&self, local: &Path, parent: Option<String>, progress: impl Fn(u64, u64)) -> Result<(String, usize)> {
        let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).context("that folder has no name")?;
        let (dirs, files) = {
            let local = local.to_path_buf();
            tokio::task::spawn_blocking(move || walk(&local)).await??
        };
        let top = new_folder_id();
        let mut ids: HashMap<PathBuf, String> = HashMap::from([(PathBuf::new(), top.clone())]);
        let mut edits = vec![Edit::CreateFolder { id: Some(top.clone()), parent, name }];
        for d in &dirs {
            let id = new_folder_id();
            let parent = ids[d.parent().unwrap_or(Path::new(""))].clone();
            let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            edits.push(Edit::CreateFolder { id: Some(id.clone()), parent: Some(parent), name });
            ids.insert(d.clone(), id);
        }
        // Names on disk are already unique in their folder; a clash with what is there is numbered.
        self.edit_with(edits, false).await?;
        let total: u64 = files.iter().map(|(_, len)| len).sum();
        let mut done = 0u64;
        progress(0, total);
        for (n, (rel, len)) in files.iter().enumerate() {
            let dir = ids[rel.parent().unwrap_or(Path::new(""))].clone();
            let meta = UploadMeta { path: local.join(rel), dir: Some(dir), mime: Some(guess_mime(&rel.to_string_lossy()).into()), ..Default::default() };
            let base = done;
            self.upload_staged(meta, |d, _| progress(base + d, total)).await?;
            done += len;
            // Written in batches: a stop part-way keeps what went before.
            if (n + 1) % 50 == 0 {
                self.commit_uploads().await?;
            }
        }
        self.commit_uploads().await?;
        progress(total, total);
        Ok((top, files.len()))
    }

    /// Downloads a folder and everything in it into `out_dir`, as `out_dir/<folder name>/…`.
    /// Returns the folder written. `progress` gets bytes.
    pub async fn download_folder(&self, folder: &str, out_dir: &Path, progress: impl Fn(u64, u64)) -> Result<PathBuf> {
        let tree = self.tree().await?;
        let top = tree.folders.iter().find(|f| f.id == folder).context("no such folder")?;
        let mut under: HashMap<String, PathBuf> = HashMap::from([(top.id.clone(), out_dir.join(disk_name(&top.name)))]);
        // Parents before children.
        let mut rest: Vec<&FolderItem> = tree.folders.iter().filter(|f| f.id != folder).collect();
        loop {
            let before = under.len();
            rest.retain(|f| match f.parent.as_ref().and_then(|p| under.get(p)).cloned() {
                Some(p) => {
                    under.insert(f.id.clone(), p.join(disk_name(&f.name)));
                    false
                }
                None => true,
            });
            if under.len() == before {
                break;
            }
        }
        for d in under.values() {
            tokio::fs::create_dir_all(d).await.with_context(|| format!("create {}", d.display()))?;
        }
        let files: Vec<(&Item, PathBuf)> =
            tree.files.iter().filter_map(|i| Some((i, under.get(i.dir.as_ref()?)?.join(disk_name(&i.name))))).collect();
        let total: u64 = files.iter().map(|(i, _)| i.size).sum();
        let mut done = 0u64;
        progress(0, total);
        for (item, path) in files {
            let base = done;
            self.download(&item.file_id, &path, |d, _| progress(base + d, total)).await?;
            done += item.size;
        }
        progress(total, total);
        Ok(under.remove(folder).expect("the top folder"))
    }
}

/// A folder's subfolders and files, relative to it, parents first; files with their sizes.
/// Links are not followed.
#[allow(clippy::type_complexity)]
fn walk(root: &Path) -> Result<(Vec<PathBuf>, Vec<(PathBuf, u64)>)> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut queue = vec![PathBuf::new()];
    while let Some(rel) = queue.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(root.join(&rel))
            .with_context(|| format!("read {}", root.join(&rel).display()))?
            .collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let t = e.file_type()?;
            let r = rel.join(e.file_name());
            if t.is_dir() {
                dirs.push(r.clone());
                queue.push(r);
            } else if t.is_file() {
                files.push((r, e.metadata()?.len()));
            }
        }
    }
    dirs.sort_by_key(|d| d.components().count());
    Ok((dirs, files))
}

/// Names Windows refuses as file names whatever the extension.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A library name as a file name on any system: what Windows refuses becomes `_`. Names made in
/// Ursa are already safe; uploads from other devices may not be.
pub fn disk_name(name: &str) -> String {
    const BAD: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let mut n: String = name.chars().map(|c| if BAD.contains(&c) || c.is_control() { '_' } else { c }).collect();
    while n.ends_with('.') || n.ends_with(' ') {
        n.pop();
    }
    if n.is_empty() {
        n = "_".into();
    }
    let stem = n.split('.').next().unwrap_or_default().trim_end().to_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        n.insert(0, '_');
    }
    n
}

/// A MIME type from a file name, for files that arrive without one.
pub fn guess_mime(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_names_become_file_names() {
        assert_eq!(disk_name("a:b?.txt"), "a_b_.txt");
        assert_eq!(disk_name("con.txt"), "_con.txt");
        assert_eq!(disk_name("trailing. "), "trailing");
        assert_eq!(disk_name("fine name.jpg"), "fine name.jpg");
        assert_eq!(guess_mime("A.JPG"), "image/jpeg");
    }

    #[test]
    fn a_walk_lists_parents_first() {
        let root = std::env::temp_dir().join(format!("ursa-walk-{}", new_folder_id()));
        std::fs::create_dir_all(root.join("a").join("b")).unwrap();
        std::fs::write(root.join("a").join("b").join("x.txt"), b"x").unwrap();
        std::fs::write(root.join("top.txt"), b"hello").unwrap();
        let (dirs, files) = walk(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(dirs, vec![PathBuf::from("a"), PathBuf::from("a").join("b")]);
        let mut files: Vec<_> = files.into_iter().map(|(p, n)| (p.components().count(), n)).collect();
        files.sort();
        assert_eq!(files, vec![(1, 5), (3, 1)]);
    }
}
