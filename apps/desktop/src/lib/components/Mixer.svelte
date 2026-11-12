<script lang="ts">
  import { engine, cmd } from '$lib/state/engine.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import Knob from './Knob.svelte';
  import Fader from './Fader.svelte';
  import Meter from './Meter.svelte';
  import Select from './Select.svelte';
  import Headphones from '@lucide/svelte/icons/headphones';
  import { COLOR_FX_PARAMS, SMART_CFX_PRESETS, type BandName, type ColorFxKind, type CrossfaderCurve, type DeckIndex, type SmartCfxPreset } from '$lib/api';
  import { SMART_CFX_LABELS, SMART_CFX_SIDES } from '$lib/fx/labels';
  import { fxControl } from '$lib/state/fxcontrol.svelte';

  const channels = $derived(engine.snap.channels);
  const master = $derived(engine.snap.master);
  const colorFx = $derived(engine.snap.colorFx);
  const colorParam = $derived(COLOR_FX_PARAMS[colorFx][0]);

  const eqDb = (v: number) => (v <= 0.004 ? 'KILL' : `${v >= 0.5 ? '+' : ''}${(v >= 0.5 ? (v - 0.5) * 24 : 20 * Math.log10(v * 2)).toFixed(1)} dB`);
  const trimDb = (v: number) => `${v >= 0.5 ? '+' : ''}${((v - 0.5) * 24).toFixed(1)} dB`;
  const cfxTxt = (v: number) => (Math.abs(v) < 0.02 ? 'OFF' : `${v < 0 ? '◂' : '▸'} ${Math.round(Math.abs(v) * 100)}%`);
  const pct = (v: number) => `${Math.round(v * 100)}%`;

  const BANDS: { key: 'eqHigh' | 'eqMid' | 'eqLow'; band: BandName; label: string }[] = [
    { key: 'eqHigh', band: 'High', label: 'HI' },
    { key: 'eqMid', band: 'Mid', label: 'MID' },
    { key: 'eqLow', band: 'Low', label: 'LOW' },
  ];
  const CURVES: { id: CrossfaderCurve; label: string; title: string }[] = [
    { id: 'linear', label: 'Lin', title: 'Linear blend' },
    { id: 'equal-power', label: 'Pwr', title: 'Equal-power blend' },
    { id: 'sharp', label: 'Cut', title: 'Sharp cut (scratch curve)' },
    { id: 'off', label: 'Off', title: 'Crossfader off: both decks play through wherever it sits' },
  ];
  const COLOR_FX: { value: ColorFxKind; label: string }[] = [
    { value: 'filter', label: 'Filter' },
    { value: 'space', label: 'Space' },
    { value: 'dubEcho', label: 'Dub Echo' },
    { value: 'crush', label: 'Crush' },
    { value: 'noise', label: 'Noise' },
  ];
  const SMART_SHORT: Record<SmartCfxPreset, string> = { echoWash: 'Wash', riser: 'Riser', space: 'Space', crushChop: 'Crush' };
  const SMART_PRESETS: { value: SmartCfxPreset; label: string }[] = SMART_CFX_PRESETS.map((p) => ({ value: p, label: SMART_SHORT[p] }));
  const smart = $derived(engine.snap.smartCfx);
  const sides = $derived(SMART_CFX_SIDES[smart.preset]);
  const macroTxt = (v: number) => (Math.abs(v) < 0.02 ? 'Centre' : `${v < 0 ? sides[0] : sides[1]} ${Math.round(Math.abs(v) * 100)}%`);
  const decks = [0, 1] as const;
  const knob = $derived(ui.compact ? 22 : 26);
</script>

