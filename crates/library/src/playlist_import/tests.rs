use super::scan::tests::TempTree;
use crate::{Library, Playlist};

fn by_name<'a>(lists: &'a [Playlist], name: &str) -> &'a Playlist {
    lists.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no playlist {name} in {lists:?}"))
}

fn track_names(lib: &Library, playlist: i64) -> Vec<String> {
    lib.tracks("", Some(playlist)).unwrap().into_iter().map(|t| t.title).collect()
}

#[test]
fn folder_import_creates_tree_and_collection() {
    let t = TempTree::new(&[("Afro/Deep/a.mp3", ""), ("Afro/Peak/b.mp3", "")]);
    let lib = Library::open_in_memory().unwrap();
    let report = lib.import_playlists(&[t.p("Afro")]).unwrap();
    assert_eq!(report.added, 2);
    let lists = lib.playlists().unwrap();
    let afro = by_name(&lists, "Afro");
    assert!(afro.is_folder && afro.parent_id.is_none());
    assert_eq!(by_name(&lists, "Deep").parent_id, Some(afro.id));
    assert_eq!(track_names(&lib, by_name(&lists, "Deep").id), ["a"]);
    assert_eq!(report.playlists.len(), 2);
    assert!(report.playlists.iter().all(|s| s.found == 1 && s.total == 1 && s.missing.is_empty()));
}

#[test]
fn reimport_updates_in_place() {
    let t = TempTree::new(&[
        ("Afro/Deep/a.mp3", ""),
        ("Afro/Deep/b.mp3", ""),
        ("Afro/Deep/list.m3u8", "a.mp3\nb.mp3\n"),
        ("Afro/Peak/c.mp3", ""),
    ]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_playlists(&[t.p("Afro")]).unwrap();
    let before = lib.playlists().unwrap();
    let list = by_name(&before, "list").id;
    lib.rename_playlist(by_name(&before, "Peak").id, "Mine").unwrap();
    let hand = lib.create_playlist("Hand", None, false).unwrap();
    let a = lib.tracks("a", None).unwrap()[0].id;
    lib.add_to_playlist(hand.id, &[a]).unwrap();

    std::fs::write(t.p("Afro/Deep/d.mp3"), "").unwrap();
    std::fs::write(t.p("Afro/Deep/list.m3u8"), "b.mp3\na.mp3\nd.mp3\n").unwrap();
    let report = lib.import_playlists(&[t.p("Afro")]).unwrap();

    assert_eq!(report.added, 1);
    let after = lib.playlists().unwrap();
    assert_eq!(after.len(), 4);
    assert_eq!(by_name(&after, "list").id, list);
    assert_eq!(track_names(&lib, list), ["b", "a", "d"]);
    assert_eq!(by_name(&after, "Mine").parent_id, Some(by_name(&after, "Afro").id));
    assert_eq!(track_names(&lib, hand.id), ["a"]);
}

#[test]
fn reimport_recreates_a_deleted_playlist() {
    let t = TempTree::new(&[("Set/Deep/a.mp3", ""), ("Set/Peak/b.mp3", "")]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_playlists(&[t.p("Set")]).unwrap();
    lib.delete_playlist(by_name(&lib.playlists().unwrap(), "Deep").id).unwrap();
    lib.import_playlists(&[t.p("Set")]).unwrap();
    assert_eq!(track_names(&lib, by_name(&lib.playlists().unwrap(), "Deep").id), ["a"]);
}

#[test]
fn deleting_the_folder_then_reimporting_rebuilds_the_tree() {
    let t = TempTree::new(&[("Set/Deep/a.mp3", ""), ("Set/Peak/b.mp3", "")]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_playlists(&[t.p("Set")]).unwrap();
    lib.delete_playlist(by_name(&lib.playlists().unwrap(), "Set").id).unwrap();
    lib.import_playlists(&[t.p("Set")]).unwrap();
    assert_eq!(lib.playlists().unwrap().len(), 3);
}

#[test]
fn duplicate_entries_keep_first_position() {
    let t = TempTree::new(&[("S/a.mp3", ""), ("S/b.mp3", ""), ("S/s.m3u8", "a.mp3\nb.mp3\na.mp3\n")]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_playlists(&[t.p("S/s.m3u8")]).unwrap();
    assert_eq!(track_names(&lib, by_name(&lib.playlists().unwrap(), "s").id), ["a", "b"]);
}

#[test]
fn missing_entries_fall_back_to_file_name_then_report() {
    let t = TempTree::new(&[("lib/x/a.mp3", ""), ("exports/old.m3u", "C:\\Old\\x\\a.mp3\nC:\\Old\\gone.mp3\n")]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_paths(&[t.p("lib")]);
    let report = lib.import_playlists(&[t.p("exports/old.m3u")]).unwrap();
    assert_eq!(report.added, 0);
    let s = &report.playlists[0];
    assert_eq!((s.found, s.total, s.missing.clone()), (1, 2, vec!["gone.mp3".to_string()]));
    assert_eq!(track_names(&lib, s.id), ["a"]);
}

#[test]
fn ambiguous_file_name_counts_as_missing() {
    let t = TempTree::new(&[("p/a.mp3", ""), ("q/a.mp3", ""), ("e/list.m3u8", "a.mp3\n")]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_paths(&[t.p("p"), t.p("q")]);
    let report = lib.import_playlists(&[t.p("e/list.m3u8")]).unwrap();
    assert_eq!(report.playlists[0].missing, ["a.mp3"]);
}

#[test]
fn nfc_path_matches_existing_track() {
    let t = TempTree::new(&[("m/Cafe\u{301}.mp3", "")]);
    let lib = Library::open_in_memory().unwrap();
    lib.import_paths(&[t.p("m")]);
    let nfc = format!("{}/Caf\u{e9}.mp3\n", t.s("m"));
    std::fs::write(t.p("list.m3u8"), nfc).unwrap();
    let report = lib.import_playlists(&[t.p("list.m3u8")]).unwrap();
    assert_eq!(lib.tracks("", None).unwrap().len(), 1);
    assert_eq!(report.playlists[0].found, 1);
}

#[test]
fn xml_import_reports_unknown_ids() {
    let t = TempTree::new(&[("music/a.mp3", ""), ("music/c.mp3", "")]);
    let dir = t.s("music");
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">
<TRACK TrackID="1" Location="file://localhost{dir}/a.mp3"/><TRACK TrackID="2" Location="file://localhost{dir}/c.mp3"/>
</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Type="0" Name="House" Count="1">
<NODE Name="Deep" Type="1" KeyType="0" Entries="3"><TRACK Key="2"/><TRACK Key="1"/><TRACK Key="99"/></NODE>
</NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#
    );
    std::fs::write(t.p("rb.xml"), xml).unwrap();
    let lib = Library::open_in_memory().unwrap();
    let report = lib.import_playlists(&[t.p("rb.xml")]).unwrap();
    assert_eq!(report.added, 2);
    let s = &report.playlists[0];
    assert_eq!((s.name.as_str(), s.found, s.total), ("Deep", 2, 3));
    assert_eq!(track_names(&lib, s.id), ["c", "a"]);
    let lists = lib.playlists().unwrap();
    assert_eq!(by_name(&lists, "Deep").parent_id, Some(by_name(&lists, "House").id));
}

#[test]
fn malformed_xml_is_an_error() {
    let t = TempTree::new(&[("bad.xml", "<plist/>")]);
    let lib = Library::open_in_memory().unwrap();
    assert!(lib.import_playlists(&[t.p("bad.xml")]).is_err());
    assert!(lib.playlists().unwrap().is_empty());
}
