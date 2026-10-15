use deckforge_gfx::gpu::Gpu;
use deckforge_gfx::offscreen::{Image, OffscreenRenderer};
use deckforge_gfx::scene::{
    DECKS, DeckView, FrameInput, PhraseKind, PhraseView, PlacedSurface, SurfaceId, SurfaceKind,
};
use deckforge_gfx::synth::{synth_deck, synth_phrases, synth_track};
use deckforge_gfx::waveform::WaveformData;
use deckforge_gfx::{PixelRect, Style};

const DURATION: f64 = 240.0;
const BPM: f64 = 124.0;
const SCALE: f64 = 2.0;

fn renderer() -> Option<OffscreenRenderer> {
    match Gpu::headless() {
        Ok(_) => OffscreenRenderer::new().ok(),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

struct Scene {
    r: OffscreenRenderer,
    decks: [DeckView; DECKS],
}

fn scene(style: Style) -> Option<Scene> {
    let mut r = renderer()?;
    let wf = synth_track(DURATION, BPM);
    r.renderer.set_waveform(0, 1, Some(&wf));
    r.renderer.set_waveform(1, 2, Some(&wf));
    r.renderer.set_style(style);
    r.renderer.set_phrases(0, 1, synth_phrases(DURATION, BPM));
    r.renderer.set_phrases(1, 2, synth_phrases(DURATION, BPM));
    Some(Scene {
        r,
        decks: [
            synth_deck(60.0, DURATION, BPM, 1),
            synth_deck(61.0, DURATION, BPM, 2),
        ],
    })
}

fn render(s: &mut Scene, size: (u32, u32), surfaces: &[PlacedSurface], zoom: f64) -> Image {
    let input = FrameInput {
        size,
        scale: SCALE,
        surfaces,
        decks: &s.decks,
        zoom_css_pps: [zoom, zoom],
        hover_px: [None, None],
        anim_secs: 0.0,
    };
    s.r.render(&input).expect("render")
}

fn place(id: SurfaceId, x: u32, y: u32, w: u32, h: u32) -> PlacedSurface {
    PlacedSurface {
        id,
        rect: PixelRect { x, y, w, h },
    }
}

fn luma(p: [u8; 4]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

fn bg_pixel(style: &Style) -> [u8; 4] {
    let c = style.bg;
    [
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
        255,
    ]
}

#[test]
fn scrolling_view_draws_inside_its_rect_only_with_playhead_on_the_centre() {
    let style = Style::default();
    let Some(mut s) = scene(style.clone()) else {
        return;
    };
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        200,
        100,
        1200,
        300,
    );
    let img = render(&mut s, (1600, 600), &[rect], 90.0);
    let bg = bg_pixel(&style);
    assert_eq!(img.pixel(50, 50), bg);
    assert_eq!(img.pixel(1500, 500), bg);
    assert_eq!(
        img.pixel(199, 250),
        bg,
        "pixel just left of the rect stays background"
    );
    assert_eq!(
        img.pixel(1400, 250),
        bg,
        "pixel just right of the rect stays background"
    );
    let head = img.pixel(200 + 600, 250);
    assert!(
        head[0] > 230 && head[1] > 230 && head[2] > 230,
        "playhead is bright: {head:?}"
    );
    assert_ne!(img.pixel(200 + 700, 250), bg, "waveform body is drawn");
}

#[test]
fn played_side_is_dimmer_than_the_unplayed_side() {
    let Some(mut s) = scene(Style::default()) else {
        return;
    };
    s.decks[0].grid = None;
    s.decks[0].hot_cues = Default::default();
    s.decks[0].loop_region = None;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        300,
    );
    let img = render(&mut s, (1200, 300), &[rect], 90.0);
    let mean = |x0: u32, x1: u32| -> f32 {
        let mut sum = 0.0;
        let mut n = 0.0;
        for x in x0..x1 {
            for y in 0..300 {
                sum += luma(img.pixel(x, y));
                n += 1.0;
            }
        }
        sum / n
    };
    assert!(
        mean(610, 1150) > mean(50, 590) * 1.15,
        "unplayed {} vs played {}",
        mean(610, 1150),
        mean(50, 590)
    );
}

#[test]
fn waveform_is_mirrored_around_the_centre_line() {
    let Some(mut s) = scene(Style::default()) else {
        return;
    };
    s.decks[0].grid = None;
    s.decks[0].hot_cues = Default::default();
    s.decks[0].loop_region = None;
    s.r.renderer.set_phrases(0, 1, Vec::new());
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        300,
    );
    let img = render(&mut s, (1200, 300), &[rect], 90.0);
    for x in [700u32, 800, 905, 1100] {
        for dy in [10u32, 40, 80, 120] {
            let up = luma(img.pixel(x, 150 - dy));
            let down = luma(img.pixel(x, 149 + dy));
            assert!((up - down).abs() < 3.0, "x {x} dy {dy}: {up} vs {down}");
        }
    }
}

