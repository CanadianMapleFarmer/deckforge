use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct M3u {
    pub name: Option<String>,
    pub entries: Vec<String>,
}

pub(crate) fn parse_m3u(bytes: &[u8]) -> M3u {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    };
    let mut m = M3u { name: None, entries: Vec::new() };
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("#PLAYLIST:") {
            m.name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
        } else if !line.is_empty() && !line.starts_with('#') {
            m.entries.push(line.to_string());
        }
    }
    m
}

pub(crate) fn entry_path(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("file://") else { return raw.to_string() };
    let decoded = percent_decode(rest.strip_prefix("localhost").unwrap_or(rest));
    let b = decoded.as_bytes();
    if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { decoded[1..].to_string() } else { decoded }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len())
            .then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn is_playlist_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extended_m3u_with_bom_and_name() {
        let m = parse_m3u("\u{feff}#EXTM3U\n#PLAYLIST:Deep Sunset\n#EXTINF:300,A - B\n01 A.mp3\r\n\n/abs/02 B.flac\n".as_bytes());
        assert_eq!(m.name.as_deref(), Some("Deep Sunset"));
        assert_eq!(m.entries, vec!["01 A.mp3", "/abs/02 B.flac"]);
    }

    #[test]
    fn latin1_falls_back_when_not_utf8() {
        assert_eq!(parse_m3u(b"Caf\xe9.mp3\n").entries, vec!["Café.mp3"]);
    }

    #[test]
    fn entry_paths_decode_file_urls_and_drive_letters() {
        assert_eq!(entry_path("file://localhost/Users/me/My%20Song.mp3"), "/Users/me/My Song.mp3");
        assert_eq!(entry_path("file:///C:/Music/a%23b.mp3"), "C:/Music/a#b.mp3");
        assert_eq!(entry_path("file:///Music/100%.mp3"), "/Music/100%.mp3");
        assert_eq!(entry_path("Music\\a.mp3"), "Music\\a.mp3");
    }

    #[test]
    fn recognises_playlist_extensions() {
        assert!(is_playlist_file(Path::new("/x/Set.M3U8")) && is_playlist_file(Path::new("a.m3u")));
        assert!(!is_playlist_file(Path::new("a.mp3")));
    }
}
