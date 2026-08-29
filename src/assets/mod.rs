//! Where assets come from.
//!
//! Everything in a film refers to media by a short string — `"map.png"`,
//! `"clips/battle-04.mp4"`. [`AssetStore`] turns that string into a local file.
//! Today it does so by searching a list of roots. That indirection is the seam
//! the brief asked to leave open: a fetcher (MCP, an HTTP cache, a content
//! store) becomes another [`Resolver`], and no film, layer or renderer changes.
//! Nothing else in the crate opens a path by itself.

pub mod clip;
pub mod data;
pub mod still;

pub use clip::{Clip, ClipLoop};
pub use data::DataTable;
pub use still::Still;

use anyhow::{Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Turns an asset reference into a local file, or declines.
///
/// A resolver that has to fetch does so here, and returns the local path it
/// landed on. Implementors must be cheap on a cache hit: this is called once
/// per distinct reference per render, not per frame, but a film can name many.
pub trait Resolver: Send + Sync {
    fn resolve(&self, reference: &str) -> Option<PathBuf>;
    /// For error messages: where this resolver looked.
    fn describe(&self) -> String;
}

/// Looks for the reference under a directory.
pub struct DirResolver(pub PathBuf);

impl Resolver for DirResolver {
    fn resolve(&self, reference: &str) -> Option<PathBuf> {
        let p = self.0.join(reference);
        p.exists().then_some(p)
    }

    fn describe(&self) -> String {
        self.0.display().to_string()
    }
}

/// Accepts a reference that is already an absolute, existing path.
pub struct AbsoluteResolver;

impl Resolver for AbsoluteResolver {
    fn resolve(&self, reference: &str) -> Option<PathBuf> {
        let p = Path::new(reference);
        (p.is_absolute() && p.exists()).then(|| p.to_path_buf())
    }

    fn describe(&self) -> String {
        "absolute paths".into()
    }
}

/// Resolves references and caches what it has decoded.
///
/// Decoding is shared across the whole render: a still named by ten layers is
/// decoded and pyramided once. The caches are behind a mutex and handed out as
/// `Arc`s, so worker threads rendering different frames share one copy.
pub struct AssetStore {
    resolvers: Vec<Box<dyn Resolver>>,
    /// Every directory handed to [`AssetStore::add_root`], in order — kept
    /// alongside `resolvers` (which only exposes resolution, not enumeration)
    /// so a caller can rebuild an equivalent, independently-caching store
    /// without re-deriving the roots itself. `src/segments.rs`'s concurrent
    /// segment workers are the reason this exists: each gets its own
    /// `AssetStore` (its own `ffmpeg` decode child for any clip, rather than
    /// contending one streaming clip's single decoder across threads — see
    /// that module's doc), built from this list. A store built only from
    /// programmatically inserted assets (`insert_still`/`insert_clip`/
    /// `insert_data`, no roots at all) can't be rebuilt this way; callers
    /// that need concurrency must go through the filesystem.
    roots: Vec<PathBuf>,
    stills: Mutex<HashMap<String, Arc<Still>>>,
    clips: Mutex<HashMap<String, Arc<Clip>>>,
    datas: Mutex<HashMap<String, Arc<DataTable>>>,
}

impl AssetStore {
    pub fn new() -> Self {
        AssetStore {
            resolvers: vec![Box::new(AbsoluteResolver)],
            roots: Vec::new(),
            clips: Mutex::new(HashMap::new()),
            stills: Mutex::new(HashMap::new()),
            datas: Mutex::new(HashMap::new()),
        }
    }

    /// A store that searches `root` as well as absolute paths.
    pub fn rooted(root: impl Into<PathBuf>) -> Self {
        let mut s = Self::new();
        s.add_root(root);
        s
    }

    pub fn add_root(&mut self, root: impl Into<PathBuf>) -> &mut Self {
        let root = root.into();
        self.resolvers.push(Box::new(DirResolver(root.clone())));
        self.roots.push(root);
        self
    }

    /// Every root this store searches, in the order they were added — see
    /// the field doc on why this exists.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn add_resolver(&mut self, r: Box<dyn Resolver>) -> &mut Self {
        self.resolvers.push(r);
        self
    }

    pub fn resolve(&self, reference: &str) -> Result<PathBuf> {
        for r in &self.resolvers {
            if let Some(p) = r.resolve(reference) {
                return Ok(p);
            }
        }
        // A relative path against the process's own directory, last, so that a
        // one-off `showreel still film.json` in a project directory works.
        let p = PathBuf::from(reference);
        if p.exists() {
            return Ok(p);
        }
        let looked: Vec<String> = self.resolvers.iter().map(|r| r.describe()).collect();
        bail!("asset {reference:?} not found. Looked in: {}", looked.join(", "));
    }

    /// Decode a still, pyramid included, once.
    pub fn still(&self, reference: &str) -> Result<Arc<Still>> {
        if let Some(s) = self.stills.lock().unwrap().get(reference) {
            return Ok(s.clone());
        }
        let path = self.resolve(reference)?;
        let s = Arc::new(Still::load(&path)?);
        self.stills.lock().unwrap().insert(reference.to_string(), s.clone());
        Ok(s)
    }

    /// Decode a clip once. The cache key includes the decode parameters,
    /// because the same file drawn at two sizes is two different decodes.
    pub fn clip(
        &self,
        reference: &str,
        fps: f64,
        max_width: u32,
        trim: Option<(f64, f64)>,
    ) -> Result<Arc<Clip>> {
        let key = format!("{reference}|{fps}|{max_width}|{trim:?}");
        if let Some(c) = self.clips.lock().unwrap().get(&key) {
            return Ok(c.clone());
        }
        let path = self.resolve(reference)?;
        let c = Arc::new(Clip::load(&path, fps, max_width, trim)?);
        self.clips.lock().unwrap().insert(key, c.clone());
        Ok(c)
    }

    /// Parse a chart's external data file once, caching it. Resolved through
    /// the same [`Resolver`] chain as a still or clip, so `-A`/`--assets` and
    /// absolute paths locate it identically.
    pub fn data(&self, reference: &str) -> Result<Arc<DataTable>> {
        if let Some(d) = self.datas.lock().unwrap().get(reference) {
            return Ok(d.clone());
        }
        let path = self.resolve(reference)?;
        let d = Arc::new(DataTable::load(&path)?);
        self.datas.lock().unwrap().insert(reference.to_string(), d.clone());
        Ok(d)
    }

    /// Register an already-built still under a name, for programmatic films
    /// that generate their own imagery.
    pub fn insert_still(&self, name: &str, still: Still) {
        self.stills.lock().unwrap().insert(name.to_string(), Arc::new(still));
    }

    /// Register an already-parsed data table under a name — the seam a browser
    /// build (no filesystem) or a programmatic film feeds a chart's data
    /// through, the same way [`AssetStore::insert_still`] feeds imagery.
    pub fn insert_data(&self, name: &str, table: DataTable) {
        self.datas.lock().unwrap().insert(name.to_string(), Arc::new(table));
    }

    /// Register an already-built clip under the exact key [`AssetStore::clip`]
    /// would look up, so a test can exercise clip drawing without ffmpeg.
    pub fn insert_clip(
        &self,
        reference: &str,
        fps: f64,
        max_width: u32,
        trim: Option<(f64, f64)>,
        clip: Clip,
    ) {
        let key = format!("{reference}|{fps}|{max_width}|{trim:?}");
        self.clips.lock().unwrap().insert(key, Arc::new(clip));
    }

    /// Bytes held by decoded assets. Reported by the CLI so a heavy film says
    /// so rather than quietly swapping.
    pub fn memory_bytes(&self) -> usize {
        let s: usize = self.stills.lock().unwrap().values().map(|s| s.memory_bytes()).sum();
        let c: usize = self.clips.lock().unwrap().values().map(|c| c.memory_bytes()).sum();
        let d: usize = self.datas.lock().unwrap().values().map(|d| d.memory_bytes()).sum();
        s + c + d
    }
}

impl Default for AssetStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_asset_names_where_it_looked() {
        let store = AssetStore::rooted("/definitely/not/here");
        let err = store.resolve("nope.png").unwrap_err().to_string();
        assert!(err.contains("nope.png"), "{err}");
        assert!(err.contains("/definitely/not/here"), "{err}");
    }

    #[test]
    fn absolute_paths_resolve_without_a_root() {
        let store = AssetStore::new();
        // Something that certainly exists on any Linux box.
        assert!(store.resolve("/etc/hostname").is_ok());
    }

    #[test]
    fn stills_are_decoded_once() {
        let store = AssetStore::new();
        store.insert_still("gen", Still::solid(4, 4, crate::color::Color::WHITE).unwrap());
        let a = store.still("gen").unwrap();
        let b = store.still("gen").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "second request must reuse the decode");
    }
}