#[test]
fn columns_are_anchored_to_track_time_so_one_pixel_of_scroll_is_a_pure_shift() {
    let style = Style {
        sub_pixel: false,
        ..Style::default()
    };
    let Some(mut s) = scene(style) else { return };
    if s.r.gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu {
        eprintln!("skipping bit-exact shift test: software rasterizers do not promise identical float rounding");
        return;
    }
    for d in &mut s.decks {
        d.grid = None;
        d.hot_cues = Default::default();
        d.loop_region = None;
    }
    let zoom = 90.0;
    let pps_dev = zoom * SCALE;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        200,
    );
    s.decks[0].pos_secs = 100.25;
    let a = render(&mut s, (1200, 200), &[rect], zoom);
    s.decks[0].pos_secs = 100.25 + 1.0 / pps_dev;
    let b = render(&mut s, (1200, 200), &[rect], zoom);
    let mut diff = 0u32;
    for x in 700..1150 {
        for y in 0..200 {
            if a.pixel(x + 1, y) != b.pixel(x, y) {
                diff += 1;
            }
        }
    }
    assert_eq!(
        diff, 0,
        "shifting by exactly one device pixel must only translate the image"
    );
    s.decks[0].pos_secs = 100.25 + 0.3 / pps_dev;
    let c = render(&mut s, (1200, 200), &[rect], zoom);
    let unchanged = (700..1150).all(|x| (0..200).all(|y| a.pixel(x, y) == c.pixel(x, y)));
    assert!(
        unchanged,
        "a sub-pixel move must not change nearest-bucket output"
    );
}

#[test]
fn sub_pixel_scrolling_changes_columns_gradually_without_crawl() {
    let Some(mut s) = scene(Style::default()) else {
        return;
    };
    for d in &mut s.decks {
        d.grid = None;
        d.hot_cues = Default::default();
        d.loop_region = None;
    }
    let zoom = 90.0;
    let pps_dev = zoom * SCALE;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        200,
    );
    let mean_diff = |a: &Image, b: &Image| -> f32 {
        let mut sum = 0.0;
        for x in 700..1150 {
            for y in 0..200 {
                sum += (luma(a.pixel(x, y)) - luma(b.pixel(x, y))).abs();
            }
        }
        sum / (450.0 * 200.0)
    };
    s.decks[0].pos_secs = 100.0;
    let base = render(&mut s, (1200, 200), &[rect], zoom);
    s.decks[0].pos_secs = 100.0 + 1.0 / pps_dev;
    let full = mean_diff(&base, &render(&mut s, (1200, 200), &[rect], zoom));
    let mut last = 0.0;
    for step in 1..=8 {
        s.decks[0].pos_secs = 100.0 + step as f64 * 0.125 / pps_dev;
        let d = mean_diff(&base, &render(&mut s, (1200, 200), &[rect], zoom));
        assert!(
            d >= last - 0.01,
            "difference must grow smoothly with the offset: {d} after {last}"
        );
        last = d;
        if step == 1 {
            assert!(
                d < full * 0.4,
                "an eighth of a pixel changed the image by {d} vs {full} for a full pixel"
            );
        }
    }
}

