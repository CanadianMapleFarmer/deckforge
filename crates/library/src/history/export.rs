use std::fmt::Write;
use std::path::Path;

use super::SetEntry;

const CUE_FRAMES_PER_SEC: f64 = 75.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Text,
    Csv,
    Cue,
    FcpXml,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Cue => "cue",
            Self::FcpXml => "xml",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportItem {
    pub at_secs: f64,
    pub blend_secs: Option<f64>,
    pub artist: String,
    pub title: String,
    pub length_secs: Option<f64>,
}

/// Took-over entries timed from `origin_secs` until `end_secs`: the entry playing at the origin is clamped to 0, entries fully replaced before it are dropped.
pub fn export_items(entries: &[SetEntry], origin_secs: f64, end_secs: f64) -> Vec<ExportItem> {
    let mut took: Vec<&SetEntry> = entries.iter().filter(|e| e.took_over_secs.is_some()).collect();
    took.sort_by(|a, b| a.took_over_secs.partial_cmp(&b.took_over_secs).unwrap_or(std::cmp::Ordering::Equal));
    let mut items = Vec::new();
    for (i, e) in took.iter().enumerate() {
        let start = e.took_over_secs.unwrap_or_default();
        let next = took.get(i + 1).and_then(|n| n.took_over_secs).unwrap_or(end_secs).min(end_secs);
        if start >= end_secs || next <= origin_secs {
            continue;
        }
        let blend = (e.audible_secs > origin_secs && e.audible_secs < start).then_some(e.audible_secs - origin_secs);
        let (artist, title) = display_names(e);
        items.push(ExportItem {
            at_secs: (start - origin_secs).max(0.0),
            blend_secs: blend,
            artist,
            title,
            length_secs: Some(next - start.max(origin_secs)),
        });
    }
    items
}

fn display_names(e: &SetEntry) -> (String, String) {
    let stem = Path::new(&e.path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let title = if e.title.trim().is_empty() { stem } else { e.title.trim().to_string() };
    (e.artist.trim().to_string(), title)
}

fn label(item: &ExportItem) -> String {
    if item.artist.is_empty() { item.title.clone() } else { format!("{} – {}", item.artist, item.title) }
}

pub fn format_time(secs: f64) -> String {
    let total = secs.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

pub fn render(format: ExportFormat, items: &[ExportItem], title: &str, audio_file: Option<&str>, frame_rate: u32) -> String {
    match format {
        ExportFormat::Text => items.iter().map(|i| format!("{} {}\n", format_time(i.at_secs), label(i))).collect(),
        ExportFormat::Csv => {
            let mut out = String::from("time,artist,title,blend_start,length\n");
            for i in items {
                let _ = writeln!(
                    out,
                    "{},{},{},{},{}",
                    format_time(i.at_secs),
                    csv(&i.artist),
                    csv(&i.title),
                    i.blend_secs.map(format_time).unwrap_or_default(),
                    i.length_secs.map(format_time).unwrap_or_default()
                );
            }
            out
        }
        ExportFormat::Cue => {
            let mut out = format!("TITLE {}\n", cue_quote(title));
            if let Some(file) = audio_file {
                let _ = writeln!(out, "FILE {} WAVE", cue_quote(file));
            }
            for (n, i) in items.iter().enumerate() {
                let frames = (i.at_secs * CUE_FRAMES_PER_SEC).round() as u64;
                let (m, s, f) = (frames / 75 / 60, frames / 75 % 60, frames % 75);
                let _ = write!(out, "  TRACK {:02} AUDIO\n    TITLE {}\n", n + 1, cue_quote(&i.title));
                if !i.artist.is_empty() {
                    let _ = writeln!(out, "    PERFORMER {}", cue_quote(&i.artist));
                }
                let _ = writeln!(out, "    INDEX 01 {m:02}:{s:02}:{f:02}");
            }
            out
        }
        ExportFormat::FcpXml => fcp_xml(items, title, audio_file, frame_rate.max(1)),
    }
}

fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n']) { format!("\"{}\"", field.replace('"', "\"\"")) } else { field.to_string() }
}

