use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use super::m3u::entry_path;
use super::Node;

pub(crate) const MISSING_PREFIX: &str = "#missing:";

struct Frame {
    name: String,
    folder: bool,
    by_location: bool,
    children: Vec<Node>,
    entries: Vec<String>,
}

#[derive(Default)]
struct Parser {
    seen_root: bool,
    in_collection: bool,
    in_playlists: bool,
    locations: HashMap<String, String>,
    stack: Vec<Frame>,
    roots: Vec<Node>,
}

fn attr(e: &BytesStart<'_>, key: &str) -> Result<Option<String>, String> {
    let Some(a) = e.try_get_attribute(key).map_err(|e| e.to_string())? else { return Ok(None) };
    a.normalized_value(XmlVersion::Implicit1_0).map(|v| Some(v.into_owned())).map_err(|e| e.to_string())
}

impl Parser {
    fn open(&mut self, e: &BytesStart<'_>) -> Result<(), String> {
        match e.name().as_ref() {
            "DJ_PLAYLISTS" => self.seen_root = true,
            "COLLECTION" => self.in_collection = true,
            "PLAYLISTS" => self.in_playlists = true,
            "TRACK" if self.in_collection => {
                if let (Some(id), Some(location)) = (attr(e, "TrackID")?, attr(e, "Location")?) {
                    self.locations.insert(id, entry_path(&location));
                }
            }
            "TRACK" if self.in_playlists => {
                let Some(key) = attr(e, "Key")? else { return Ok(()) };
                let Some(top) = self.stack.last_mut() else { return Ok(()) };
                let entry = if top.by_location {
                    entry_path(&key)
                } else {
                    self.locations.get(&key).cloned().unwrap_or_else(|| format!("{MISSING_PREFIX}{key}"))
                };
                top.entries.push(entry);
            }
            "NODE" if self.in_playlists => self.stack.push(Frame {
                name: attr(e, "Name")?.unwrap_or_default(),
                folder: attr(e, "Type")?.as_deref() == Some("0"),
                by_location: attr(e, "KeyType")?.as_deref() == Some("1"),
                children: Vec::new(),
                entries: Vec::new(),
            }),
            _ => {}
        }
        Ok(())
    }

    fn close(&mut self, name: &str, prefix: &str) {
        match name {
            "COLLECTION" => self.in_collection = false,
            "PLAYLISTS" => self.in_playlists = false,
            "NODE" if self.in_playlists => {
                let Some(frame) = self.stack.pop() else { return };
                if self.stack.is_empty() {
                    self.roots = frame.children;
                    return;
                }
                let path: Vec<&str> =
                    self.stack.iter().skip(1).map(|f| f.name.as_str()).chain(std::iter::once(frame.name.as_str())).collect();
                let source = format!("{prefix}#{}", path.join("/"));
                let node = if frame.folder {
                    Node::Folder { name: frame.name, source, children: frame.children }
                } else {
                    Node::Playlist { name: frame.name, source, base_dir: None, entries: frame.entries }
                };
                if let Some(parent) = self.stack.last_mut() {
                    parent.children.push(node);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn parse_rekordbox_xml(text: &str, source_prefix: &str) -> Result<Vec<Node>, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut p = Parser::default();
    loop {
        match reader.read_event().map_err(|e| format!("invalid XML: {e}"))? {
            Event::Start(e) => p.open(&e)?,
            Event::Empty(e) => {
                p.open(&e)?;
                p.close(e.name().as_ref(), source_prefix);
            }
            Event::End(e) => p.close(e.name().as_ref(), source_prefix),
            Event::Eof => break,
            _ => {}
        }
    }
    if !p.seen_root {
        return Err("not a rekordbox collection export".into());
    }
    Ok(p.roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0"><PRODUCT Name="rekordbox"/><COLLECTION Entries="2">
<TRACK TrackID="1" Name="A &amp; B" Location="file://localhost/Music/A%20B.mp3"><TEMPO Inizio="0.1" Bpm="128"/></TRACK>
<TRACK TrackID="2" Name="C" Location="file://localhost/Music/C.flac"/>
</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="3">
<NODE Type="0" Name="House &amp; Garage" Count="1"><NODE Name="Deep" Type="1" KeyType="0" Entries="3">
<TRACK Key="2"/><TRACK Key="1"/><TRACK Key="99"/></NODE></NODE>
<NODE Name="ByPath" Type="1" KeyType="1" Entries="1"><TRACK Key="file://localhost/Music/C.flac"/></NODE>
<NODE Name="Empty" Type="1" KeyType="0" Entries="0"/>
</NODE></PLAYLISTS></DJ_PLAYLISTS>"#;

    fn playlist(n: &Node) -> (&str, &str, &[String]) {
        let Node::Playlist { name, source, entries, .. } = n else { panic!("not a playlist: {n:?}") };
        (name, source, entries)
    }

    #[test]
    fn builds_the_folder_tree_in_order() {
        let nodes = parse_rekordbox_xml(XML, "/x.xml").unwrap();
        assert_eq!(nodes.len(), 3);
        let Node::Folder { name, source, children } = &nodes[0] else { panic!() };
        assert_eq!((name.as_str(), source.as_str()), ("House & Garage", "/x.xml#House & Garage"));
        let (name, source, entries) = playlist(&children[0]);
        assert_eq!((name, source), ("Deep", "/x.xml#House & Garage/Deep"));
        assert_eq!(entries[..2], ["/Music/C.flac", "/Music/A B.mp3"]);
        assert_eq!(playlist(&nodes[2]).2.len(), 0);
    }

    #[test]
    fn unknown_track_id_is_kept_as_missing_entry() {
        let nodes = parse_rekordbox_xml(XML, "/x.xml").unwrap();
        let Node::Folder { children, .. } = &nodes[0] else { panic!() };
        assert_eq!(playlist(&children[0]).2[2], "#missing:99");
    }

    #[test]
    fn key_type_one_uses_locations() {
        let nodes = parse_rekordbox_xml(XML, "/x.xml").unwrap();
        assert_eq!(playlist(&nodes[1]).2, ["/Music/C.flac"]);
    }

    #[test]
    fn rejects_non_rekordbox_xml() {
        assert!(parse_rekordbox_xml("<plist><dict/></plist>", "/x.xml").is_err());
        assert!(parse_rekordbox_xml("<DJ_PLAYLISTS><broken", "/x.xml").is_err());
    }
}
