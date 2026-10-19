<script lang="ts">
  import { engine } from '$lib/state/engine.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import BeatFx from './BeatFx.svelte';
  import RecordButton from './RecordButton.svelte';
  import Settings from '@lucide/svelte/icons/settings';
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import Headphones from '@lucide/svelte/icons/headphones';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Cpu from '@lucide/svelte/icons/cpu';
  import Usb from '@lucide/svelte/icons/usb';
  import Volume2 from '@lucide/svelte/icons/volume-2';

  const audio = $derived(engine.audio);
  const active = $derived(audio?.active ?? null);
  const state = $derived<'ok' | 'warn' | 'bad'>(!audio ? 'warn' : audio.state === 'running' ? 'ok' : audio.state === 'lost' ? 'bad' : 'warn');
  const stateText = $derived(!audio ? 'Starting audio…' : audio.state === 'running' ? 'Running' : audio.state === 'lost' ? 'Device lost' : 'Silent');
  const secondary = $derived(audio?.secondary ?? null);
  const secondaryName = $derived(
    secondary ? secondary.deviceName.replace(/\s*\(.*?\)\s*/g, ' ').replace(/\s+Speakers?$/i, '').trim() || secondary.deviceName : '',
  );
  const secondaryTitle = $derived(
    secondary
      ? secondary.state === 'running'
        ? `Additional output: ${secondary.deviceName} (${secondary.source}), ${secondary.latencyMs.toFixed(0)} ms behind. Click for audio settings`
        : `Additional output: ${secondary.deviceName} is not available. It reconnects automatically`
      : '',
  );
  const cueOk = $derived(engine.snap.cueAvailable);
  const ctl = $derived(engine.controller);
  const load = $derived(Math.round(engine.snap.callbackLoad * 100));
  const loadClass = $derived(load > 85 ? 'bad' : load > 60 ? 'warn' : 'ok');
  const clip = $derived(engine.clipLatch);
  const masterIndex = $derived(engine.snap.masterDeck);
  const master = $derived(masterIndex === null ? null : engine.snap.decks[masterIndex]);
</script>

