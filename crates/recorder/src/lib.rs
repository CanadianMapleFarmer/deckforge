mod flac;
mod wav;
mod writer;

use std::path::PathBuf;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

pub use flac::FlacWriter;
pub use wav::{WavWriter, RF64_THRESHOLD};
pub use writer::{Recorder, RecorderStatus, RecordingResult, LOW_SPACE_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AudioFormat {
    #[default]
    Wav,
    Flac,
}

impl AudioFormat {
    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MarkerFormats {
    pub csv: bool,
    pub cue: bool,
    pub fcp_xml: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordingSettings {
    pub folder: PathBuf,
    pub format: AudioFormat,
    pub bits: u16,
    pub sample_rate: u32,
    pub markers: MarkerFormats,
}

impl Default for RecordingSettings {
    fn default() -> Self {
        let music = dirs::audio_dir().or_else(|| dirs::home_dir().map(|h| h.join("Music"))).unwrap_or_else(|| PathBuf::from("."));
        Self { folder: music.join("deckforge Recordings"), format: AudioFormat::Wav, bits: 24, sample_rate: 48_000, markers: MarkerFormats::default() }
    }
}

impl RecordingSettings {
    /// Clamps values a stored or hand-edited setting may hold to the supported set.
    pub fn sanitized(mut self) -> Self {
        if self.bits != 16 {
            self.bits = 24;
        }
        if self.sample_rate != 44_100 {
            self.sample_rate = 48_000;
        }
        self
    }

    pub fn bytes_per_second(&self) -> u64 {
        self.sample_rate as u64 * 2 * (self.bits as u64 / 8)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "detail")]
pub enum RecordError {
    #[error("the recording folder can't be used: {0}")]
    Folder(String),
    #[error("only about {free_minutes} minutes of disk space left")]
    DiskSpace { free_minutes: u64 },
    #[error("recording failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for RecordError {
    fn from(e: std::io::Error) -> Self {
        RecordError::Io(e.to_string())
    }
}

pub fn min_free_bytes_for_an_hour(settings: &RecordingSettings) -> u64 {
    let raw = settings.bytes_per_second() * 3600;
    match settings.format {
        AudioFormat::Wav => raw,
        AudioFormat::Flac => raw * 7 / 10,
    }
}

pub fn recording_file_name(now_local: NaiveDateTime, format: AudioFormat) -> String {
    format!("{}.{}", now_local.format("%Y-%m-%d %H.%M"), format.extension())
}

/// Seconds of audio a WAV/RF64 or FLAC file declares in its header, as last patched by the recorder.
pub fn recorded_duration(path: &std::path::Path) -> Option<f64> {
    use std::io::Read;
    let mut head = [0u8; 4096];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..n];
    let le32 = |o: usize| head.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    match head.get(..4)? {
        b"fLaC" => {
            let v = u64::from_be_bytes(head.get(18..26)?.try_into().ok()?);
            let rate = v >> 44;
            (rate > 0).then(|| (v & ((1 << 36) - 1)) as f64 / rate as f64)
        }
        b"RIFF" | b"RF64" => {
            let (mut pos, mut rate, mut align, mut ds64) = (12usize, 0u32, 0u32, None);
            while let (Some(id), Some(size)) = (head.get(pos..pos + 4), le32(pos + 4)) {
                match id {
                    b"ds64" => ds64 = head.get(pos + 16..pos + 24).map(|b| u64::from_le_bytes(b.try_into().unwrap())),
                    b"fmt " => {
                        rate = le32(pos + 12)?;
                        align = u16::from_le_bytes(head.get(pos + 20..pos + 22)?.try_into().ok()?) as u32;
                    }
                    b"data" => {
                        let bytes = if size == u32::MAX { ds64? } else { size as u64 };
                        return (rate > 0 && align > 0).then(|| (bytes / align as u64) as f64 / rate as f64);
                    }
                    _ => {}
                }
                pos += 8 + size as usize + (size as usize & 1);
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn file_name_is_the_local_date_and_time() {
        let now = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap().and_hms_opt(21, 5, 42).unwrap();
        assert_eq!(recording_file_name(now, AudioFormat::Wav), "2026-10-19 21.05.wav");
        assert_eq!(recording_file_name(now, AudioFormat::Flac), "2026-10-19 21.05.flac");
    }

    #[test]
    fn defaults_are_24_bit_48k_wav_in_the_music_folder() {
        let s = RecordingSettings::default();
        assert_eq!((s.format, s.bits, s.sample_rate), (AudioFormat::Wav, 24, 48_000));
        assert!(s.folder.ends_with("deckforge Recordings"));
        assert_eq!(s.markers, MarkerFormats::default());
        assert_eq!(min_free_bytes_for_an_hour(&s), 48_000 * 6 * 3600);
    }

    #[test]
    fn recorded_duration_reads_a_wav_header_patched_mid_recording() {
        let dir = crate::wav::tests::tempdir();
        let path = dir.join("cut.wav");
        let mut w = WavWriter::create(&path, 44_100, 24).unwrap();
        w.write(&crate::wav::tests::tone(44_100, 24)).unwrap();
        w.checkpoint().unwrap();
        w.write(&crate::wav::tests::tone(1_000, 24)).unwrap();
        drop(w);
        assert_eq!(recorded_duration(&path), Some(1.0));
        assert_eq!(recorded_duration(&dir.join("missing.wav")), None);
    }
}
