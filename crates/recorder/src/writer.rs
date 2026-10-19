use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use deckforge_engine::{RecordFrame, RecordShared};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use serde::Serialize;

use crate::{min_free_bytes_for_an_hour, recording_file_name, AudioFormat, FlacWriter, RecordError, RecordingSettings, WavWriter};

pub const LOW_SPACE_BYTES: u64 = 200 * 1024 * 1024;
const CHECKPOINT: Duration = Duration::from_secs(2);
const RESAMPLE_CHUNK: usize = 1024;
const BATCH: usize = 8192;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecorderStatus {
    pub path: String,
    pub elapsed_secs: f64,
    pub bytes: u64,
    pub dropped_secs: f64,
    pub warning: Option<String>,
    /// The writer stopped on its own (disk full or write error); the recording must be stopped.
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordingResult {
    pub path: PathBuf,
    pub start_clock: f64,
    pub duration_secs: f64,
    /// Why the writer stopped early; the file is still playable up to its last checkpoint.
    pub error: Option<RecordError>,
}

pub(crate) struct Finished {
    pub duration_secs: f64,
    pub error: Option<RecordError>,
}

enum Sink {
    Wav(WavWriter),
    Flac(Box<FlacWriter>),
    #[cfg(test)]
    Failing,
}

#[cfg(test)]
fn disk_full() -> std::io::Error {
    std::io::Error::other("disk full")
}

impl Sink {
    fn write(&mut self, frames: &[[f32; 2]]) -> std::io::Result<()> {
        match self {
            Sink::Wav(w) => w.write(frames),
            Sink::Flac(w) => w.write(frames),
            #[cfg(test)]
            Sink::Failing => Err(disk_full()),
        }
    }

    fn checkpoint(&mut self) -> std::io::Result<()> {
        match self {
            Sink::Wav(w) => w.checkpoint(),
            Sink::Flac(w) => w.checkpoint(),
            #[cfg(test)]
            Sink::Failing => Err(disk_full()),
        }
    }

    fn bytes(&self) -> u64 {
        match self {
            Sink::Wav(w) => w.bytes(),
            Sink::Flac(w) => w.bytes(),
            #[cfg(test)]
            Sink::Failing => 0,
        }
    }

    fn finish(self) -> std::io::Result<()> {
        match self {
            Sink::Wav(w) => w.finish(),
            Sink::Flac(w) => w.finish(),
            #[cfg(test)]
            Sink::Failing => Err(disk_full()),
        }
    }
}

struct Converter {
    resampler: SincFixedIn<f32>,
    input: [Vec<f32>; 2],
    fed: u64,
    emitted: u64,
    skip: usize,
    ratio: f64,
}

impl Converter {
    fn new(from: u32, to: u32) -> Option<Self> {
        let ratio = to as f64 / from as f64;
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Cubic,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler = SincFixedIn::new(ratio, 1.0, params, RESAMPLE_CHUNK, 2).ok()?;
        let skip = resampler.output_delay();
        Some(Self { resampler, input: [Vec::new(), Vec::new()], fed: 0, emitted: 0, skip, ratio })
    }

    fn push(&mut self, frames: &[[f32; 2]], out: &mut Vec<[f32; 2]>) {
        for f in frames {
            self.input[0].push(f[0]);
            self.input[1].push(f[1]);
        }
        self.fed += frames.len() as u64;
        while self.input[0].len() >= self.resampler.input_frames_next() {
            let n = self.resampler.input_frames_next();
            let Ok(res) = self.resampler.process(&[&self.input[0][..n], &self.input[1][..n]], None) else { break };
            self.input[0].drain(..n);
            self.input[1].drain(..n);
            self.emit(&res, out, u64::MAX);
        }
    }

    fn flush(mut self, out: &mut Vec<[f32; 2]>) {
        let target = (self.fed as f64 * self.ratio).round() as u64;
        let mut guard = 0;
        while self.emitted < target && guard < 64 {
            guard += 1;
            let tail = [self.input[0].as_slice(), self.input[1].as_slice()];
            let res = if tail[0].is_empty() { self.resampler.process_partial::<&[f32]>(None, None) } else { self.resampler.process_partial(Some(&tail), None) };
            self.input[0].clear();
            self.input[1].clear();
            let Ok(res) = res else { break };
            self.emit(&res, out, target);
        }
    }

    fn emit(&mut self, res: &[Vec<f32>], out: &mut Vec<[f32; 2]>, limit: u64) {
        for (&l, &r) in res[0].iter().zip(&res[1]) {
            if self.skip > 0 {
                self.skip -= 1;
                continue;
            }
            if self.emitted >= limit {
                return;
            }
            out.push([l, r]);
            self.emitted += 1;
        }
    }
}

struct Worker {
    rx: rtrb::Consumer<RecordFrame>,
    shared: Arc<RecordShared>,
    sink: Sink,
    folder: PathBuf,
    target_rate: u32,
    source_rate: u32,
    converter: Option<Converter>,
    written: u64,
    staged: Vec<[f32; 2]>,
    converted: Vec<[f32; 2]>,
    status: Arc<Mutex<RecorderStatus>>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    fn run(mut self) -> Finished {
        let mut warning = None;
        let mut error = self.drive(&mut warning).and_then(|()| self.flush_converter()).err();
        let duration_secs = self.written as f64 / self.target_rate as f64;
        let stopped_itself = warning.is_some() || error.is_some();
        let bytes = self.sink.bytes();
        let status = self.status.clone();
        let rate = self.status_rate();
        let dropped = self.shared.dropped();
        if let Err(e) = self.sink.finish() {
            error.get_or_insert(e.into());
        }
        if let Some(e) = &error {
            warning = Some(format!("Recording stopped: {e}. The file is kept up to the last few seconds."));
        }
        let mut s = status.lock().unwrap_or_else(|p| p.into_inner());
        s.elapsed_secs = duration_secs;
        s.bytes = bytes;
        s.dropped_secs = dropped as f64 / rate as f64;
        if warning.is_some() {
            s.warning = warning;
        }
        s.finished |= stopped_itself;
        Finished { duration_secs, error }
    }

    fn drive(&mut self, warning: &mut Option<String>) -> Result<(), RecordError> {
        let mut last = Instant::now();
        loop {
            let stopping = self.stop.load(Ordering::Acquire);
            let got = self.pump()?;
            if last.elapsed() >= CHECKPOINT {
                last = Instant::now();
                self.sink.checkpoint()?;
                if fs4::available_space(&self.folder).is_ok_and(|free| free < LOW_SPACE_BYTES) {
                    *warning = Some("The disk is almost full, so the recording was stopped.".to_owned());
                    return Ok(());
                }
                self.publish(None, false);
            }
            if stopping && got == 0 {
                return Ok(());
            }
            if got == 0 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn flush_converter(&mut self) -> Result<(), RecordError> {
        if let Some(c) = self.converter.take() {
            self.converted.clear();
            c.flush(&mut self.converted);
            self.sink.write(&self.converted)?;
            self.written += self.converted.len() as u64;
        }
        Ok(())
    }

    fn status_rate(&self) -> u32 {
        if self.source_rate > 0 { self.source_rate } else { self.target_rate }
    }

    fn pump(&mut self) -> Result<usize, RecordError> {
        let mut got = 0;
        self.staged.clear();
        while got < BATCH {
            let Ok(frame) = self.rx.pop() else { break };
            got += 1;
            match frame {
                RecordFrame::Audio(f) => self.staged.push(f),
                RecordFrame::Gap(n) => self.staged.extend(std::iter::repeat_n([0.0; 2], n as usize)),
                RecordFrame::Rate(r) => {
                    self.deliver()?;
                    self.set_source_rate(r)?;
                }
            }
        }
        self.deliver()?;
        Ok(got)
    }

    fn set_source_rate(&mut self, rate: u32) -> Result<(), RecordError> {
        if rate == self.source_rate {
            return Ok(());
        }
        if let Some(c) = self.converter.take() {
            self.converted.clear();
            c.flush(&mut self.converted);
            self.sink.write(&self.converted)?;
            self.written += self.converted.len() as u64;
        }
        self.source_rate = rate;
        if rate != self.target_rate && rate > 0 {
            self.converter = Converter::new(rate, self.target_rate);
        }
        Ok(())
    }

    fn deliver(&mut self) -> Result<(), RecordError> {
        if self.staged.is_empty() {
            return Ok(());
        }
        if let Some(c) = &mut self.converter {
            self.converted.clear();
            c.push(&self.staged, &mut self.converted);
            self.sink.write(&self.converted)?;
            self.written += self.converted.len() as u64;
        } else {
            self.sink.write(&self.staged)?;
            self.written += self.staged.len() as u64;
        }
        self.staged.clear();
        Ok(())
    }

    fn publish(&self, warning: Option<String>, finished: bool) {
        let rate = self.status_rate();
        let mut s = self.status.lock().unwrap_or_else(|p| p.into_inner());
        s.elapsed_secs = self.written as f64 / self.target_rate as f64;
        s.bytes = self.sink.bytes();
        s.dropped_secs = self.shared.dropped() as f64 / rate as f64;
        if warning.is_some() {
            s.warning = warning;
        }
        s.finished |= finished;
    }
}

#[cfg(test)]
impl Worker {
    fn for_test(rx: rtrb::Consumer<RecordFrame>, sink: Sink, status: Arc<Mutex<RecorderStatus>>) -> Self {
        Worker {
            rx,
            shared: Arc::new(RecordShared::default()),
            sink,
            folder: std::env::temp_dir(),
            target_rate: 48_000,
            source_rate: 0,
            converter: None,
            written: 0,
            staged: Vec::new(),
            converted: Vec::new(),
            status,
            stop: Arc::new(AtomicBool::new(true)),
        }
    }
}

pub struct Recorder {
    path: PathBuf,
    shared: Arc<RecordShared>,
    status: Arc<Mutex<RecorderStatus>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Finished>>,
}

fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..).map(|n| folder.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

impl Recorder {
    /// Refuses only below the low-space floor; under an hour of space it records with a warning.
    pub fn start(settings: &RecordingSettings, rx: rtrb::Consumer<RecordFrame>, shared: Arc<RecordShared>, now_local: NaiveDateTime) -> Result<Recorder, RecordError> {
        std::fs::create_dir_all(&settings.folder).map_err(|e| RecordError::Folder(format!("{}: {e}", settings.folder.display())))?;
        let free = fs4::available_space(&settings.folder).map_err(|e| RecordError::Folder(e.to_string()))?;
        Self::start_with_free_space(settings, rx, shared, now_local, free)
    }

    pub(crate) fn start_with_free_space(
        settings: &RecordingSettings,
        rx: rtrb::Consumer<RecordFrame>,
        shared: Arc<RecordShared>,
        now_local: NaiveDateTime,
        free: u64,
    ) -> Result<Recorder, RecordError> {
        let settings = settings.clone().sanitized();
        std::fs::create_dir_all(&settings.folder).map_err(|e| RecordError::Folder(format!("{}: {e}", settings.folder.display())))?;
        let per_minute = (min_free_bytes_for_an_hour(&settings) / 60).max(1);
        if free < LOW_SPACE_BYTES {
            return Err(RecordError::DiskSpace { free_minutes: free / per_minute });
        }
        let warning = (free < min_free_bytes_for_an_hour(&settings))
            .then(|| format!("Only about {} minutes of disk space left for this recording.", free.saturating_sub(LOW_SPACE_BYTES) / per_minute));
        let path = unique_path(&settings.folder, &recording_file_name(now_local, settings.format));
        let folder_err = |e: std::io::Error| RecordError::Folder(format!("{}: {e}", path.display()));
        let sink = match settings.format {
            AudioFormat::Wav => Sink::Wav(WavWriter::create(&path, settings.sample_rate, settings.bits).map_err(folder_err)?),
            AudioFormat::Flac => Sink::Flac(Box::new(FlacWriter::create(&path, settings.sample_rate, settings.bits).map_err(folder_err)?)),
        };
        let status = Arc::new(Mutex::new(RecorderStatus { path: path.display().to_string(), warning, ..RecorderStatus::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            rx,
            shared: shared.clone(),
            sink,
            folder: settings.folder.clone(),
            target_rate: settings.sample_rate,
            source_rate: 0,
            converter: None,
            written: 0,
            staged: Vec::with_capacity(BATCH),
            converted: Vec::with_capacity(BATCH * 2),
            status: status.clone(),
            stop: stop.clone(),
        };
        let worker = std::thread::Builder::new().name("deckforge-recorder".into()).spawn(move || worker.run())?;
        Ok(Recorder { path, shared, status, stop, worker: Some(worker) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn status(&self) -> RecorderStatus {
        self.status.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Drains what is already queued, finalizes the file and returns where it went.
    pub fn stop(mut self) -> Result<RecordingResult, RecordError> {
        self.stop.store(true, Ordering::Release);
        let Some(Ok(finished)) = self.worker.take().map(JoinHandle::join) else {
            return Err(RecordError::Io("the recording thread stopped unexpectedly".into()));
        };
        Ok(RecordingResult {
            path: self.path.clone(),
            start_clock: self.shared.start_clock().unwrap_or(0.0),
            duration_secs: finished.duration_secs,
            error: finished.error,
        })
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::wav::tests::{tempdir, tone};
    use chrono::NaiveDate;
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    pub fn decode_i32(path: &Path) -> Vec<i32> {
        let mss = MediaSourceStream::new(Box::new(std::fs::File::open(path).unwrap()), Default::default());
        let mut hint = Hint::new();
        hint.with_extension(path.extension().unwrap().to_str().unwrap());
        let mut format = symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default()).unwrap();
        let track = format.default_track(TrackType::Audio).unwrap();
        let id = track.id;
        let params = track.codec_params.as_ref().and_then(|p| p.audio()).unwrap().clone();
        let shift = 32 - params.bits_per_sample.unwrap_or(32);
        let mut decoder = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).unwrap();
        let mut out = Vec::new();
        let mut scratch: Vec<i32> = Vec::new();
        while let Ok(Some(packet)) = format.next_packet() {
            if packet.track_id != id {
                continue;
            }
            let buf = decoder.decode(&packet).unwrap();
            scratch.resize(buf.samples_interleaved(), 0);
            buf.copy_to_slice_interleaved(&mut scratch);
            out.extend(scratch.iter().map(|s| s >> shift));
        }
        out
    }

    pub fn decode_frames(path: &Path) -> usize {
        decode_i32(path).len() / 2
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 19).unwrap().and_hms_opt(21, 5, 0).unwrap()
    }

    fn settings(folder: PathBuf, format: AudioFormat) -> RecordingSettings {
        RecordingSettings { folder, format, ..RecordingSettings::default() }
    }

    fn feed(frames: Vec<RecordFrame>, settings: &RecordingSettings) -> RecordingResult {
        let (mut tx, rx) = rtrb::RingBuffer::new(frames.len() + 1);
        for f in frames {
            tx.push(f).unwrap();
        }
        Recorder::start(settings, rx, Arc::new(RecordShared::default()), now()).unwrap().stop().unwrap()
    }

    #[test]
    fn gaps_are_filled_with_silence_to_keep_sync() {
        let dir = tempdir();
        let loud = tone(200, 24);
        let mut frames = vec![RecordFrame::Rate(48_000)];
        frames.extend(loud[..100].iter().map(|&f| RecordFrame::Audio(f)));
        frames.push(RecordFrame::Gap(50));
        frames.extend(loud[100..].iter().map(|&f| RecordFrame::Audio(f)));
        let result = feed(frames, &settings(dir.clone(), AudioFormat::Wav));
        assert!(result.path.ends_with("2026-10-19 21.05.wav"));
        assert!((result.duration_secs - 250.0 / 48_000.0).abs() < 1e-9);
        let samples = decode_i32(&result.path);
        assert_eq!(samples.len(), 500);
        assert!(samples[200..300].iter().all(|&s| s == 0));
        assert_eq!(samples[300], (loud[100][0] * 8_388_607.0).round() as i32);
    }

    #[test]
    fn a_44k1_stream_is_resampled_to_48k_with_the_right_length() {
        let dir = tempdir();
        let n = 44_100 * 3;
        let mut frames = vec![RecordFrame::Rate(44_100)];
        frames.extend((0..n).map(|i| {
            let x = (std::f64::consts::TAU * 1_000.0 * i as f64 / 44_100.0).sin() as f32 * 0.5;
            RecordFrame::Audio([x, x])
        }));
        let result = feed(frames, &settings(dir, AudioFormat::Flac));
        let got = decode_frames(&result.path);
        assert!((got as i64 - 144_000).abs() <= RESAMPLE_CHUNK as i64, "{got}");
        let samples = decode_i32(&result.path);
        let peak = samples[48_000..96_000].iter().map(|s| s.abs()).max().unwrap() as f64 / 8_388_607.0;
        assert!((peak - 0.5).abs() < 0.02, "{peak}");
    }

    #[test]
    fn an_unwritable_folder_fails_to_start() {
        let dir = tempdir();
        let file = dir.join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        let (_tx, rx) = rtrb::RingBuffer::<RecordFrame>::new(4);
        let err = Recorder::start(&settings(file.join("sub"), AudioFormat::Wav), rx, Arc::new(RecordShared::default()), now()).err().unwrap();
        assert!(matches!(err, RecordError::Folder(_)), "{err:?}");
    }

    #[test]
    fn a_second_recording_in_the_same_minute_gets_its_own_file() {
        let dir = tempdir();
        let s = settings(dir, AudioFormat::Wav);
        let a = feed(vec![RecordFrame::Rate(48_000)], &s);
        let b = feed(vec![RecordFrame::Rate(48_000)], &s);
        assert_ne!(a.path, b.path);
        assert!(b.path.ends_with("2026-10-19 21.05 (2).wav"));
    }

    #[test]
    fn low_free_space_warns_but_still_records() {
        let dir = tempdir();
        let s = settings(dir, AudioFormat::Wav);
        let (_tx, rx) = rtrb::RingBuffer::<RecordFrame>::new(4);
        let r = Recorder::start_with_free_space(&s, rx, Arc::new(RecordShared::default()), now(), LOW_SPACE_BYTES * 2).unwrap();
        assert!(r.status().warning.unwrap().contains("minutes of disk space"));
        r.stop().unwrap();
        let (_tx, rx) = rtrb::RingBuffer::<RecordFrame>::new(4);
        let err = Recorder::start_with_free_space(&s, rx, Arc::new(RecordShared::default()), now(), LOW_SPACE_BYTES / 2).err().unwrap();
        assert!(matches!(err, RecordError::DiskSpace { .. }), "{err:?}");
    }

    #[test]
    fn a_write_error_stops_the_recording_and_says_why() {
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        tx.push(RecordFrame::Rate(48_000)).unwrap();
        tx.push(RecordFrame::Audio([0.1, 0.1])).unwrap();
        let status = Arc::new(Mutex::new(RecorderStatus::default()));
        let worker = Worker::for_test(rx, Sink::Failing, status.clone());
        let finished = worker.run();
        assert!(finished.error.is_some());
        let s = status.lock().unwrap().clone();
        assert!(s.finished, "the app is told to close the recording");
        assert!(s.warning.unwrap().contains("disk full"), "the reason reaches the DJ");
    }
}
