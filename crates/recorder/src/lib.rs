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
}