#[test]
fn sub_pixel_scrolling_keeps_peak_heights_constant() {
    let Some(mut s) = scene(Style::default()) else {
        return;
    };
    let level: Vec<u8> = (0..(DURATION * 150.0) as usize)
        .map(|i| if i % 23 == 0 { 255 } else { 20 })
        .collect();
    let spikes = WaveformData {
        points_per_sec: 150.0,
        peak: level.clone(),
        low: level.clone(),
        mid: level.clone(),
        high: level,
    };
    s.r.renderer.set_waveform(0, 1, Some(&spikes));
    s.r.renderer.set_phrases(0, 1, Vec::new());
    s.decks[0].grid = None;
    s.decks[0].hot_cues = Default::default();
    s.decks[0].loop_region = None;
    let zoom = 75.0;
    let pps_dev = zoom * SCALE;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        200,
    );
    let ink_near_tips = |img: &Image| -> f32 {
        let bg = luma(img.pixel(1199, 1));
        (700..1150)
            .map(|x| (luma(img.pixel(x, 25)) - bg).max(0.0))
            .sum()
    };
    s.decks[0].pos_secs = 100.0;
    let aligned = ink_near_tips(&render(&mut s, (1200, 200), &[rect], zoom));
    assert!(aligned > 0.0, "the spikes must reach the measured row");
    for step in 1..8 {
        s.decks[0].pos_secs = 100.0 + step as f64 * 0.125 / pps_dev;
        let ink = ink_near_tips(&render(&mut s, (1200, 200), &[rect], zoom));
        assert!(
            (ink - aligned).abs() < aligned * 0.05,
            "peaks shrank or grew mid-pixel: {ink} vs {aligned} at offset {}/8",
            step
        );
    }
}

#[test]
fn sub_pixel_grid_lines_move_with_the_waveform() {
    let Some(mut s) = scene(Style::default()) else {
        return;
    };
    s.r.renderer.set_waveform(0, 1, None);
    s.decks[0].hot_cues = Default::default();
    s.decks[0].loop_region = None;
    s.decks[0].cue_secs = 0.0;
    let zoom = 75.0;
    let pps_dev = zoom * SCALE;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        200,
    );
    let bg = luma(bg_pixel(&Style::default()));
    let centroid = |img: &Image, from: u32| -> f64 {
        let start = (from..1190)
            .find(|&x| luma(img.pixel(x, 60)) - bg > 4.0)
            .expect("a grid line right of the playhead");
        let (mut sum, mut weight) = (0.0, 0.0);
        for x in start - 2..start + 8 {
            let w = f64::from((luma(img.pixel(x, 60)) - bg).max(0.0));
            sum += w * (f64::from(x) + 0.5);
            weight += w;
        }
        sum / weight
    };
    s.decks[0].pos_secs = 100.0;
    let base = centroid(&render(&mut s, (1200, 200), &[rect], zoom), 620);
    for step in 1..4 {
        let offset = step as f64 * 0.25;
        s.decks[0].pos_secs = 100.0 + offset / pps_dev;
        let moved = centroid(&render(&mut s, (1200, 200), &[rect], zoom), 620);
        assert!(
            (base - moved - offset).abs() < 0.08,
            "grid line moved {} px for a {offset} px scroll",
            base - moved
        );
    }
}

#[test]
fn overview_rises_from_the_baseline_and_places_the_playhead_proportionally() {
    let style = Style::default();
    let Some(mut s) = scene(style.clone()) else {
        return;
    };
    s.decks[0].pos_secs = 60.0;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Overview,
            deck: 0,
        },
        100,
        50,
        1000,
        120,
    );
    let img = render(&mut s, (1200, 300), &[rect], 90.0);
    let bg = bg_pixel(&style);
    let column = |x: u32| -> (u32, u32) {
        let lit: Vec<u32> = (50..170).filter(|&y| img.pixel(x, y) != bg).collect();
        (*lit.first().unwrap_or(&0), *lit.last().unwrap_or(&0))
    };
    let (top, bottom) = column(100 + 700);
    assert!(bottom >= 140, "bars reach the baseline: {bottom}");
    assert!(top > 50, "tallest bars stay inside the rect");
    let head_x = 100 + (60.0 / DURATION * 1000.0) as u32;
    let head = img.pixel(head_x, 90);
    assert!(
        head[0] > 230 && head[1] > 230 && head[2] > 230,
        "playhead at {head_x}: {head:?}"
    );
    assert_eq!(img.pixel(50, 100), bg);
    let phrase_band = img.pixel(100 + 400, 50 + 120 - 6);
    assert_ne!(
        phrase_band, bg,
        "phrase band is drawn under the waveform: {phrase_band:?}"
    );
}

