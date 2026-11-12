<script lang="ts">
  import { engine, cmd } from '$lib/state/engine.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import { toast } from '$lib/state/toast.svelte';
  import * as be from '$lib/api/backend';
  import { errorMessage } from '$lib/util/format';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import type { CrossfaderCurve, EqCurve, OutputDeviceInfo, RoutingMode, SecondarySource } from '$lib/api';

  let devices = $state<OutputDeviceInfo[]>([]);
  let deviceId = $state<string | null>(null);
  let sampleRate = $state<number | null>(null);
  let buffer = $state<number>(256);
  let mode = $state<RoutingMode>('master-and-phones');
  let masterPair = $state(0);
  let phonesPair = $state(1);
  let applying = $state(false);
  let loaded = false;

  const UNITY_LEVEL = 0.8;
  let secDeviceId = $state<string | null>(null);
  let secSource = $state<SecondarySource>('master');
  let secPair = $state(0);
  let secLevel = $state(UNITY_LEVEL);
  let secBusy = $state(false);
  let levelTimer: ReturnType<typeof setTimeout> | undefined;

  const BUFFERS = [128, 256, 512, 1024];
  const MODES: { id: RoutingMode; label: string; hint: string }[] = [
    { id: 'master-and-phones', label: 'Master + phones', hint: 'Master on one pair, headphone cue on another. Needs a 4-channel output.' },
    { id: 'master-only', label: 'Master only', hint: 'Master out only. Headphone cue is unavailable.' },
    { id: 'split-mono', label: 'Split mono', hint: 'Master on the left channel, cue on the right channel of one pair — for a splitter cable on a stereo output.' },
  ];
  const CURVES: { id: CrossfaderCurve; label: string }[] = [
    { id: 'linear', label: 'Linear' },
    { id: 'equal-power', label: 'Equal power' },
    { id: 'sharp', label: 'Sharp cut' },
    { id: 'off', label: 'Off' },
  ];
  const EQ_CURVES: { id: EqCurve; label: string; title: string }[] = [
    { id: 'eq', label: 'EQ', title: 'DJM-900NXS2 EQ: gentle near the centre, -26 dB fully off' },
    { id: 'isolator', label: 'Isolator', title: 'Isolator: each band can be killed completely' },
  ];

  const device = $derived(devices.find((d) => d.id === deviceId) ?? null);
  const pairs = $derived(Array.from({ length: Math.max(1, Math.floor((device?.maxChannels ?? 2) / 2)) }, (_, i) => i));
  const effRate = $derived(sampleRate ?? device?.sampleRates.find((r) => r === 48000) ?? device?.sampleRates[0] ?? 48000);
  const latency = $derived((buffer / effRate) * 1000);
  const audio = $derived(engine.audio);
  const master = $derived(engine.snap.master);
  const secondary = $derived(audio?.secondary ?? null);
  const secOn = $derived(secondary !== null);
  const secCandidates = $derived(devices.filter((d) => d.id !== audio?.active?.deviceId));
  const secDevice = $derived(devices.find((d) => d.id === secDeviceId) ?? null);
  const secPairs = $derived(Array.from({ length: Math.max(1, Math.floor((secDevice?.maxChannels ?? 2) / 2)) }, (_, i) => i));
  const outLatency = $derived(audio && audio.active ? (audio.lastCallbackFrames / audio.active.sampleRate) * 1000 : null);

  $effect(() => {
    if (!ui.settingsOpen || ui.settingsTab !== 'audio') {
      loaded = false;
      return;
    }
    if (loaded) return;
    loaded = true;
    be.backend.audioDevices().then((list) => {
      devices = list;
      const a = engine.audio?.active;
      deviceId = a?.deviceId ?? list.find((d) => d.isDefault)?.id ?? list[0]?.id ?? null;
      if (a) {
        sampleRate = a.sampleRate;
        buffer = a.requestedBuffer ?? 256;
        mode = a.routing.mode;
        masterPair = Math.floor(a.routing.master[0] / 2);
        phonesPair = Math.floor(a.routing.phones[0] / 2);
      }
      const sec = engine.audio?.secondary;
      if (sec) {
        secDeviceId = sec.deviceId;
        secSource = sec.source;
        secPair = Math.floor(sec.channels[0] / 2);
        secLevel = sec.level;
      } else {
        secDeviceId = list.find((d) => d.isDefault && d.id !== deviceId)?.id ?? list.find((d) => d.id !== deviceId)?.id ?? null;
      }
    }).catch((e) => toast.error(`Could not list audio devices: ${errorMessage(e)}`));
  });

  async function apply() {
    if (!device) return;
    applying = true;
    try {
      const active = await be.backend.audioOpen({
        deviceId: device.id,
        sampleRate: sampleRate,
        bufferFrames: buffer,
        routing: { mode, master: [masterPair * 2, masterPair * 2 + 1], phones: mode === 'split-mono' ? [masterPair * 2, masterPair * 2 + 1] : [phonesPair * 2, phonesPair * 2 + 1] },
      });
      toast.ok(`Output opened: ${active.deviceName} · ${active.sampleRate} Hz`);
    } catch (e) {
      toast.error(`Could not open output: ${errorMessage(e)}`);
    } finally {
      applying = false;
    }
  }

  function levelDb(level: number): string {
    if (level <= 0) return '−∞ dB';
    const db = level >= UNITY_LEVEL ? ((level - UNITY_LEVEL) / 0.2) * 3 : 40 * Math.log10(level / UNITY_LEVEL);
    return Math.abs(db) < 0.05 ? 'Unity' : `${db > 0 ? '+' : '−'}${Math.abs(db).toFixed(1)} dB`;
  }

  async function applySecondary(enabled: boolean) {
    if (secBusy) return;
    if (enabled && !secDevice) return;
    secBusy = true;
    try {
      await be.backend.audioSetSecondary(
        enabled && secDevice
          ? { deviceId: secDevice.id, source: secSource, level: secLevel, channels: [secPair * 2, secPair * 2 + 1] }
          : null,
      );
    } catch (e) {
      toast.error(`Additional output: ${errorMessage(e)}`);
    } finally {
      secBusy = false;
    }
  }

  function onSecDevice(id: string) {
    secDeviceId = id;
    secPair = 0;
    if (secOn) applySecondary(true);
  }

  function onSecLevel(value: number) {
    secLevel = value;
    if (!secOn) return;
    clearTimeout(levelTimer);
    levelTimer = setTimeout(() => be.backend.audioSetSecondaryLevel(secLevel).catch(() => {}), 30);
  }
