use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::engine::{Routing, RtEngine};
use crate::secondary::{
    secondary_channel, SecondaryOutputRequest, SecondaryOutputStatus, SecondaryRenderer, SecondaryShared, SecondaryState,
    DEFAULT_LEVEL, RING_FRAMES,
};

const PREFERRED_BUFFER: u32 = 256;
const NULL_BLOCK: usize = 256;
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const SECONDARY_BUFFER: u32 = 512;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDeviceInfo {
    pub id: String,
    pub name: String,
    pub max_channels: u16,
    pub sample_rates: Vec<u32>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRequest {
    pub device_id: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub routing: Option<Routing>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveOutput {
    pub device_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub requested_buffer: Option<u32>,
    pub routing: Routing,
    pub sample_format: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputState {
    Running,
    /// Device vanished; waiting for it to return. Engine keeps running silently.
    Lost,
    /// No device opened; engine runs silently so the UI stays live.
    Silent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputStatus {
    pub state: OutputState,
    pub active: Option<ActiveOutput>,
    pub last_error: Option<String>,
    pub callbacks: u64,
    pub last_callback_frames: u32,
    pub stream_errors: u32,
    pub xruns: u32,
    pub secondary: Option<SecondaryOutputStatus>,
}

struct Shared {
    lost: AtomicBool,
    errors: AtomicU32,
    xruns: AtomicU32,
    callbacks: AtomicU64,
    last_frames: AtomicU32,
}

enum Request {
    Open(OutputRequest, Sender<Result<ActiveOutput, String>>),
    Close,
    SetRouting(Routing),
    SetSecondary(Option<SecondaryOutputRequest>, Sender<Result<(), String>>),
}

/// Owns the audio stream on a dedicated thread (cpal streams are not Send on every platform).
pub struct AudioManager {
    tx: Sender<Request>,
    status: Arc<Mutex<OutputStatus>>,
    secondary: Arc<SecondaryShared>,
}

pub fn list_output_devices() -> Vec<OutputDeviceInfo> {
    let host = cpal::default_host();
    let default_id = host.default_output_device().and_then(|d| d.id().ok()).map(|id| id.to_string());
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    devices
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            let name = d.description().map(|desc| desc.name().to_string()).unwrap_or_else(|_| id.clone());
            let configs: Vec<_> = d.supported_output_configs().ok()?.collect();
            if configs.is_empty() {
                return None;
            }
            let max_channels = configs.iter().map(|c| c.channels()).max().unwrap_or(0);
            let mut sample_rates: Vec<u32> = [44_100, 48_000, 88_200, 96_000]
                .into_iter()
                .filter(|r| configs.iter().any(|c| c.min_sample_rate() <= *r && *r <= c.max_sample_rate()))
                .collect();
            sample_rates.dedup();
            Some(OutputDeviceInfo { is_default: default_id.as_deref() == Some(&id), id, name, max_channels, sample_rates })
        })
        .collect()
}

impl AudioManager {
    pub fn spawn(rt: RtEngine) -> Self {
        let (tx, rx) = bounded(16);
        let status = Arc::new(Mutex::new(OutputStatus {
            state: OutputState::Silent,
            active: None,
            last_error: None,
            callbacks: 0,
            last_callback_frames: 0,
            stream_errors: 0,
            xruns: 0,
            secondary: None,
        }));
        let secondary = SecondaryShared::new(Default::default(), DEFAULT_LEVEL);
        let (st, sec) = (status.clone(), secondary.clone());
        thread::Builder::new()
            .name("deckforge-audio-manager".into())
            .spawn(move || manager_loop(Arc::new(Mutex::new(rt)), rx, st, sec))
            .expect("spawn audio manager");
        Self { tx, status, secondary }
    }

    pub fn open(&self, request: OutputRequest) -> Result<ActiveOutput, String> {
        let (reply_tx, reply_rx) = bounded(1);
        self.tx.send(Request::Open(request, reply_tx)).map_err(|e| e.to_string())?;
        reply_rx.recv_timeout(Duration::from_secs(10)).map_err(|e| e.to_string())?
    }

    pub fn close(&self) {
        let _ = self.tx.send(Request::Close);
    }

    pub fn set_routing(&self, routing: Routing) {
        let _ = self.tx.send(Request::SetRouting(routing));
    }

    /// Starts, retargets or (with `None`) stops the additional output without touching the primary.
    pub fn set_secondary(&self, request: Option<SecondaryOutputRequest>) -> Result<(), String> {
        let (reply_tx, reply_rx) = bounded(1);
        self.tx.send(Request::SetSecondary(request, reply_tx)).map_err(|e| e.to_string())?;
        reply_rx.recv_timeout(Duration::from_secs(10)).map_err(|e| e.to_string())?
    }

    pub fn set_secondary_level(&self, level: f32) {
        self.secondary.set_level(level);
    }

    pub fn secondary_shared(&self) -> Arc<SecondaryShared> {
        self.secondary.clone()
    }

    pub fn status(&self) -> OutputStatus {
        let mut status = self.status.lock().clone();
        if let Some(secondary) = &mut status.secondary {
            secondary.level = self.secondary.level();
            secondary.source = self.secondary.source();
        }
        status
    }
}

struct Active {
    _stream: cpal::Stream,
    info: ActiveOutput,
    request: OutputRequest,
}

struct SecondaryInfo {
    device_id: String,
    device_name: String,
    sample_rate: u32,
}

struct SecondaryRuntime {
    request: SecondaryOutputRequest,
    info: SecondaryInfo,
    stream: Option<cpal::Stream>,
    last_attempt: Instant,
}

impl SecondaryRuntime {
    fn suspend(&mut self, rt: &Arc<Mutex<RtEngine>>) {
        self.stream = None;
        self.last_attempt = Instant::now();
        detach_tap(rt);
    }

    fn status(&self, shared: &SecondaryShared) -> SecondaryOutputStatus {
        let running = self.stream.is_some();
        let stats = shared.stats();
        SecondaryOutputStatus {
            device_id: self.info.device_id.clone(),
            device_name: self.info.device_name.clone(),
            source: shared.source(),
            level: shared.level(),
            state: if running { SecondaryState::Running } else { SecondaryState::Lost },
            sample_rate: self.info.sample_rate,
            latency_ms: if running { stats.latency_ms } else { 0.0 },
            drift_ppm: if running { stats.drift_ppm } else { 0.0 },
            underruns: stats.underruns,
            channels: self.request.channels,
        }
    }
}

fn detach_tap(rt: &Arc<Mutex<RtEngine>>) {
    let old = rt.lock().set_secondary_tap(None);
    drop(old);
}

fn manager_loop(
    rt: Arc<Mutex<RtEngine>>,
    rx: Receiver<Request>,
    status: Arc<Mutex<OutputStatus>>,
    secondary_shared: Arc<SecondaryShared>,
) {
    let shared = Arc::new(Shared {
        lost: AtomicBool::new(false),
        errors: AtomicU32::new(0),
        xruns: AtomicU32::new(0),
        callbacks: AtomicU64::new(0),
        last_frames: AtomicU32::new(0),
    });
    let mut active: Option<Active> = None;
    let mut primary_device: Option<String> = None;
    let mut secondary: Option<SecondaryRuntime> = None;
    let mut lost_request: Option<OutputRequest> = None;
    let mut last_attempt = Instant::now();
    let mut null_buf = vec![0f32; NULL_BLOCK * 2];
    let mut null_clock = Instant::now();

    loop {
        let timeout = if active.is_some() { Duration::from_millis(200) } else { Duration::from_millis(5) };
        match rx.recv_timeout(timeout) {
            Ok(Request::Open(req, reply)) => {
                active = None;
                lost_request = None;
                let result = open_stream(&rt, &shared, &req);
                let reply_value = match result {
                    Ok(a) => {
                        let info = a.info.clone();
                        primary_device = Some(info.device_id.clone());
                        active = Some(a);
                        status.lock().last_error = None;
                        Ok(info)
                    }
                    Err(e) => {
                        primary_device = None;
                        status.lock().last_error = Some(e.clone());
                        Err(e)
                    }
                };
                let _ = reply.send(reply_value);
                null_clock = Instant::now();
            }
            Ok(Request::Close) => {
                active = None;
                primary_device = None;
                lost_request = None;
                null_clock = Instant::now();
            }
            Ok(Request::SetSecondary(request, reply)) => {
                let result = apply_secondary(&rt, &secondary_shared, &mut secondary, primary_device.as_deref(), request);
                let _ = reply.send(result);
            }
            Ok(Request::SetRouting(routing)) => {
                let mut engine = rt.lock();
                let sr = engine.sample_rate();
                engine.reconfigure(sr, routing);
                if let Some(a) = &mut active {
                    a.info.routing = routing;
                    a.request.routing = Some(routing);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }

        if shared.lost.swap(false, Ordering::AcqRel)
            && let Some(a) = active.take()
        {
            log::warn!("audio device lost: {}", a.info.device_name);
            lost_request = Some(a.request);
            last_attempt = Instant::now();
            null_clock = Instant::now();
        }
        if let Some(req) = &lost_request
            && last_attempt.elapsed() >= RECONNECT_INTERVAL
        {
            last_attempt = Instant::now();
            if device_present(req.device_id.as_deref())
                && let Ok(a) = open_stream(&rt, &shared, req)
            {
                log::info!("audio device reconnected: {}", a.info.device_name);
                active = Some(a);
                lost_request = None;
            }
        }

        if let Some(sec) = &mut secondary {
            if secondary_shared.take_lost() && sec.stream.is_some() {
                log::warn!("secondary audio device lost: {}", sec.info.device_name);
                sec.suspend(&rt);
            }
            let contested = primary_device.as_deref() == Some(sec.info.device_id.as_str());
            if contested && sec.stream.is_some() {
                log::info!("secondary output paused: primary output moved to {}", sec.info.device_name);
                sec.suspend(&rt);
            } else if sec.stream.is_none()
                && !contested
                && sec.last_attempt.elapsed() >= RECONNECT_INTERVAL
            {
                sec.last_attempt = Instant::now();
                if device_present(Some(&sec.info.device_id))
                    && let Ok((stream, info)) = open_secondary(&rt, &secondary_shared, &sec.request)
                {
                    log::info!("secondary audio device reconnected: {}", info.device_name);
                    sec.stream = Some(stream);
                    sec.info = info;
                }
            }
        }

        if active.is_none() {
            let due = (null_clock.elapsed().as_secs_f64() * rt.lock().sample_rate() as f64) as usize;
            let mut rendered = 0;
            while rendered + NULL_BLOCK <= due {
                if let Some(mut e) = rt.try_lock() {
                    e.process(&mut null_buf, 2);
                }
                rendered += NULL_BLOCK;
            }
            if rendered > 0 {
                let sr = rt.lock().sample_rate() as f64;
                null_clock += Duration::from_secs_f64(rendered as f64 / sr);
            }
        }

        let mut s = status.lock();
        s.state = if active.is_some() {
            OutputState::Running
        } else if lost_request.is_some() {
            OutputState::Lost
        } else {
            OutputState::Silent
        };
        s.active = active.as_ref().map(|a| a.info.clone());
        s.callbacks = shared.callbacks.load(Ordering::Relaxed);
        s.last_callback_frames = shared.last_frames.load(Ordering::Relaxed);
        s.stream_errors = shared.errors.load(Ordering::Relaxed);
        s.xruns = shared.xruns.load(Ordering::Relaxed);
        s.secondary = secondary.as_ref().map(|sec| sec.status(&secondary_shared));
    }
}

fn apply_secondary(
    rt: &Arc<Mutex<RtEngine>>,
    shared: &Arc<SecondaryShared>,
    secondary: &mut Option<SecondaryRuntime>,
    primary_device: Option<&str>,
    request: Option<SecondaryOutputRequest>,
) -> Result<(), String> {
    let Some(request) = request else {
        *secondary = None;
        detach_tap(rt);
        return Ok(());
    };
    if primary_device == Some(request.device_id.as_str()) {
        return Err("that device is already the main output; pick a different one for the additional output".into());
    }
    *secondary = None;
    detach_tap(rt);
    shared.set_source(request.source);
    shared.set_level(request.level);
    let (stream, info) = open_secondary(rt, shared, &request)?;
    *secondary = Some(SecondaryRuntime { request, info, stream: Some(stream), last_attempt: Instant::now() });
    Ok(())
}

fn device_present(id: Option<&str>) -> bool {
    let host = cpal::default_host();
    match id {
        None => host.default_output_device().is_some(),
        Some(id) => host
            .output_devices()
            .map(|mut ds| ds.any(|d| d.id().map(|i| i.to_string() == id).unwrap_or(false)))
            .unwrap_or(false),
    }
}

fn find_device(id: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    match id {
        None => host.default_output_device().ok_or_else(|| "no default output device".to_string()),
        Some(id) => host
            .output_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.id().map(|i| i.to_string() == id).unwrap_or(false))
            .ok_or_else(|| format!("output device not found: {id}")),
    }
}

fn rate_ok(c: &cpal::SupportedStreamConfigRange, rate: u32) -> bool {
    c.min_sample_rate() <= rate && rate <= c.max_sample_rate()
}

fn format_rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I16 => 2,
        _ => 9,
    }
}

fn pick_buffer(range: &SupportedBufferSize, wanted: u32) -> BufferSize {
    match range {
        SupportedBufferSize::Range { min, max } if *min <= wanted && wanted <= *max => BufferSize::Fixed(wanted),
        _ => BufferSize::Default,
    }
}

fn open_stream(rt: &Arc<Mutex<RtEngine>>, shared: &Arc<Shared>, req: &OutputRequest) -> Result<Active, String> {
    let device = find_device(req.device_id.as_deref())?;
    let device_id = device.id().map(|i| i.to_string()).map_err(|e| e.to_string())?;
    let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| device_id.clone());
    let default_rate = device.default_output_config().map(|c| c.sample_rate()).unwrap_or(48_000);
    let wanted_rate = req.sample_rate.unwrap_or(default_rate);

    let ranges: Vec<_> = device.supported_output_configs().map_err(|e| e.to_string())?.collect();
    let channel_rank = |ch: u16| if ch >= 4 { (0, ch) } else { (1, u16::MAX - ch) };
    let candidate = ranges
        .iter()
        .filter(|c| format_rank(c.sample_format()) < 9)
        .filter(|c| rate_ok(c, wanted_rate) || rate_ok(c, default_rate))
        .min_by_key(|c| (channel_rank(c.channels()), format_rank(c.sample_format())))
        .ok_or_else(|| format!("{device_name}: no usable f32/i32/i16 output configuration"))?;
    let rate = if rate_ok(candidate, wanted_rate) { wanted_rate } else { default_rate };
    let supported = candidate.with_sample_rate(rate);
    let format = supported.sample_format();
    let channels = supported.channels();
    let buffer = pick_buffer(supported.buffer_size(), req.buffer_frames.unwrap_or(PREFERRED_BUFFER));
    let config = StreamConfig { channels, sample_rate: rate, buffer_size: buffer };
    let routing = req
        .routing
        .filter(|r| r.master.iter().chain(r.phones.iter()).all(|&c| c < channels as usize))
        .unwrap_or_else(|| Routing::for_channel_count(channels as usize));

    rt.lock().reconfigure(rate, routing);
    shared.lost.store(false, Ordering::Release);

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, rt.clone(), shared.clone()),
        SampleFormat::I32 => build::<i32>(&device, &config, rt.clone(), shared.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config, rt.clone(), shared.clone()),
        other => Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    log::info!("audio output: {device_name} {rate} Hz, {channels} ch, {format}, {buffer:?}");
    Ok(Active {
        _stream: stream,
        info: ActiveOutput {
            device_id,
            device_name,
            sample_rate: rate,
            channels,
            requested_buffer: match buffer {
                BufferSize::Fixed(n) => Some(n),
                BufferSize::Default => None,
            },
            routing,
            sample_format: format.to_string(),
        },
        request: OutputRequest { routing: Some(routing), ..req.clone() },
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    rt: Arc<Mutex<RtEngine>>,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + 'static,
{
    let channels = config.channels as usize;
    let mut scratch: Vec<f32> = vec![0.0; 8192 * channels];
    let err_shared = shared.clone();
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                shared.callbacks.fetch_add(1, Ordering::Relaxed);
                shared.last_frames.store((data.len() / channels) as u32, Ordering::Relaxed);
                if scratch.len() < data.len() {
                    scratch.resize(data.len(), 0.0);
                }
                let buf = &mut scratch[..data.len()];
                let stamp = info.timestamp();
                let latency = stamp.playback.duration_since(stamp.callback).as_secs_f64();
                match rt.try_lock() {
                    Some(mut engine) => {
                        engine.set_device_latency(latency);
                        engine.process(buf, channels)
                    }
                    None => buf.fill(0.0),
                }
                for (o, s) in data.iter_mut().zip(buf.iter()) {
                    *o = T::from_sample(*s);
                }
            },
            move |err: cpal::Error| {
                match err.kind() {
                    cpal::ErrorKind::Xrun => {
                        err_shared.xruns.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    cpal::ErrorKind::DeviceNotAvailable => err_shared.lost.store(true, Ordering::Release),
                    _ => {}
                }
                err_shared.errors.fetch_add(1, Ordering::Relaxed);
                log::warn!("audio stream error: {err}");
            },
            None,
        )
        .map_err(|e| e.to_string())
}

fn open_secondary(
    rt: &Arc<Mutex<RtEngine>>,
    shared: &Arc<SecondaryShared>,
    req: &SecondaryOutputRequest,
) -> Result<(cpal::Stream, SecondaryInfo), String> {
    let device = find_device(Some(&req.device_id))?;
    let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| req.device_id.clone());
    let default_rate = device.default_output_config().map(|c| c.sample_rate()).unwrap_or(48_000);
    let needed = req.channels[0].max(req.channels[1]) + 1;
    let ranges: Vec<_> = device.supported_output_configs().map_err(|e| e.to_string())?.collect();
    let candidate = ranges
        .iter()
        .filter(|c| format_rank(c.sample_format()) < 9 && c.channels() as usize >= needed && rate_ok(c, default_rate))
        .min_by_key(|c| (c.channels(), format_rank(c.sample_format())))
        .ok_or_else(|| format!("{device_name}: no output configuration with {needed} channels at {default_rate} Hz"))?;
    let supported = candidate.with_sample_rate(default_rate);
    let format = supported.sample_format();
    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate: default_rate,
        buffer_size: pick_buffer(supported.buffer_size(), SECONDARY_BUFFER),
    };

    shared.reset_stats();
    shared.clear_lost();
    let primary_rate = rt.lock().sample_rate();
    let (tap, renderer) = secondary_channel(shared.clone(), primary_rate, default_rate, RING_FRAMES);
    let pair = req.channels;
    let stream = match format {
        SampleFormat::F32 => build_secondary::<f32>(&device, &config, renderer, pair, shared.clone()),
        SampleFormat::I32 => build_secondary::<i32>(&device, &config, renderer, pair, shared.clone()),
        SampleFormat::I16 => build_secondary::<i16>(&device, &config, renderer, pair, shared.clone()),
        other => Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    let previous = rt.lock().set_secondary_tap(Some(tap));
    drop(previous);
    log::info!("secondary output: {device_name} {default_rate} Hz, {} ch, {format}", config.channels);
    Ok((stream, SecondaryInfo { device_id: req.device_id.clone(), device_name, sample_rate: default_rate }))
}

fn build_secondary<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut renderer: SecondaryRenderer,
    pair: [usize; 2],
    shared: Arc<SecondaryShared>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + 'static,
{
    let channels = config.channels as usize;
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                renderer.begin_block(data.len() / channels);
                for frame in data.chunks_exact_mut(channels) {
                    let [left, right] = renderer.next_frame();
                    frame.fill(T::EQUILIBRIUM);
                    frame[pair[0]] = T::from_sample(left);
                    frame[pair[1]] = T::from_sample(right);
                }
            },
            move |err: cpal::Error| match err.kind() {
                cpal::ErrorKind::Xrun => {}
                cpal::ErrorKind::DeviceNotAvailable => shared.mark_lost(),
                _ => log::warn!("secondary audio stream error: {err}"),
            },
            None,
        )
        .map_err(|e| e.to_string())
}