#[test]
fn two_surfaces_on_one_target_do_not_bleed_into_each_other() {
    let style = Style::default();
    let Some(mut s) = scene(style.clone()) else {
        return;
    };
    let a = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        600,
        200,
    );
    let b = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 1,
        },
        600,
        0,
        600,
        200,
    );
    let img = render(&mut s, (1200, 220), &[a, b], 90.0);
    assert_eq!(img.pixel(300, 210), bg_pixel(&style));
    let head_a = img.pixel(300, 100);
    let head_b = img.pixel(900, 100);
    assert!(head_a[0] > 230 && head_b[0] > 230);
}

#[test]
fn saves_review_renders() {
    let Some(mut s) = scene(Style::default()) else {
        return;
    };
    let Some(dir) = std::env::var_os("DECKFORGE_GFX_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let surfaces = |w: u32| {
        [
            place(
                SurfaceId {
                    kind: SurfaceKind::Overview,
                    deck: 0,
                },
                0,
                0,
                w,
                120,
            ),
            place(
                SurfaceId {
                    kind: SurfaceKind::Scroll,
                    deck: 0,
                },
                0,
                140,
                w,
                300,
            ),
            place(
                SurfaceId {
                    kind: SurfaceKind::Scroll,
                    deck: 1,
                },
                0,
                460,
                w,
                300,
            ),
            place(
                SurfaceId {
                    kind: SurfaceKind::Overview,
                    deck: 1,
                },
                0,
                780,
                w,
                120,
            ),
        ]
    };
    s.decks[0].pos_secs = 62.3;
    s.decks[1].pos_secs = 118.7;
    s.decks[0].master = true;
    for (name, zoom) in [("scroll-default", 90.0), ("scroll-zoomed-out", 28.0)] {
        let img = render(&mut s, (2400, 920), &surfaces(2400), zoom);
        img.write_png(&dir.join(format!("native-{name}.png")))
            .expect("png");
    }
}

struct OddArrangement {
    bpm: f64,
    drop_start: f64,
    phrases: Vec<PhraseView>,
    waveform: WaveformData,
}

fn odd_arrangement() -> OddArrangement {
    let bpm = 124.0;
    let beat = 60.0 / bpm;
    let bar = beat * 4.0;
    let plan = [
        (PhraseKind::Intro, 8.0, false),
        (PhraseKind::Build, 2.0, false),
        (PhraseKind::Drop, 16.0, true),
        (PhraseKind::Break, 8.0, false),
        (PhraseKind::Drop, 16.0, true),
    ];
    let mut phrases = Vec::new();
    let mut t = 0.18;
    let mut kick_spans = Vec::new();
    for (kind, bars, kick) in plan {
        let end = t + bars * bar;
        phrases.push(PhraseView {
            start_secs: t,
            end_secs: end,
            kind,
            energy: if kick { 1.0 } else { 0.4 },
            fill_start_secs: (kind == PhraseKind::Build).then_some(end - bar),
        });
        if kick {
            kick_spans.push((t, end));
        }
        t = end;
    }
    let pps = 150.0;
    let n = (t * pps) as usize;
    let (mut peak, mut low, mut mid, mut high) =
        (vec![0u8; n], vec![0u8; n], vec![0u8; n], vec![0u8; n]);
    for i in 0..n {
        let secs = i as f64 / pps;
        let kicking = kick_spans.iter().any(|(a, b)| secs >= *a && secs < *b);
        let phase = ((secs - 0.18) / beat).rem_euclid(1.0);
        let kick = if kicking { (-phase * 7.0).exp() } else { 0.0 };
        let hat = (-((phase + 0.5) % 1.0 * 9.0)).exp() * 0.5;
        let pad = if kicking { 0.55 } else { 0.25 };
        let height = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        low[i] = height(kick);
        mid[i] = height(pad);
        high[i] = height(hat);
        peak[i] = height((kick + pad * 0.6 + hat * 0.4).min(1.0));
    }
    OddArrangement {
        bpm,
        drop_start: phrases[2].start_secs,
        phrases,
        waveform: WaveformData {
            points_per_sec: pps,
            peak,
            low,
            mid,
            high,
        },
    }
}

