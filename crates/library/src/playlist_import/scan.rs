use std::path::{Path, PathBuf};

use deckforge_analysis::decode::is_supported;

use super::m3u::{is_playlist_file, parse_m3u};
use super::Node;

pub(crate) struct Scan {
    pub nodes: Vec<Node>,
    pub audio: Vec<PathBuf>,
}

pub(crate) fn scan_dir(dir: &Path) -> Scan {
    let mut audio = Vec::new();
    let nodes = scan_node(dir, &mut audio);
    Scan { nodes, audio }
}

pub(crate) fn m3u_node(path: &Path) -> std::io::Result<Node> {
    let m3u = parse_m3u(&std::fs::read(path)?);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Node::Playlist {
        name: m3u.name.unwrap_or(stem),
        source: path.to_string_lossy().into_owned(),
        base_dir: path.parent().map(Path::to_path_buf),
        entries: m3u.entries,
    })
}

fn node_name(node: &Node) -> &str {
    match node {
        Node::Folder { name, .. } | Node::Playlist { name, .. } => name,
    }
}

fn scan_node(dir: &Path, audio: &mut Vec<PathBuf>) -> Vec<Node> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let (mut subdirs, mut tracks, mut lists) = (Vec::new(), Vec::new(), Vec::new());
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            subdirs.push(path);
        } else if is_playlist_file(&path) {
            lists.push(path);
        } else if is_supported(&path) {
            tracks.push(path);
        }
    }
    let by_name = |a: &PathBuf, b: &PathBuf| a.file_name().cmp(&b.file_name());
    subdirs.sort_by(by_name);
    tracks.sort_by(by_name);
    lists.sort_by(by_name);

    let mut folders = Vec::new();
    let mut playlists = Vec::new();
    for sub in &subdirs {
        for node in scan_node(sub, audio) {
            match node {
                Node::Folder { .. } => folders.push(node),
                Node::Playlist { .. } => playlists.push(node),
            }
        }
    }
    for list in &lists {
        match m3u_node(list) {
            Ok(node) => playlists.push(node),
            Err(e) => log::warn!("skipping unreadable playlist {}: {e}", list.display()),
        }
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let source = dir.to_string_lossy().into_owned();
    if lists.is_empty() && !tracks.is_empty() {
        playlists.push(Node::Playlist {
            name: name.clone(),
            source: format!("{source}#tracks"),
            base_dir: None,
            entries: tracks.iter().map(|t| t.to_string_lossy().into_owned()).collect(),
        });
    }
    audio.extend(tracks);

    let sort = |nodes: &mut Vec<Node>| nodes.sort_by_key(|n| node_name(n).to_lowercase());
    sort(&mut folders);
    sort(&mut playlists);
    if folders.is_empty() && subdirs.is_empty() && playlists.len() == 1 {
        return playlists;
    }
    folders.extend(playlists);
    if folders.is_empty() {
        return Vec::new();
    }
    vec![Node::Folder { name, source, children: folders }]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) struct TempTree(pub PathBuf);

    impl TempTree {
        pub(crate) fn new(files: &[(&str, &str)]) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("df-scan-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            for (rel, body) in files {
                let p = root.join(rel);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                if !rel.ends_with('/') {
                    std::fs::write(&p, body).unwrap();
                }
            }
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        pub(crate) fn p(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
        pub(crate) fn s(&self, rel: &str) -> String {
            self.p(rel).to_string_lossy().into_owned()
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn playlist(name: &str, source: String, base_dir: Option<PathBuf>, entries: Vec<String>) -> Node {
        Node::Playlist { name: name.into(), source, base_dir, entries }
    }

    #[test]
    fn single_track_folder_collapses_to_one_playlist() {
        let t = TempTree::new(&[("Deep/b.mp3", ""), ("Deep/a.mp3", ""), ("Deep/cover.jpg", "")]);
        let scan = scan_dir(&t.p("Deep"));
        let src = format!("{}#tracks", t.s("Deep"));
        assert_eq!(scan.nodes, vec![playlist("Deep", src, None, vec![t.s("Deep/a.mp3"), t.s("Deep/b.mp3")])]);
        assert_eq!(scan.audio, vec![t.p("Deep/a.mp3"), t.p("Deep/b.mp3")]);
    }

    #[test]
    fn m3u8_alongside_tracks_names_and_orders_the_playlist() {
        let t = TempTree::new(&[("Deep/a.mp3", ""), ("Deep/b.mp3", ""), ("Deep/Deep Sunset.m3u8", "#EXTM3U\nb.mp3\na.mp3\n")]);
        let scan = scan_dir(&t.p("Deep"));
        let expected = playlist("Deep Sunset", t.s("Deep/Deep Sunset.m3u8"), Some(t.p("Deep")), vec!["b.mp3".into(), "a.mp3".into()]);
        assert_eq!(scan.nodes, vec![expected]);
        assert_eq!(scan.audio.len(), 2);
    }

    #[test]
    fn subdirectories_become_a_folder() {
        let t = TempTree::new(&[
            ("Afro/x.mp3", ""),
            ("Afro/Peak/b.mp3", ""),
            ("Afro/Deep/a.mp3", ""),
            ("Afro/warmup.m3u8", "Deep/a.mp3\n"),
        ]);
        let scan = scan_dir(&t.p("Afro"));
        let Node::Folder { name, source, children } = &scan.nodes[0] else { panic!("{:?}", scan.nodes) };
        assert_eq!((name.as_str(), source.clone()), ("Afro", t.s("Afro")));
        let names: Vec<_> = children
            .iter()
            .map(|c| match c {
                Node::Playlist { name, .. } | Node::Folder { name, .. } => name.as_str(),
            })
            .collect();
        assert_eq!(names, ["Deep", "Peak", "warmup"]);
        assert_eq!(scan.audio.len(), 3, "loose x.mp3 is still collected");
    }

    #[test]
    fn loose_tracks_beside_subdirectories_get_their_own_playlist() {
        let t = TempTree::new(&[("Mix/y.mp3", ""), ("Mix/Sub/z.mp3", ""), ("Mix/Nested/Inner/q.mp3", "")]);
        let scan = scan_dir(&t.p("Mix"));
        let Node::Folder { children, .. } = &scan.nodes[0] else { panic!() };
        assert!(matches!(&children[0], Node::Folder { name, .. } if name == "Nested"), "folders sort first");
        assert_eq!(children[1], playlist("Mix", format!("{}#tracks", t.s("Mix")), None, vec![t.s("Mix/y.mp3")]));
        assert!(matches!(&children[2], Node::Playlist { name, .. } if name == "Sub"));
    }

    #[test]
    fn empty_and_hidden_directories_produce_nothing() {
        let t = TempTree::new(&[("Root/.hidden/a.mp3", ""), ("Root/empty/", ""), ("Root/._b.mp3", "")]);
        let scan = scan_dir(&t.p("Root"));
        assert!(scan.nodes.is_empty() && scan.audio.is_empty());
    }
}
