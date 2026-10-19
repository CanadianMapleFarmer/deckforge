<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { history } from '$lib/state/history.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import * as be from '$lib/api/backend';
  import type { MarkerFormats, RecordingSettings } from '$lib/api';
  import FolderOpen from '@lucide/svelte/icons/folder-open';

  const MARKERS: { key: keyof MarkerFormats; label: string; hint: string }[] = [
    { key: 'csv', label: 'Spreadsheet (.csv)', hint: 'Time, artist, title, blend start and length, for any tool.' },
    { key: 'cue', label: 'CUE sheet (.cue)', hint: 'Track markers that audio players and SoundCloud-style uploaders read.' },
    { key: 'fcpXml', label: 'Video editor markers (.xml)', hint: 'Timeline markers DaVinci Resolve and Premiere Pro both import.' },
  ];

  $effect(() => {
    if (ui.settingsOpen && ui.settingsTab === 'recording') void history.loadSettings();
  });

  const s = $derived(history.settings);
  const perHour = $derived(s ? (s.sampleRate * 2 * (s.bits / 8) * 3600 * (s.format === 'flac' ? 0.6 : 1)) / 1e9 : 0);

  function update(patch: Partial<RecordingSettings>) {
    if (s) void history.saveSettings({ ...s, ...patch });
  }
  async function pickFolder() {
    if (!s || !be.inTauri) return;
    const picked = await open({ directory: true, multiple: false, defaultPath: s.folder, title: 'Choose where recordings go' }).catch(() => null);
    if (typeof picked === 'string') update({ folder: picked });
  }
</script>

<div class="wrap">
  {#if s}
    <section>
      <h3>Where recordings go</h3>
      <div class="folder">
        <span class="path" title={s.folder}><span dir="ltr">{s.folder}</span></span>
        <button class="btn" onclick={pickFolder}><FolderOpen size={14} strokeWidth={2.1} />Change</button>
      </div>
      <p class="muted">Files are named by date and time, like <span class="num">2026-10-19 21.05.wav</span>. The tracklist is written next to each one when you stop.</p>
    </section>

    <section>
      <h3>Format</h3>
      <div class="grid3">
        <div>
          <span class="lbl">File type</span>
          <div class="seg wide" role="group" aria-label="File type">
            <button class:on={s.format === 'wav'} onclick={() => update({ format: 'wav' })}>WAV</button>
            <button class:on={s.format === 'flac'} onclick={() => update({ format: 'flac' })}>FLAC</button>
          </div>
        </div>
        <div>
          <span class="lbl">Bit depth</span>
          <div class="seg wide" role="group" aria-label="Bit depth">
            <button class:on={s.bits === 16} onclick={() => update({ bits: 16 })}>16-bit</button>
            <button class:on={s.bits === 24} onclick={() => update({ bits: 24 })}>24-bit</button>
          </div>
        </div>
        <div>
          <span class="lbl">Sample rate</span>
          <div class="seg wide" role="group" aria-label="Sample rate">
            <button class:on={s.sampleRate === 44100} onclick={() => update({ sampleRate: 44100 })}>44.1 kHz</button>
            <button class:on={s.sampleRate === 48000} onclick={() => update({ sampleRate: 48000 })}>48 kHz</button>
          </div>
        </div>
      </div>
      <p class="muted">About <span class="num">{perHour.toFixed(1)} GB</span> per hour{s.format === 'flac' ? ', lossless but smaller than WAV' : ''}. The recording is taken before the master volume and limited like the master, so it never clips.</p>
    </section>

    <section>
      <h3>Markers beside the recording</h3>
      <p class="muted nomargin">The plain tracklist (.txt) is always written. Turn on the formats your editor or uploader reads.</p>
      {#each MARKERS as m (m.key)}
        <label class="toggle">
          <input type="checkbox" checked={s.markers[m.key]} onchange={(e) => update({ markers: { ...s.markers, [m.key]: e.currentTarget.checked } })} />
          <span class="sw"></span>
          <span><b>{m.label}</b><small>{m.hint}</small></span>
        </label>
      {/each}
    </section>
  {:else}
    <p class="muted">Loading recording settings…</p>
  {/if}
</div>

<style>
  .wrap {
    padding: 14px 20px 30px;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 720;
    color: var(--dim);
    letter-spacing: 0.01em;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .grid3 {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }
  .lbl {
    display: block;
    margin-bottom: 5px;
    font-size: 11.5px;
    color: var(--dim);
    font-weight: 600;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 11px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.02);
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    font-size: 12.5px;
    color: var(--text);
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--line-2);
    border-radius: 8px;
    overflow: hidden;
  }
  .seg.wide {
    display: flex;
  }
  .seg button {
    flex: 1;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-right: 1px solid var(--line-2);
    background: var(--raised);
    color: var(--dim);
    font-size: 12px;
    font-weight: 620;
  }
  .seg button:last-child {
    border-right: 0;
  }
  .seg button.on {
    background: rgba(250, 250, 250, 0.12);
    color: var(--fg);
  }
  .muted {
    margin: 4px 0 0;
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--faint);
  }
  .nomargin {
    margin: -4px 0 4px;
  }
  .toggle {
    display: flex;
    gap: 11px;
    align-items: flex-start;
    cursor: pointer;
  }
  .toggle input {
    position: absolute;
    opacity: 0;
  }
  .sw {
    flex: none;
    width: 34px;
    height: 20px;
    margin-top: 1px;
    border-radius: 11px;
    background: var(--raised-3);
    position: relative;
    transition: background 120ms;
  }
  .sw::after {
    content: '';
    position: absolute;
    left: 2px;
    top: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--dim);
    transition: transform 140ms var(--ease), background 120ms;
  }
  .toggle input:checked + .sw {
    background: var(--ok);
  }
  .toggle input:checked + .sw::after {
    transform: translateX(14px);
    background: #fff;
  }
  .toggle input:focus-visible + .sw {
    outline: 2px solid var(--ring);
  }
  .toggle b {
    display: block;
    font-size: 12.5px;
    font-weight: 620;
  }
  .toggle small {
    display: block;
    margin-top: 2px;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--faint);
  }
</style>