</script>

<div class="wrap">
  <section>
    <h3>Output device</h3>
    <div class="devices" role="radiogroup" aria-label="Output device">
      {#each devices as d (d.id)}
        <button class="dev" class:sel={d.id === deviceId} role="radio" aria-checked={d.id === deviceId} onclick={() => { deviceId = d.id; sampleRate = null; masterPair = 0; phonesPair = Math.min(1, Math.floor(d.maxChannels / 2) - 1); }}>
          <span class="radio"></span>
          <span class="dn">{d.name}{#if d.isDefault}<em>default</em>{/if}</span>
          <span class="dm num">{d.maxChannels} ch</span>
        </button>
      {:else}
        <p class="muted">No output devices found.</p>
      {/each}
    </div>
  </section>

  <section class="grid2">
    <label>
      <span class="lbl">Sample rate</span>
      <select bind:value={sampleRate}>
        <option value={null}>Device default</option>
        {#each device?.sampleRates ?? [] as r (r)}<option value={r}>{(r / 1000).toFixed(r % 1000 ? 1 : 0)} kHz</option>{/each}
      </select>
    </label>
    <div>
      <span class="lbl">Buffer size</span>
      <div class="seg" role="group" aria-label="Buffer size">
        {#each BUFFERS as b (b)}<button class:on={buffer === b} onclick={() => (buffer = b)}>{b}</button>{/each}
      </div>
      <p class="muted num">≈ {latency.toFixed(1)} ms per buffer at {(effRate / 1000).toFixed(effRate % 1000 ? 1 : 0)} kHz</p>
    </div>
  </section>

  <section>
    <h3>Routing</h3>
    <div class="seg wide" role="group" aria-label="Routing mode">
      {#each MODES as m (m.id)}<button class:on={mode === m.id} onclick={() => (mode = m.id)}>{m.label}</button>{/each}
    </div>
    <p class="muted">{MODES.find((m) => m.id === mode)?.hint}</p>
    {#if mode !== 'master-only'}
      <div class="grid2">
        <label>
          <span class="lbl">{mode === 'split-mono' ? 'Output pair' : 'Master pair'}</span>
          <select bind:value={masterPair}>
            {#each pairs as p (p)}<option value={p}>Channels {p * 2 + 1}–{p * 2 + 2}</option>{/each}
          </select>
        </label>
        {#if mode === 'master-and-phones'}
          <label>
            <span class="lbl">Headphone pair</span>
            <select bind:value={phonesPair} disabled={pairs.length < 2}>
              {#each pairs as p (p)}<option value={p} disabled={p === masterPair}>Channels {p * 2 + 1}–{p * 2 + 2}</option>{/each}
            </select>
          </label>
        {/if}
      </div>
      {#if mode === 'master-and-phones' && pairs.length < 2}
        <p class="warn">This device only has one stereo pair, so headphone cue can't use separate outputs. Choose “Split mono” or pick a 4-channel device.</p>
      {/if}
    {/if}
    <button class="btn btn-primary apply" onclick={apply} disabled={applying || !device}>
      {#if applying}<LoaderCircle size={14} class="spin" />Applying…{:else}Apply audio settings{/if}
    </button>
  </section>

  <section>
    <h3>Additional output</h3>
    <p class="muted nomargin">Plays the master or the cue on a second device, such as the laptop speakers, alongside the controller output.</p>
    <label class="toggle">
      <input type="checkbox" checked={secOn} disabled={secBusy || (!secOn && !secDevice)} onchange={(e) => applySecondary(e.currentTarget.checked)} />
      <span class="sw"></span>
      <span>
        <b>Play on a second device</b>
        <small>Starts silent and fades in. It runs a few tens of milliseconds behind the main output.</small>
      </span>
    </label>
    <div class="grid2">
      <label>
        <span class="lbl">Device</span>
        <select value={secDeviceId ?? ''} disabled={secCandidates.length === 0} onchange={(e) => onSecDevice(e.currentTarget.value)}>
          {#if secCandidates.length === 0}<option value="">No other output devices</option>{/if}
          {#each secCandidates as d (d.id)}<option value={d.id}>{d.name} · {d.maxChannels} ch</option>{/each}
        </select>
      </label>
      <div>
        <span class="lbl">Source</span>
        <div class="seg wide" role="group" aria-label="Additional output source">
          {#each ['master', 'cue'] as const as src (src)}
            <button class:on={secSource === src} onclick={() => { secSource = src; if (secOn) applySecondary(true); }}>{src === 'master' ? 'Master' : 'Cue'}</button>
          {/each}
        </div>
      </div>
    </div>
    <div class="grid2">
      {#if secPairs.length > 1}
        <label>
          <span class="lbl">Channel pair</span>
          <select value={secPair} onchange={(e) => { secPair = +e.currentTarget.value; if (secOn) applySecondary(true); }}>
            {#each secPairs as p (p)}<option value={p}>Channels {p * 2 + 1}–{p * 2 + 2}</option>{/each}
          </select>
        </label>
      {/if}
      <label class:span2={secPairs.length <= 1}>
        <span class="lbl level">Level <b class="num">{levelDb(secLevel)}</b></span>
        <input class="range" type="range" min="0" max="1" step="0.01" value={secLevel} aria-label="Additional output level" oninput={(e) => onSecLevel(+e.currentTarget.value)} />
      </label>
    </div>
    {#if secondary}
      <div class="secline" data-s={secondary.state}>
        <i class="led" data-state={secondary.state === 'running' ? 'ok' : 'bad'}></i>
        {#if secondary.state === 'running'}
          <b>Running</b>
          <span class="num">{secondary.latencyMs.toFixed(0)} ms behind</span>
          <span class="num">{secondary.driftPpm >= 0 ? '+' : '−'}{Math.abs(secondary.driftPpm).toFixed(0)} ppm drift</span>
          <span class="num" class:bad={secondary.underruns > 0}>{secondary.underruns} underruns</span>
          <span class="num">{(secondary.sampleRate / 1000).toFixed(secondary.sampleRate % 1000 ? 1 : 0)} kHz</span>
        {:else}
          <b>Lost</b>
          <span>Device unavailable. It reconnects automatically when it returns.</span>
        {/if}
      </div>
    {/if}
  </section>

  <section>
    <h3>Mixer</h3>
    <label class="toggle">
      <input type="checkbox" checked={master.hardwarePhones} onchange={(e) => cmd({ type: 'hardwarePhones', enabled: e.currentTarget.checked })} />
      <span class="sw"></span>
      <span>
        <b>Headphone mix handled by controller hardware</b>
        <small>The FLX4 headphone MIX and LEVEL knobs control the cue. Turn this off to use the on-screen knobs.</small>
      </span>
    </label>
    <div>
      <span class="lbl">Crossfader curve</span>
      <div class="seg wide" role="group" aria-label="Crossfader curve">
        {#each CURVES as c (c.id)}<button class:on={master.curve === c.id} onclick={() => cmd({ type: 'crossfaderCurve', curve: c.id })}>{c.label}</button>{/each}
      </div>
    </div>
    <div>
      <span class="lbl">EQ curve</span>
      <div class="seg wide" role="group" aria-label="EQ curve">
        {#each EQ_CURVES as c (c.id)}<button class:on={master.eqCurve === c.id} title={c.title} onclick={() => cmd({ type: 'eqCurve', curve: c.id })}>{c.label}</button>{/each}
      </div>
    </div>
  </section>

  <section>
    <h3>Output status</h3>
    <div class="status">
      {#if audio}
        <div class="row"><span>State</span><b class="st" data-s={audio.state}><i class="led" data-state={audio.state === 'running' ? 'ok' : audio.state === 'lost' ? 'bad' : 'warn'}></i>{audio.state}</b></div>
        {#if audio.active}
          <div class="row"><span>Device</span><b>{audio.active.deviceName}</b></div>
          <div class="row"><span>Format</span><b class="num">{audio.active.sampleRate} Hz · {audio.active.channels} ch · {audio.active.sampleFormat}</b></div>
          <div class="row"><span>Routing</span><b>{audio.active.routing.mode} · master {audio.active.routing.master.map((c) => c + 1).join('/')} · phones {audio.active.routing.phones.map((c) => c + 1).join('/')}</b></div>
          <div class="row"><span>Buffer</span><b class="num">{audio.active.requestedBuffer ?? 'default'} requested · {audio.lastCallbackFrames} frames/callback{#if outLatency !== null} · {outLatency.toFixed(1)} ms{/if}</b></div>
        {/if}
        <div class="row"><span>Callbacks</span><b class="num">{audio.callbacks.toLocaleString()}</b></div>
        <div class="row"><span>Underruns / overruns</span><b class="num" class:bad={audio.xruns > 0}>{audio.xruns}</b></div>
        <div class="row"><span>Stream errors</span><b class="num" class:bad={audio.streamErrors > 0}>{audio.streamErrors}</b></div>
        {#if audio.lastError}<div class="row err"><span>Last error</span><b>{audio.lastError}</b></div>{/if}
      {:else}
        <p class="muted">Waiting for the audio engine…</p>
      {/if}
    </div>
  </section>
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
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  .lbl {
    display: block;
    margin-bottom: 5px;
    font-size: 11.5px;
    color: var(--dim);
    font-weight: 600;
  }
  select {
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    background: #09090b;
    color: var(--fg);
    outline: none;
  }
  select:focus {
    border-color: var(--ring);
  }
  .devices {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .dev {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 11px;
    border-radius: 9px;
    border: 1px solid var(--line);
    background: rgba(255, 255, 255, 0.02);
    text-align: left;
    transition: border-color 120ms, background 120ms;
  }
  .dev:hover {
    border-color: var(--line-2);
  }
  .dev.sel {
    border-color: var(--dim);
    background: rgba(255, 255, 255, 0.05);
  }
  .radio {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid #52525b;
    flex: none;
  }
  .sel .radio {
    border-color: var(--fg);
    background: radial-gradient(var(--fg) 40%, transparent 45%);
  }
  .dn {
    flex: 1;
    font-weight: 560;
  }
  .dn em {
    margin-left: 8px;
    font-style: normal;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--raised-2);
    color: var(--dim);
  }
  .dm {
    font-size: 11.5px;
    color: var(--faint);
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
    padding: 0 12px;
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
  .nomargin {
    margin: 0;
  }
  .span2 {
    grid-column: 1 / -1;
  }
  .lbl.level {
    display: flex;
    justify-content: space-between;
  }
  .lbl.level b {
    font-weight: 600;
    color: var(--text);
  }
  .range {
    width: 100%;
    height: 30px;
    margin: 0;
    accent-color: var(--fg);
  }
  .secline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 14px;
    padding: 8px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
    font-size: 12px;
    color: var(--dim);
  }
  .secline b {
    display: inline-flex;
    align-items: center;
    font-weight: 620;
    color: var(--text);
  }
  .secline .led {
    margin-right: -8px;
  }
  .secline[data-s='lost'] {
    border-color: color-mix(in srgb, var(--danger) 35%, var(--line));
  }
  .muted {
    margin: 4px 0 0;
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--faint);
  }
  .warn {
    margin: 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--warn) 9%, transparent);
    border: 1px solid color-mix(in srgb, var(--warn) 35%, var(--line));
    color: #fcd34d;
    font-size: 12px;
    line-height: 1.45;
  }
  .apply {
    align-self: flex-start;
    height: 32px;
    margin-top: 4px;
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
  .status {
    border: 1px solid var(--line);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
    padding: 4px 12px;
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 7px 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.045);
    font-size: 12px;
  }
  .row:last-child {
    border-bottom: 0;
  }
  .row span {
    color: var(--faint);
    flex: none;
  }
  .row b {
    font-weight: 560;
    text-align: right;
    word-break: break-word;
  }
  .st {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    text-transform: capitalize;
  }
  .bad {
    color: var(--danger);
  }
  .err b {
    color: var(--danger);
  }
</style>