fn odd_scene(pos: f64) -> Option<(Scene, OddArrangement)> {
    let a = odd_arrangement();
    let mut r = renderer()?;
    r.renderer.set_waveform(0, 1, Some(&a.waveform));
    r.renderer.set_waveform(1, 2, Some(&a.waveform));
    r.renderer.set_phrases(0, 1, a.phrases.clone());
    r.renderer.set_phrases(1, 2, a.phrases.clone());
    let mut decks = [
        synth_deck(pos, a.waveform.duration_secs(), a.bpm, 1),
        synth_deck(pos, a.waveform.duration_secs(), a.bpm, 2),
    ];
    for d in &mut decks {
        d.hot_cues = Default::default();
        d.loop_region = None;
        d.cue_secs = 0.0;
        d.grid.as_mut().unwrap().first_beat_secs = 0.18;
        d.tempo_ratio = 1.0;
    }
    Some((Scene { r, decks }, a))
}

#[test]
fn phrase_anchored_lines_replace_the_global_four_bar_lattice() {
    let Some((mut s, a)) = odd_scene(0.0) else {
        return;
    };
    let bar = 60.0 / a.bpm * 4.0;
    let (zoom, w) = (90.0, 2000u32);
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        w,
        300,
    );
    s.r.renderer.set_waveform(0, 1, None);
    let check_zoom = 40.0;
    s.decks[0].pos_secs = a.drop_start + 3.0 * bar;
    let img = render(&mut s, (w, 300), &[rect], check_zoom);
    s.r.renderer.set_waveform(0, 1, Some(&a.waveform));
    let pps = check_zoom * SCALE;
    let x_at = |t: f64| (1000.0 + (t - s.decks[0].pos_secs) * pps).round() as u32;
    let (bottom, mid) = (297, 200);
    let drop_line = img.pixel(x_at(a.drop_start), bottom);
    assert!(
        drop_line[0] > 200 && drop_line[1] < 150,
        "phrase start is tinted by its phrase colour: {drop_line:?}"
    );
    let lattice = luma(img.pixel(x_at(a.phrases[0].start_secs + 12.0 * bar), mid));
    let four = luma(img.pixel(x_at(a.drop_start + 4.0 * bar), mid));
    assert!(
        four > lattice + 40.0,
        "the 4-bar line counted from the phrase start outranks the global lattice position: {four} vs {lattice}"
    );
    let eight = img.pixel(x_at(a.drop_start + 8.0 * bar), mid);
    assert!(
        eight[2] > eight[0] + 60,
        "8 bars into the drop is a deck-coloured line: {eight:?}"
    );
    if let Some(dir) = std::env::var_os("DECKFORGE_PHRASE_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        let surfaces = [
            place(
                SurfaceId {
                    kind: SurfaceKind::Overview,
                    deck: 0,
                },
                0,
                0,
                w,
                100,
            ),
            place(
                SurfaceId {
                    kind: SurfaceKind::Scroll,
                    deck: 0,
                },
                0,
                120,
                w,
                300,
            ),
            place(
                SurfaceId {
                    kind: SurfaceKind::Scroll,
                    deck: 1,
                },
                0,
                440,
                w,
                300,
            ),
        ];
        s.decks[0].pos_secs = a.drop_start - 0.6 * bar;
        s.decks[1].pos_secs = a.phrases[3].end_secs - 0.5 * bar;
        let img = render(&mut s, (w, 760), &surfaces, zoom);
        img.write_png(&dir.join("phrase-anchored-native.png"))
            .expect("png");
    }
}

