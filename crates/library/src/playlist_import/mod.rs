pub(crate) mod m3u;
pub(crate) mod rekordbox_xml;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Folder { name: String, source: String, children: Vec<Node> },
    Playlist { name: String, source: String, base_dir: Option<PathBuf>, entries: Vec<String> },
}