<section class="mixer">
  <div class="chs">
    {#each decks as i (i)}
      {@const c = channels[i]}
      <div class="ch" data-deck={i} class:rev={i === 1}>
        <div class="knobs">
          <div class="kr"><Knob value={c.trim} bipolar reset={0.5} size={knob} label="Trim" format={trimDb} onchange={(v) => cmd({ type: 'trim', channel: i as DeckIndex, value: v })} /><span>TRIM</span></div>
          {#each BANDS as b (b.key)}
            <div class="kr"><Knob value={c[b.key]} bipolar reset={0.5} size={knob} kill label={b.label} format={eqDb} onchange={(v) => cmd({ type: 'eq', channel: i as DeckIndex, band: b.band, value: v })} /><span>{b.label}</span></div>
          {/each}
          <div class="kr cfx" class:smart={smart.enabled}><Knob value={c.filter} min={-1} max={1} bipolar reset={0} size={knob} label={smart.enabled ? `Smart CFX ${SMART_CFX_LABELS[smart.preset]}` : 'Color FX'} format={smart.enabled ? macroTxt : cfxTxt} onchange={(v) => cmd({ type: 'filter', channel: i as DeckIndex, value: v })} /><span>CFX</span></div>
        </div>
        <div class="mf">
          <div class="meter"><Meter source={i} /></div>
          <div class="fad"><Fader value={c.fader} reset={0.85} capSize={28} fill="none" label={`Channel ${i === 0 ? 'A' : 'B'} fader`} ticks={[0, 0.25, 0.5, 0.75, 1]} onchange={(v) => cmd({ type: 'fader', channel: i as DeckIndex, value: v })} /></div>
        </div>
        {#if smart.enabled}
          {@const arming = fxControl.state.smartArming[i]}
          <div class="macro" class:arming title={arming ? 'This CFX knob is ignored until it passes the centre' : `${SMART_CFX_LABELS[smart.preset]}: ${macroTxt(c.filter)}`}>
            {#if arming}
              <span class="arm">Arm: return to centre</span>
            {:else}
              <span class:hot={c.filter < -0.02}>{sides[0]}</span>
              <span class:hot={c.filter > 0.02}>{sides[1]}</span>
            {/if}
            <span class="mbar">
              <i style="left:{50 + Math.min(0, c.filter) * 50}%;width:{Math.abs(c.filter) * 50}%"></i>
              <b style="left:{50 + c.filter * 50}%"></b>
            </span>
          </div>
        {/if}
        <button class="pfl" data-on={c.pfl} class:on={c.pfl} onclick={() => cmd({ type: 'pfl', channel: i as DeckIndex, enabled: !c.pfl })} title="Headphone cue" aria-pressed={c.pfl}>
          <Headphones size={12} strokeWidth={2.2} />Cue
        </button>
      </div>
    {/each}

    <div class="master">
      {#if smart.enabled}
        <Select value={smart.preset} options={SMART_PRESETS} label="Smart CFX preset" width={78} accent="var(--smart)" onchange={(preset) => cmd({ type: 'smartCfxPreset', preset })} />
      {:else}
        <Select value={colorFx} options={COLOR_FX} label="Color FX type" width={78} accent="var(--fx)" onchange={(kind) => cmd({ type: 'colorFx', kind })} />
      {/if}
      <button class="smartsw" class:on={smart.enabled} onclick={() => cmd({ type: 'smartCfx', enabled: !smart.enabled })} aria-pressed={smart.enabled} title="Smart CFX: each CFX knob drives a one-knob macro instead of Color FX (SMART CFX on the FLX4)">
        <i class="sw"><b></b></i>Smart
      </button>
      {#if colorParam && !smart.enabled}
        <div class="cfxp">
          <Knob value={engine.snap.colorFxParams[0] ?? colorParam.default} min={colorParam.min} max={colorParam.max} reset={colorParam.default} size={22} label={`Color FX ${colorParam.label}`} format={(v) => `${Math.round(v)}%`} onchange={(v) => cmd({ type: 'colorFxParam', index: 0, value: v })} />
          <span>{colorParam.label}</span>
        </div>
      {/if}
      <div class="mtop">
        <Knob value={engine.hardware.masterLevel ?? master.level} reset={0.8} size={ui.compact ? 30 : 34} label="Master level" format={pct} onchange={(v) => { engine.hardware = { ...engine.hardware, masterLevel: undefined }; cmd({ type: 'masterLevel', value: v }); }} />
        <i class="clip" class:on={engine.clipLatch} title="Master clip"></i>
      </div>
      <div class="mmeter"><Meter source="master" scale /></div>
      <div class="hp">
        {#if master.hardwarePhones}
          <div class="hks hw" title="Headphone mix and level are set on the FLX4; these mirror its knobs.">
            <div class="hk"><Knob value={engine.hardware.headphoneMix ?? master.headphoneMix} size={24} label="Headphone mix (FLX4)" disabled format={(v) => (v < 0.02 ? 'CUE' : v > 0.98 ? 'MASTER' : `${Math.round(v * 100)}% master`)} onchange={() => {}} /><span>FLX4 MIX</span></div>
            <div class="hk"><Knob value={engine.hardware.headphoneLevel ?? master.headphoneLevel} size={24} label="Headphone level (FLX4)" disabled format={pct} onchange={() => {}} /><span>FLX4 LVL</span></div>
          </div>
        {:else}
          <div class="hks">
            <div class="hk"><Knob value={master.headphoneMix} size={24} label="Headphone mix" format={(v) => (v < 0.02 ? 'CUE' : v > 0.98 ? 'MASTER' : `${Math.round(v * 100)}% master`)} onchange={(v) => cmd({ type: 'headphoneMix', value: v })} /><span>MIX</span></div>
            <div class="hk"><Knob value={master.headphoneLevel} size={24} label="Headphone level" format={pct} onchange={(v) => cmd({ type: 'headphoneLevel', value: v })} /><span>PHONES</span></div>
          </div>
        {/if}
      </div>
    </div>
  </div>

  <div class="xf">
    <b class="end" style="color:var(--deck-a)">A</b>
    <div class="xfw" class:idle={!master.crossfaderActive} title={master.curve === 'off' ? 'Crossfader is off' : master.crossfaderActive ? undefined : 'Move the crossfader past the centre to turn it back on'}><Fader orientation="horizontal" value={master.crossfader} reset={0.5} detent={0.5} capSize={30} fill="none" label="Crossfader" ticks={[0, 0.25, 0.5, 0.75, 1]} onchange={(v) => cmd({ type: 'crossfader', value: v })} /></div>
    <b class="end" style="color:var(--deck-b)">B</b>
    <div class="seg" role="group" aria-label="Crossfader curve">
      {#each CURVES as cv (cv.id)}
        <button class:on={master.curve === cv.id} title={cv.title} onclick={() => cmd({ type: 'crossfaderCurve', curve: cv.id })}>{cv.label}</button>
      {/each}
    </div>
  </div>
</section>

<style>
  .mixer {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 10px 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    min-width: 0;
  }
  .chs {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 82px minmax(0, 1fr);
    gap: 6px;
  }
  .ch[data-deck='0'] {
    grid-column: 1;
  }
  .ch[data-deck='1'] {
    grid-column: 3;
  }
  .ch {
    grid-row: 1;
    display: grid;
    grid-template-columns: 56px 1fr;
    grid-template-rows: 1fr auto auto;
    column-gap: 5px;
    row-gap: 5px;
    min-height: 0;
  }
  .ch.rev {
    grid-template-columns: 1fr 56px;
  }
  .ch.rev .knobs {
    order: 2;
    align-items: flex-end;
  }
  .ch.rev .mf {
    order: 1;
    flex-direction: row-reverse;
  }
  .ch.rev .macro {
    order: 3;
  }
  .ch.rev .pfl {
    order: 4;
  }
  .kr.cfx.smart {
    --knob: var(--smart);
    color: var(--smart);
  }
  .macro {
    grid-column: 1 / -1;
    display: grid;
    grid-template-columns: 1fr auto;
    row-gap: 3px;
    padding: 3px 5px 4px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, var(--smart) 30%, var(--line));
    background: color-mix(in srgb, var(--smart) 7%, transparent);
    font-size: 9px;
    font-weight: 650;
    color: var(--faint);
    white-space: nowrap;
  }
  .macro span.hot {
    color: var(--smart);
  }
  .macro .arm {
    grid-column: 1 / -1;
    color: var(--cue);
    animation: blink 0.9s steps(1) infinite;
  }
  .macro.arming {
    border-color: color-mix(in srgb, var(--cue) 50%, var(--line));
    background: color-mix(in srgb, var(--cue) 8%, transparent);
  }
  .mbar {
    grid-column: 1 / -1;
    position: relative;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.08);
  }
  .mbar::after {
    content: '';
    position: absolute;
    left: 50%;
    top: -2px;
    bottom: -2px;
    width: 1px;
    background: var(--line-2);
  }
  .mbar i {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: inherit;
    background: var(--smart);
    box-shadow: 0 0 8px -1px var(--smart);
  }
  .mbar b {
    position: absolute;
    top: -2px;
    width: 2px;
    height: 8px;
    margin-left: -1px;
    border-radius: 1px;
    background: #fff;
  }
  .arming .mbar i {
    background: var(--cue);
    opacity: 0.4;
    box-shadow: none;
  }
  .smartsw {
    width: 78px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 7px;
    border-radius: 5px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--dim);
    font-size: 10.5px;
    font-weight: 650;
    transition:
      color var(--dur),
      border-color var(--dur),
      background var(--dur);
  }
  .smartsw:hover {
    color: var(--fg);
    border-color: var(--line-2);
  }
  .sw {
    position: relative;
    width: 20px;
    height: 11px;
    border-radius: 6px;
    background: var(--raised-3);
    transition: background var(--dur);
  }
  .sw b {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--dim);
    transition:
      translate var(--dur) var(--ease),
      background var(--dur);
  }
  .smartsw.on {
    color: #ccfbf1;
    border-color: color-mix(in srgb, var(--smart) 60%, transparent);
    background: color-mix(in srgb, var(--smart) 12%, transparent);
  }
  .smartsw.on .sw {
    background: color-mix(in srgb, var(--smart) 55%, var(--raised-3));
  }
  .smartsw.on .sw b {
    translate: 9px 0;
    background: #fff;
  }
  .knobs {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }
  .kr {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 8.5px;
    font-weight: 650;
    color: var(--faint);
  }
  .kr.cfx {
    --knob: var(--fx);
  }
  .ch.rev .kr {
    flex-direction: row-reverse;
  }
  .mf {
    display: flex;
    gap: 5px;
    min-height: 0;
    justify-content: center;
  }
  .meter {
    width: 20px;
    min-height: 60px;
  }
  .fad {
    width: 28px;
    min-height: 60px;
  }
  .pfl {
    grid-column: 1 / -1;
    height: 24px;
    border-radius: 5px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--dim);
    font-size: 10.5px;
    font-weight: 650;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    transition:
      background var(--dur),
      color var(--dur),
      border-color var(--dur);
  }
  .pfl:hover {
    background: var(--raised);
    color: var(--fg);
    border-color: var(--line-2);
  }
  .pfl.on {
    color: #1c1203;
    background: var(--cue);
    border-color: var(--cue);
  }
  .master {
    grid-column: 2;
    grid-row: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    min-height: 0;
  }
  .mtop {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    position: relative;
  }
  .clip {
    position: absolute;
    top: 0;
    right: -8px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--raised-3);
  }
  .clip.on {
    background: var(--danger);
  }
  .mmeter {
    flex: 1;
    min-height: 34px;
    width: 52px;
  }
  .hp {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
  }
  .hks.hw :global(.knob.disabled) {
    opacity: 0.9;
  }
  .hks.hw span {
    color: var(--accent, var(--dim));
  }
  .cfxp {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    font-size: 7.5px;
    font-weight: 650;
    color: var(--faint);
  }
  .hks {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    width: 100%;
  }
  .hk {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    font-size: 7.5px;
    font-weight: 650;
    color: var(--faint);
  }
  .xf {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
  }
  @media (max-height: 820px) {
    .mixer {
      padding: 8px 8px 6px;
    }
    .xf {
      height: 26px;
    }
  }
  .xfw {
    flex: 1;
    min-width: 0;
    transition: opacity 120ms;
  }
  .xfw.idle {
    opacity: 0.4;
  }
  .end {
    font-size: 12px;
    font-weight: 750;
  }
  .seg {
    display: flex;
    border: 1px solid var(--line);
    border-radius: 5px;
    padding: 2px;
    gap: 2px;
    flex: none;
  }
  .seg button {
    border: 0;
    height: 20px;
    padding: 0 7px;
    border-radius: 3px;
    background: transparent;
    color: var(--dim);
    font-size: 10.5px;
    font-weight: 650;
    transition:
      background var(--dur),
      color var(--dur);
  }
  .seg button:hover {
    color: var(--fg);
    background: rgba(255, 255, 255, 0.06);
  }
  .seg button.on {
    background: rgba(250, 250, 250, 0.12);
    color: var(--fg);
  }
</style>
