use std::path::{Path, PathBuf};

use deckforge_analysis::decode::is_supported;
use walkdir::WalkDir;

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

pub(crate) fn node_name(node: &Node) -> &str {
    match node {
        Node::Folder { name, .. } | Node::Playlist { name, .. } => name,
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

fn collect_audio(dir: &Path, audio: &mut Vec<PathBuf>) {
    let walk = WalkDir::new(dir).follow_links(true).sort_by_file_name().into_iter();
    for entry in walk.filter_entry(|e| !is_hidden(e.path())).filter_map(Result::ok) {
        if entry.file_type().is_file() && is_supported(entry.path()) {
            audio.push(entry.into_path());
        }
    }
}

fn pool_layout(subdirs: &[PathBuf]) -> Option<(&Path, &Path)> {
    let named = |name: &str| subdirs.iter().find(|d| d.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    Some((named("Tracks")?, named("Playlists")?))
}

fn scan_contents(dir: &Path, audio: &mut Vec<PathBuf>) -> Vec<Node> {
    let own = dir.to_string_lossy();
    match scan_node(dir, audio).pop() {
        Some(Node::Folder { source, children, .. }) if source == own => children,
        other => other.into_iter().collect(),
    }
}

fn scan_node(dir: &Path, audio: &mut Vec<PathBuf>) -> Vec<Node> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let (mut subdirs, mut tracks, mut lists) = (Vec::new(), Vec::new(), Vec::new());
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        if is_hidden(&path) {
            continue;
        }
        if path.is_dir() {
            if entry.file_type().is_ok_and(|t| t.is_symlink()) {
                continue;
            }
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
    let pool = pool_layout(&subdirs);
    for sub in &subdirs {
        let nodes = match pool {
            Some((tracks_dir, _)) if sub == tracks_dir => {
                collect_audio(sub, audio);
                continue;
            }
            Some((_, playlists_dir)) if sub == playlists_dir => scan_contents(sub, audio),
            _ => scan_node(sub, audio),
        };
        for node in nodes {
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
    let has_tracks = !tracks.is_empty();
    audio.extend(tracks);

    let sort = |nodes: &mut Vec<Node>| nodes.sort_by_key(|n| node_name(n).to_lowercase());
    sort(&mut folders);
    sort(&mut playlists);
    if has_tracks && folders.is_empty() && subdirs.is_empty() && playlists.len() == 1 {
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
                let p = rel.split('/').filter(|c| !c.is_empty()).fold(root.clone(), |path, part| path.join(part));
                if rel.ends_with('/') {
                    std::fs::create_dir_all(&p).unwrap();
                } else {
                    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                    std::fs::write(&p, body).unwrap();
                }
            }
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        pub(crate) fn p(&self, rel: &str) -> PathBuf {
            rel.split('/').fold(self.0.clone(), |path, part| path.join(part))
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

    fn names(nodes: &[Node]) -> Vec<String> {
        nodes.iter().map(|n| node_name(n).to_string()).collect()
    }

    #[test]
    fn music_ripper_dj_export_uses_only_its_playlists() {
        let t = TempTree::new(&[
            ("Export/Tracks/Mira Chen/Second Wind [2024]/1-01. Second Wind.flac", ""),
            ("Export/Tracks/Elin Voss/Half Light [2023]/1-02. Half Light.aiff", ""),
            ("Export/Playlists/Sets/Warmup.m3u8", "#EXTM3U\n#EXTINF:-1,Mira Chen - Second Wind\n../../Tracks/Mira Chen/Second Wind [2024]/1-01. Second Wind.flac\n"),
            ("Export/Playlists/Peak.m3u8", "#EXTM3U\n../Tracks/Elin Voss/Half Light [2023]/1-02. Half Light.aiff\n"),
        ]);
        let scan = scan_dir(&t.p("Export"));
        let Node::Folder { name, children, .. } = &scan.nodes[0] else { panic!("{:?}", scan.nodes) };
        assert_eq!(name, "Export");
        assert_eq!(names(children), ["Sets", "Peak"]);
        let Node::Folder { children: sets, .. } = &children[0] else { panic!() };
        assert_eq!(names(sets), ["Warmup"]);
        assert_eq!(scan.audio.len(), 2, "the Tracks pool still reaches the collection");
    }

    #[test]
    fn pooled_export_with_only_playlist_files_unwraps_playlists() {
        let t = TempTree::new(&[("E/Tracks/a.flac", ""), ("E/Playlists/B.m3u8", "../Tracks/a.flac\n"), ("E/Playlists/A.m3u8", "../Tracks/a.flac\n")]);
        let Node::Folder { children, .. } = &scan_dir(&t.p("E")).nodes[0] else { panic!() };
        assert_eq!(names(children), ["A", "B"]);
    }

    #[test]
    fn music_ripper_folder_export_keeps_sidebar_folders() {
        let t = TempTree::new(&[
            ("Out/Sets/Warmup/01 - Mira Chen - Second Wind.flac", ""),
            ("Out/Sets/Warmup/Warmup.m3u8", "#EXTM3U\n01 - Mira Chen - Second Wind.flac\n"),
        ]);
        let scan = scan_dir(&t.p("Out/Sets"));
        let Node::Folder { name, children, .. } = &scan.nodes[0] else { panic!("{:?}", scan.nodes) };
        assert_eq!((name.as_str(), names(children)), ("Sets", vec!["Warmup".to_string()]));
    }

    #[test]
    fn empty_and_hidden_directories_produce_nothing() {
        let t = TempTree::new(&[("Root/.hidden/a.mp3", ""), ("Root/empty/", ""), ("Root/._b.mp3", "")]);
        let scan = scan_dir(&t.p("Root"));
        assert!(scan.nodes.is_empty() && scan.audio.is_empty());
    }
}