<header class="bar" data-tauri-drag-region>
  <div class="left" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <svg viewBox="0 0 32 32" width="22" height="22" aria-hidden="true">
        <path d="M3 4h10a7 7 0 0 1 0 14H3z" fill="var(--deck-a)" />
        <path d="M29 28H19a7 7 0 0 1 0-14h10z" fill="var(--deck-b)" />
      </svg>
      <span class="word" data-tauri-drag-region>deckforge</span>
    </div>
    {#if engine.preview}
      <span class="preview" title="Browser preview with simulated data. Run inside Tauri for the real engine.">Preview</span>
    {/if}
    <div class="master" title="Master tempo deck" data-tauri-drag-region>
      <span class="lbl">Master</span>
      {#if master && masterIndex !== null && master.loaded}
        <b class="mdeck" data-deck={masterIndex}>{masterIndex === 0 ? 'A' : 'B'}</b>
        <span class="num bpm">{master.bpm > 0 ? master.bpm.toFixed(1) : '—'}</span><small>BPM</small>
      {:else}
        <span class="num dim">—</span>
      {/if}
    </div>
    <RecordButton />
  </div>

  <BeatFx />

  <div class="right" data-tauri-drag-region>
    <button class="chip" onclick={() => ui.openSettings('audio')} title={active ? `${active.deviceName} — audio output settings` : 'Audio output settings'}>
      <i class="led" data-state={state}></i>
      <span class="dev">{active?.deviceName ?? 'No output device'}</span>
      <span class="meta num">{#if active}{(active.sampleRate / 1000).toFixed(active.sampleRate % 1000 ? 1 : 0)}k · {active.requestedBuffer ?? audio?.lastCallbackFrames ?? '—'}{:else}{stateText}{/if}</span>
    </button>

    {#if secondary}
      <button class="chip secondary" class:lost={secondary.state === 'lost'} onclick={() => ui.openSettings('audio')} title={secondaryTitle} aria-label="Additional audio output">
        <Volume2 size={13} strokeWidth={2.2} />
        <span class="sname">{secondaryName}</span>
        <i class="led" data-state={secondary.state === 'running' ? 'ok' : 'bad'}></i>
      </button>
    {/if}

    {#if cueOk}
      <button class="chip ok" onclick={() => ui.openSettings('audio')} title="Headphone cue is routed to a separate output pair" aria-label="Headphone cue ready">
        <Headphones size={13} strokeWidth={2.2} /><span class="long">Cue</span>
      </button>
    {:else}
      <button class="chip warn" onclick={() => ui.openSettings('audio')} title="The selected output has no separate headphone pair. Open audio settings to choose one." aria-label="Headphone cue unavailable">
        <TriangleAlert size={13} strokeWidth={2.2} /><span class="long">No cue</span>
      </button>
    {/if}

    <button class="chip" onclick={() => ui.openSettings('controller')} title="Controller and MIDI diagnostics">
      <Usb size={13} strokeWidth={2.2} />
      <i class="led" data-state={ctl?.connected ? 'ok' : 'bad'}></i>
      <span class="long">{ctl?.connected ? 'FLX4' : 'No FLX4'}</span>
    </button>

    <div class="cpu" title="Audio callback load (peak)">
      <Cpu size={13} strokeWidth={2.2} />
      <div class="track"><i class={loadClass} style="width:{Math.min(100, load)}%"></i></div>
      <span class="num">{load}%</span>
    </div>

    <div class="clipled" class:on={clip} title="Master clip indicator"><i></i><span>Clip</span></div>

    <div class="icons">
      <button class="btn btn-ghost icon" onclick={() => (ui.helpOpen = true)} aria-label="Keyboard shortcuts" title="Keyboard shortcuts (?)"><Keyboard size={16} strokeWidth={1.9} /></button>
      <button class="btn btn-ghost icon" onclick={() => ui.openSettings('audio')} aria-label="Settings" title="Settings (⌘,)"><Settings size={16} strokeWidth={1.9} /></button>
    </div>
  </div>
</header>

<style>
  .bar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: 14px;
    height: 56px;
    padding: 0 12px 0 84px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    flex: none;
  }
  @media (max-height: 820px) {
    .bar {
      height: 48px;
    }
  }
  .left,
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .right {
    justify-content: flex-end;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-right: 4px;
  }
  .word {
    font-weight: 700;
    font-size: 14px;
    letter-spacing: -0.02em;
    color: var(--fg);
  }
  .preview {
    padding: 2px 7px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--warn);
    border: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    white-space: nowrap;
  }
  .master {
    display: flex;
    align-items: baseline;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    align-items: center;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--raised);
    white-space: nowrap;
  }
  .master .lbl {
    font-size: 11px;
    color: var(--faint);
    font-weight: 560;
  }
  .master small {
    font-size: 9.5px;
    color: var(--faint);
    font-weight: 600;
    margin-left: -3px;
  }
  .mdeck {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    display: grid;
    place-items: center;
    font-size: 10px;
    font-weight: 750;
    color: #09090b;
    background: var(--deck);
  }
  .bpm {
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
  }
  .dim {
    color: var(--faint);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 9px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--dim);
    font-size: 11.5px;
    white-space: nowrap;
    min-width: 0;
    transition:
      border-color var(--dur),
      background var(--dur);
  }
  .chip:hover {
    border-color: var(--line-2);
    background: var(--raised-2);
  }
  .dev {
    color: var(--text);
    font-weight: 540;
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    color: var(--faint);
    font-size: 11px;
  }
  .chip.ok {
    color: #86efac;
  }
  .chip.secondary {
    gap: 5px;
    padding: 0 8px;
  }
  .chip.secondary.lost {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 35%, var(--line));
  }
  .sname {
    max-width: 96px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
    font-weight: 540;
  }
  .chip.secondary.lost .sname {
    color: inherit;
  }
  .chip.warn {
    color: #fcd34d;
    border-color: color-mix(in srgb, var(--warn) 35%, var(--line));
  }
  @media (max-width: 1640px) {
    .long,
    .meta,
    .preview,
    .master .lbl {
      display: none;
    }
    .dev {
      max-width: 90px;
    }
  }
  @media (max-width: 1440px) {
    .word,
    .master small,
    .dev,
    .sname,
    .preview,
    .cpu .track,
    .clipled span {
      display: none;
    }
    .cpu {
      width: auto;
    }
    .master .lbl {
      display: none;
    }
    .right,
    .left {
      gap: 6px;
    }
  }
  .cpu {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--faint);
    font-size: 11px;
    width: 86px;
  }
  .cpu .track {
    flex: 1;
    height: 4px;
    background: var(--raised-3);
    border-radius: 2px;
    overflow: hidden;
  }
  .cpu i {
    display: block;
    height: 100%;
    border-radius: 2px;
    transition: width 200ms linear;
  }
  .cpu i.ok {
    background: var(--ok);
  }
  .cpu i.warn {
    background: var(--warn);
  }
  .cpu i.bad {
    background: var(--danger);
  }
  .cpu .num {
    width: 28px;
    text-align: right;
    color: var(--dim);
  }
  .clipled {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    font-weight: 560;
    color: var(--faint);
  }
  .clipled i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--raised-3);
  }
  .clipled.on {
    color: var(--danger);
  }
  .clipled.on i {
    background: var(--danger);
  }
  .icons {
    display: flex;
    gap: 2px;
  }
  .icon {
    width: 30px;
    padding: 0;
    color: var(--dim);
  }
  .icon:hover {
    color: var(--fg);
  }
</style>
