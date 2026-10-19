<script lang="ts">
  import { history } from '$lib/state/history.svelte';
  import { formatDuration } from '$lib/util/duration';

  const rec = $derived(history.recording);
  const size = $derived.by(() => {
    if (!rec) return '';
    const mb = rec.bytes / 1_048_576;
    return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(mb < 10 ? 1 : 0)} MB`;
  });
  const elapsed = $derived(rec ? (rec.elapsedSecs >= 3600 ? formatDuration(rec.elapsedSecs) : `0:${formatDuration(rec.elapsedSecs)}`) : '');
  const title = $derived(
    rec
      ? `Recording to ${rec.path}${rec.droppedSecs > 0 ? `. ${rec.droppedSecs.toFixed(2)} s dropped by a slow disk` : ''}. Click to stop.`
      : 'Record the mix. The file is written before the master volume, so turning the room down never changes it.',
  );
</script>

<button class="rec" class:on={rec !== null} class:dropped={(rec?.droppedSecs ?? 0) > 0} onclick={() => history.toggleRecording()} disabled={history.busy} {title} aria-pressed={rec !== null} aria-label={rec ? 'Stop recording' : 'Start recording'}>
  <i></i>
  {#if rec}
    <span class="time num">{elapsed}</span>
    <span class="size num">{size}</span>
  {:else}
    <span class="lbl">Rec</span>
  {/if}
</button>

<style>
  .rec {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    padding: 0 10px 0 9px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--dim);
    font-size: 11.5px;
    font-weight: 620;
    white-space: nowrap;
    transition:
      border-color var(--dur),
      background var(--dur);
  }
  .rec:hover {
    border-color: var(--line-2);
    background: var(--raised-2);
    color: var(--fg);
  }
  .rec i {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--faint);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.15);
  }
  .rec:hover i {
    background: color-mix(in srgb, var(--danger) 70%, var(--faint));
  }
  .rec.on {
    border-color: color-mix(in srgb, var(--danger) 60%, var(--line));
    background: color-mix(in srgb, var(--danger) 16%, var(--raised));
    color: #fecaca;
  }
  .rec.on i {
    background: var(--danger);
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--danger) 70%, transparent);
    animation: beat 1.4s ease-out infinite;
  }
  .rec.dropped {
    border-color: var(--warn);
  }
  .time {
    color: #fff;
    font-size: 12px;
  }
  .size {
    color: color-mix(in srgb, #fecaca 70%, transparent);
    font-weight: 520;
  }
  @keyframes beat {
    60% {
      box-shadow: 0 0 0 5px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  @media (max-width: 1800px) {
    .size {
      display: none;
    }
  }
  @media (max-width: 1440px) {
    .lbl {
      display: none;
    }
    .rec {
      gap: 6px;
      padding: 0 8px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .rec.on i {
      animation: none;
    }
  }
</style>
