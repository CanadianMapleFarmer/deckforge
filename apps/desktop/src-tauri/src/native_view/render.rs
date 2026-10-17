use std::sync::Arc;
use std::time::{Duration, Instant};

use deckforge_engine::DeckSnapshot;
use deckforge_gfx::gpu::Gpu;
use deckforge_gfx::scene::{DeckView, FrameInput, GridView, HotCueView, LoopView, PlacedSurface, DECKS};
use deckforge_gfx::wgpu;
use deckforge_gfx::{CssRect, DeckClock, DeckSample, Renderer, SurfaceId};

use super::shared::{Shared, Upload, Work};

const OCCLUDED_POLL: Duration = Duration::from_millis(50);

fn configure(surface: &wgpu::Surface<'static>, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
    let max = gpu.device.limits().max_texture_dimension_2d;
    surface.configure(
        &gpu.device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0.clamp(1, max),
            height: size.1.clamp(1, max),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        },
    );
}

fn deck_view(d: &DeckSnapshot, pos: f64, master: bool) -> DeckView {
    let mut view = DeckView {
        loaded: d.loaded,
        token: d.track_token,
        pos_secs: pos,
        duration_secs: d.duration_secs,
        playing: d.playing || d.previewing,
        tempo_ratio: d.tempo_ratio,
        cue_secs: d.cue_secs,
        grid: d.has_grid.then_some(GridView { bpm: d.grid_bpm, first_beat_secs: d.first_beat_secs }),
        master,
        ..Default::default()
    };
    for (dst, cue) in view.hot_cues.iter_mut().zip(&d.hot_cues) {
        *dst = cue.map(|c| HotCueView { secs: c.position_secs, rgb: c.color });
    }
    if let (Some(a), Some(b)) = (d.loop_in_secs, d.loop_out_secs) {
        view.loop_region = Some(LoopView { in_secs: a, out_secs: b, active: d.loop_active });
    }
    view
}

fn place(rects: &[(SurfaceId, CssRect)], scale: f64, size: (u32, u32)) -> Vec<PlacedSurface> {
    rects.iter().map(|(id, r)| PlacedSurface { id: *id, rect: r.to_pixels(scale, size) }).filter(|s| !s.rect.is_empty()).collect()
}

pub struct RenderThread {
    pub shared: Arc<Shared>,
    pub gpu: Gpu,
    pub surface: wgpu::Surface<'static>,
}

impl RenderThread {
    pub fn run(self) {
        let Self { shared, gpu, surface } = self;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
        let gpu_timing = renderer.enable_gpu_timing();
        shared.info.lock().gpu_timing = gpu_timing;
        let mut clocks = [DeckClock::new(), DeckClock::new()];
        let mut configured: (u32, u32) = (0, 0);
        let mut style_rev = 0;
        let mut snapshot_rev = 0;
        let mut drag_rev = [0u64; DECKS];
        let mut continuous = false;
        let mut refresh_secs = 1.0 / 60.0;
        let mut frames_since_refresh_update = 0u32;
        let log_stats = std::env::var_os("DECKFORGE_NATIVE_LOG_STATS").is_some();
        let mut last_stats_log = Instant::now();

        while let Some(work) = shared.next_work(continuous) {
            if work.window_px.0 == 0 || work.window_px.1 == 0 {
                continue;
            }
            if work.window_px != configured {
                configure(&surface, &gpu, format, work.window_px);
                configured = work.window_px;
            }
            if work.style_rev != style_rev {
                renderer.set_style(work.style.clone());
                style_rev = work.style_rev;
            }
            apply_uploads(&mut renderer, &work);

            let mut reconfigure_after_present = false;
            let frame = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) => f,
                wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                    reconfigure_after_present = true;
                    f
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    configure(&surface, &gpu, format, configured);
                    shared.update(|_| {});
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Validation => {
                    std::thread::sleep(OCCLUDED_POLL);
                    shared.update(|_| {});
                    continue;
                }
            };
            let acquired = Instant::now();
            let now = shared.now_secs();
            for deck in 0..DECKS {
                if work.drag_rev[deck] != drag_rev[deck] {
                    drag_rev[deck] = work.drag_rev[deck];
                    clocks[deck].set_drag(work.drag[deck], now);
                }
            }
            if let Some(ts) = work.snapshot.filter(|_| work.snapshot_rev != snapshot_rev) {
                snapshot_rev = work.snapshot_rev;
                for (clock, d) in clocks.iter_mut().zip(&ts.snapshot.decks) {
                    clock.push(DeckSample { pos: d.position_secs, rate: d.play_rate, moving: d.playing || d.previewing || d.jog_touched, token: d.track_token, at: ts.at });
                }
            }
            let snapshot = work.snapshot.map(|t| t.snapshot).unwrap_or_default();
            let display_time = now + refresh_secs - work.latency_secs - snapshot.output_latency_secs;
            let decks = [
                deck_view(&snapshot.decks[0], clocks[0].position(display_time), snapshot.master_deck == Some(0u8)),
                deck_view(&snapshot.decks[1], clocks[1].position(display_time), snapshot.master_deck == Some(1u8)),
            ];
            let surfaces = place(&work.rects, work.scale, configured);
            let input = FrameInput {
                size: configured,
                scale: work.scale,
                surfaces: &surfaces,
                decks: &decks,
                zoom_css_pps: work.zoom,
                hover_px: [None, None],
                anim_secs: now,
            };
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            renderer.render(&view, &input);
            let cpu_ms = acquired.elapsed().as_secs_f64() * 1000.0;
            gpu.queue.present(frame);
            if reconfigure_after_present {
                configure(&surface, &gpu, format, configured);
            }

            continuous = clocks.iter().any(|c| c.is_moving() || c.is_settling());
            shared.set_continuous(continuous);
            let mut stats = shared.stats.lock();
            stats.record_frame(shared.now_secs(), cpu_ms, continuous);
            frames_since_refresh_update += 1;
            if frames_since_refresh_update >= 30 {
                frames_since_refresh_update = 0;
                let report = stats.report();
                if report.window >= 30 {
                    refresh_secs = (report.p50_ms / 1000.0).clamp(1.0 / 240.0, 1.0 / 30.0);
                }
            }
            if !continuous {
                stats.mark_idle();
            }
            drop(stats);
            *shared.gpu_ms.lock() = renderer.last_gpu_ms();
            if log_stats && continuous && last_stats_log.elapsed() >= Duration::from_secs(3) {
                last_stats_log = Instant::now();
                log::info!("native frames: {:?} gpu_ms={:?}", shared.stats.lock().report(), renderer.last_gpu_ms());
            }
        }
    }
}

fn apply_uploads(renderer: &mut Renderer, work: &Work) {
    let live = |deck: usize, token: u64| {
        work.snapshot.is_some_and(|s| s.snapshot.decks[deck].loaded && s.snapshot.decks[deck].track_token == token)
    };
    for upload in &work.uploads {
        match upload {
            Upload::Pyramid { deck, token, pyramid } if live(*deck, *token) => {
                renderer.set_pyramid(*deck, *token, (**pyramid).clone());
            }
            Upload::Phrases { deck, token, phrases } if live(*deck, *token) => renderer.set_phrases(*deck, *token, phrases.clone()),
            Upload::Clear { deck } => renderer.clear_deck(*deck),
            _ => {}
        }
    }
}