fn cue_quote(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "'"))
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Final Cut Pro 7 XML (xmeml v5): sequence markers that DaVinci Resolve and Premiere Pro both import.
fn fcp_xml(items: &[ExportItem], title: &str, audio_file: Option<&str>, rate: u32) -> String {
    let frames = |secs: f64| (secs * f64::from(rate)).round() as u64;
    let duration = items.last().map_or(0, |i| frames(i.at_secs + i.length_secs.unwrap_or(0.0)));
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"5\">\n  <sequence>\n");
    let _ = writeln!(out, "    <name>{}</name>\n    <duration>{duration}</duration>", xml(title));
    let _ = writeln!(out, "    <rate><timebase>{rate}</timebase><ntsc>FALSE</ntsc></rate>");
    if let Some(file) = audio_file {
        let _ = writeln!(out, "    <media><audio><track><clipitem><name>{}</name><start>0</start><end>{duration}</end></clipitem></track></audio></media>", xml(file));
    }
    for i in items {
        let _ = writeln!(out, "    <marker><name>{}</name><comment></comment><in>{}</in><out>-1</out></marker>", xml(&label(i)), frames(i.at_secs));
    }
    out.push_str("  </sequence>\n</xmeml>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::SetEntry;

    fn entry(title: &str, artist: &str, audible: f64, took: Option<f64>) -> SetEntry {
        SetEntry {
            id: 0,
            track_id: Some(1),
            title: title.into(),
            artist: artist.into(),
            path: format!("/music/{title}.flac"),
            key: None,
            bpm: None,
            audible_secs: audible,
            took_over_secs: took,
            teased: took.is_none(),
        }
    }

    fn set() -> Vec<SetEntry> {
        vec![
            entry("Night Shift", "Kora Vale", 0.0, Some(0.0)),
            entry("Copper Skies", "Nuvo & Sable", 240.0, Some(252.0)),
            entry("Tease", "Nobody", 300.0, None),
            entry("Static Bloom", "Ilya Marsh", 3700.0, Some(3723.0)),
        ]
    }

    #[test]
    fn text_tracklist_counts_from_zero() {
        let items = export_items(&set(), 0.0, 4000.0);
        assert_eq!(
            render(ExportFormat::Text, &items, "Beer fest", None, 25),
            "00:00 Kora Vale – Night Shift\n04:12 Nuvo & Sable – Copper Skies\n1:02:03 Ilya Marsh – Static Bloom\n"
        );
    }

    #[test]
    fn the_track_playing_at_the_origin_is_listed_at_zero() {
        let items = export_items(&set(), 100.0, 4000.0);
        assert_eq!(items[0].title, "Night Shift");
        assert_eq!(items[0].at_secs, 0.0);
        assert_eq!(items[1].at_secs, 152.0);
        assert_eq!(items[1].blend_secs, Some(140.0));
        assert_eq!(items[0].length_secs, Some(152.0));
    }

    #[test]
    fn entries_replaced_before_the_origin_are_dropped() {
        let items = export_items(&set(), 300.0, 4000.0);
        assert_eq!(items[0].title, "Copper Skies");
        assert_eq!(items[0].at_secs, 0.0);
        assert_eq!(items[0].blend_secs, None);
        assert_eq!(items.len(), 2);
        assert!(export_items(&set(), 0.0, 3710.0).iter().all(|i| i.title != "Static Bloom"), "took over after the end");
    }

    #[test]
    fn missing_metadata_falls_back_to_the_file_name() {
        let mut e = set();
        e[0].artist.clear();
        e[1].artist.clear();
        e[1].title.clear();
        let text = render(ExportFormat::Text, &export_items(&e, 0.0, 400.0), "", None, 25);
        assert_eq!(text, "00:00 Night Shift\n04:12 Copper Skies\n");
    }

    #[test]
    fn csv_quotes_commas_and_quotes() {
        let e = vec![entry("Hello, \"World\"", "A", 0.0, Some(0.0))];
        let csv = render(ExportFormat::Csv, &export_items(&e, 0.0, 10.0), "", None, 25);
        assert_eq!(csv, "time,artist,title,blend_start,length\n00:00,A,\"Hello, \"\"World\"\"\",,00:10\n");
    }

    #[test]
    fn cue_indexes_use_75_frames_per_second() {
        let cue = render(ExportFormat::Cue, &export_items(&set(), 0.0, 400.0), "Beer fest", Some("2026-10-19 21.00.wav"), 25);
        assert!(cue.starts_with("TITLE \"Beer fest\"\nFILE \"2026-10-19 21.00.wav\" WAVE\n"), "{cue}");
        assert!(cue.contains("  TRACK 02 AUDIO\n    TITLE \"Copper Skies\"\n    PERFORMER \"Nuvo & Sable\"\n    INDEX 01 04:12:00\n"), "{cue}");
        let half = vec![entry("x", "y", 0.0, Some(0.0)), entry("a", "b", 1.5, Some(1.5))];
        assert!(render(ExportFormat::Cue, &export_items(&half, 0.0, 9.0), "", None, 25).contains("INDEX 01 00:01:38"));
    }

    #[test]
    fn fcp_xml_is_well_formed_with_one_marker_per_item() {
        let mut e = set();
        e[0].title = "<Night & Day>".into();
        let xml = render(ExportFormat::FcpXml, &export_items(&e, 0.0, 4000.0), "Beer & fest", Some("set.wav"), 25);
        let mut reader = quick_xml::Reader::from_str(&xml);
        let (mut markers, mut ins, mut names) = (0, Vec::new(), Vec::new());
        let (mut field, mut text) = (String::new(), String::new());
        loop {
            match reader.read_event().expect("well-formed") {
                quick_xml::events::Event::Start(t) => {
                    field = t.name().as_ref().to_string();
                    text.clear();
                    if field == "marker" {
                        markers += 1;
                    }
                }
                quick_xml::events::Event::Text(t) => text.push_str(&t.xml10_content()),
                quick_xml::events::Event::GeneralRef(r) => text.push(match r.as_ref() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    _ => '"',
                }),
                quick_xml::events::Event::End(_) => {
                    if field == "in" {
                        ins.push(text.clone());
                    } else if field == "name" {
                        names.push(text.clone());
                    }
                    field.clear();
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(markers, 3);
        assert!(ins.contains(&"6300".to_string()), "252 s at 25 fps: {ins:?}");
        assert!(names.iter().any(|n| n == "Kora Vale – <Night & Day>"), "{names:?}");
    }

    #[test]
    fn times_format_like_a_tracklist() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(252.4), "04:12");
        assert_eq!(format_time(3723.0), "1:02:03");
    }
}
