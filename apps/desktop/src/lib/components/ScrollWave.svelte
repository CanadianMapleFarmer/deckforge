<script lang="ts">
  import { ScrollerRenderer } from '$lib/waveform/draw';
  import { canvasPreview, scrubGesture } from '$lib/waveform/seek';
  import { trackSecsPerPx } from '$lib/waveform/scale';
  import { engine } from '$lib/state/engine.svelte';
  import NativeSurface from '$lib/native/NativeSurface.svelte';
  import { native, nativeDragPreview } from '$lib/native/native.svelte';
  import { ui, ZOOM_LEVELS } from '$lib/state/ui.svelte';
  import { dropTargets } from '$lib/state/dnd.svelte';
  import CanvasLayer from './CanvasLayer.svelte';
  import type { DeckIndex } from '$lib/api';

  let { deck }: { deck: DeckIndex } = $props();
  // svelte-ignore state_referenced_locally
  const renderer = new ScrollerRenderer(deck);
  // svelte-ignore state_referenced_locally
  const gesture = scrubGesture(deck, () => trackSecsPerPx(ZOOM_LEVELS[ui.zoomIndex], engine.latest.decks[deck].tempoRatio), (d, secs) =>
    (native.available ? nativeDragPreview : canvasPreview)(d, secs),
  );
  let wheelAcc = 0;
  let grabbing = $state(false);

  function wheel(e: WheelEvent) {
    e.preventDefault();
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return gesture.wheel(e.deltaX);
    wheelAcc += e.deltaY;
    if (Math.abs(wheelAcc) > 40) {
      ui.zoom(wheelAcc > 0 ? -1 : 1);
      wheelAcc = 0;
    }
  }
</script>

<div class="wave" data-deck={deck} data-drop-deck={deck} class:drop={dropTargets.deck === deck}>
  <div class="layer">
    <NativeSurface id={deck === 0 ? 'scroll-a' : 'scroll-b'} zoom={ZOOM_LEVELS[ui.zoomIndex]}>
      {#snippet fallback()}
        <CanvasLayer label={`Deck ${deck === 0 ? 'A' : 'B'} scrolling waveform`} draw={(ctx, W, H, dpr, now) => renderer.drawBase(ctx, W, H, dpr, ZOOM_LEVELS[ui.zoomIndex], now)} onresize={() => renderer.resetCache()} />
      {/snippet}
    </NativeSurface>
  </div>
  {#if native.ready && !native.available}
    <div class="layer">
      <CanvasLayer label="" draw={(ctx, W, H, dpr) => renderer.drawOverlay(ctx, W, H, dpr, ZOOM_LEVELS[ui.zoomIndex])} />
    </div>
  {/if}
  <div
    class="hit"
    class:grabbing
    role="presentation"
    onwheel={wheel}
    onpointerdown={(e) => {
      grabbing = true;
      gesture.onpointerdown(e);
    }}
    onpointermove={gesture.onpointermove}
    onpointerup={(e) => {
      grabbing = false;
      gesture.onpointerup(e);
    }}
    onpointercancel={(e) => {
      grabbing = false;
      gesture.onpointercancel(e);
    }}
  ></div>
  <b class="tag">{deck === 0 ? 'A' : 'B'}</b>
</div>

<style>
  .wave {
    position: relative;
    flex: 1 1 0;
    min-height: 0;
    overflow: hidden;
    background: var(--wave-bg, #0b0b0d);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    transition: border-color var(--dur);
  }
  .wave.drop {
    border-color: var(--deck);
  }
  .layer {
    position: absolute;
    inset: 0;
  }
  .hit {
    position: absolute;
    inset: 0;
    cursor: grab;
    touch-action: none;
  }
  .hit.grabbing {
    cursor: grabbing;
  }
  .tag {
    position: absolute;
    left: 6px;
    bottom: 6px;
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    border-radius: 4px;
    font-size: 10.5px;
    font-weight: 750;
    color: #09090b;
    background: var(--deck);
    pointer-events: none;
  }
</style>