#[test]
fn the_current_phrase_tab_is_pinned_to_the_left_edge_with_its_name() {
    let style = Style::default();
    let Some((mut s, a)) = odd_scene(0.0) else {
        return;
    };
    let bar = 60.0 / a.bpm * 4.0;
    s.decks[0].pos_secs = a.drop_start + 12.0 * bar;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1200,
        300,
    );
    let img = render(&mut s, (1200, 300), &[rect], 90.0);
    let drop = style.phrases[2];
    let near_drop = |p: [u8; 4]| {
        (f32::from(p[0]) - drop[0] * 255.0).abs() < 12.0
            && (f32::from(p[1]) - drop[1] * 255.0).abs() < 12.0
    };
    assert!(
        near_drop(img.pixel(2, 4)),
        "tab fill at the left edge: {:?}",
        img.pixel(2, 4)
    );
    let ink = (6..80)
        .flat_map(|x| (4..28).map(move |y| (x, y)))
        .filter(|&(x, y)| luma(img.pixel(x, y)) < 60.0)
        .count();
    assert!(ink > 40, "the tab carries a dark label: {ink} ink pixels");
    let band_bottom = 32;
    assert!(
        !near_drop(img.pixel(2, band_bottom + 20)),
        "the tab stays inside the band"
    );
}

#[test]
fn the_ghost_marker_shows_where_the_other_decks_next_phrase_lands() {
    let style = Style::default();
    let Some((mut s, a)) = odd_scene(0.0) else {
        return;
    };
    let bar = 60.0 / a.bpm * 4.0;
    s.r.renderer.set_waveform(0, 1, None);
    s.decks[0].master = true;
    s.decks[0].pos_secs = a.drop_start + 2.0 * bar;
    s.decks[1].pos_secs = a.phrases[3].start_secs - 3.0 * bar;
    let zoom = 40.0;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1600,
        300,
    );
    let img = render(&mut s, (1600, 300), &[rect], zoom);
    let x = (800.0 + 3.0 * bar * zoom * SCALE).round() as u32;
    let b = style.deck[1];
    let ghost_at = |img: &Image| {
        (x - 2..x + 2).any(|xx| {
            let p = img.pixel(xx, 32 + 20 * 5 + 3);
            (f32::from(p[0]) - b[0] * 255.0).abs() < 30.0
                && (f32::from(p[2]) - b[2] * 255.0).abs() < 30.0
        })
    };
    assert!(
        ghost_at(&img),
        "deck B coloured ghost line 3 bars ahead of the playhead"
    );
    s.decks[0].master = false;
    assert!(
        !ghost_at(&render(&mut s, (1600, 300), &[rect], zoom)),
        "no ghost without a master deck"
    );
}

#[test]
fn a_fill_in_is_marked_in_the_band_before_the_next_phrase() {
    let style = Style::default();
    let Some((mut s, a)) = odd_scene(0.0) else {
        return;
    };
    let bar = 60.0 / a.bpm * 4.0;
    s.r.renderer.set_waveform(0, 1, None);
    s.decks[0].pos_secs = a.drop_start - 0.5 * bar;
    let zoom = 90.0;
    let rect = place(
        SurfaceId {
            kind: SurfaceKind::Scroll,
            deck: 0,
        },
        0,
        0,
        1600,
        300,
    );
    let img = render(&mut s, (1600, 300), &[rect], zoom);
    let pps = zoom * SCALE;
    let x_of = |t: f64| (800.0 + (t - s.decks[0].pos_secs) * pps).round() as u32;
    let fill_start = a.phrases[1].fill_start_secs.expect("the build has a fill");
    let notch = img.pixel(x_of(fill_start) - 2, 4);
    assert!(
        luma(notch) > 230.0,
        "white notch at the fill start: {notch:?}"
    );
    let drop = style.phrases[2];
    let inside = img.pixel(x_of(fill_start + 0.6 * bar), 2);
    assert!(
        f32::from(inside[0]) > drop[0] * 255.0 * 0.6 && inside[1] < inside[0],
        "the fill segment is tinted with the next phrase's colour: {inside:?}"
    );
}
