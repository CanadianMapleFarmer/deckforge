use std::collections::HashMap;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use super::m3u::entry_path;

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

fn components(raw: &str) -> Vec<String> {
    raw.split(['/', '\\']).filter(|c| !c.is_empty()).map(|c| nfc(c).to_lowercase()).collect()
}

pub(crate) fn file_label(raw: &str) -> String {
    raw.rsplit(['/', '\\']).find(|c| !c.is_empty()).unwrap_or(raw).to_string()
}

pub(crate) struct Index {
    by_path: HashMap<String, i64>,
    by_name: HashMap<String, Vec<(i64, Vec<String>)>>,
}

impl Index {
    pub(crate) fn new(tracks: impl IntoIterator<Item = (i64, String)>) -> Self {
        let mut index = Self { by_path: HashMap::new(), by_name: HashMap::new() };
        for (id, path) in tracks {
            index.by_path.insert(nfc(&path), id);
            let mut dirs = components(&path);
            if let Some(name) = dirs.pop() {
                index.by_name.entry(name).or_default().push((id, dirs));
            }
        }
        index
    }

    pub(crate) fn by_path(&self, path: &Path) -> Option<i64> {
        self.by_path.get(&nfc(&path.to_string_lossy())).copied()
    }

    pub(crate) fn by_name(&self, raw: &str) -> Option<i64> {
        let mut dirs = components(raw);
        let name = dirs.pop()?;
        let shared = |other: &[String]| dirs.iter().rev().zip(other.iter().rev()).take_while(|(a, b)| a == b).count();
        let mut best: Option<(usize, i64)> = None;
        let mut tie = false;
        for (id, other) in self.by_name.get(&name)? {
            let score = shared(other);
            match best {
                Some((b, _)) if score < b => {}
                Some((b, _)) if score == b => tie = true,
                _ => {
                    best = Some((score, *id));
                    tie = false;
                }
            }
        }
        if tie { None } else { best.map(|(_, id)| id) }
    }
}

pub(crate) fn on_disk(entry: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
    let path = entry_path(entry);
    let direct = PathBuf::from(&path);
    if direct.is_file() {
        return Some(direct);
    }
    let joined = base_dir.filter(|_| direct.is_relative())?.join(path.replace('\\', "/"));
    joined.is_file().then_some(joined)
}
