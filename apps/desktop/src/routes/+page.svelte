<script lang="ts">
  import { onMount } from 'svelte';
  import { engine } from '$lib/state/engine.svelte';
  import { library } from '$lib/state/library.svelte';
  import { history } from '$lib/state/history.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import { installShortcuts } from '$lib/shortcuts';
  import TopBar from '$lib/components/TopBar.svelte';
  import DeckTop from '$lib/components/DeckTop.svelte';
  import ScrollWave from '$lib/components/ScrollWave.svelte';
  import PhaseMeter from '$lib/components/PhaseMeter.svelte';
  import DeckControls from '$lib/components/DeckControls.svelte';
  import FxHud from '$lib/components/FxHud.svelte';
  import Mixer from '$lib/components/Mixer.svelte';
  import LibraryPanel from '$lib/components/LibraryPanel.svelte';
  import ZoomControl from '$lib/components/ZoomControl.svelte';
  import SettingsSheet from '$lib/components/SettingsSheet.svelte';
  import HelpOverlay from '$lib/components/HelpOverlay.svelte';
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
  import ImportReport from '$lib/components/ImportReport.svelte';
  import Toasts from '$lib/components/Toasts.svelte';
  import DragGhost from '$lib/components/DragGhost.svelte';
  import { runSelfTest } from '$lib/selftest';

  let lib: LibraryPanel;
  const tokens = $derived(`${engine.snap.decks[0].loaded ? engine.snap.decks[0].trackToken : 0}:${engine.snap.decks[1].loaded ? engine.snap.decks[1].trackToken : 0}`);

  $effect(() => {
    tokens;
    library.syncDeckInfo();
  });

  onMount(() => {
    const cleanups: (() => void)[] = [];
    let disposed = false;
    (async () => {
      const offEngine = await engine.init();
      const offLib = await library.init();
      const offHistory = await history.init();
      if (disposed) {
        offEngine();
        offLib();
        offHistory();
        return;
      }
      cleanups.push(offEngine, offLib, offHistory);
      if (engine.preview === false) void runSelfTest();
      else {
        document.body.classList.add('preview');
        const view = new URLSearchParams(location.search).get('view');
        if (view === 'spotify' || view === 'history') ui.browserView = view;
        if (new URLSearchParams(location.search).get('settings') === 'recording') ui.openSettings('recording');
      }
    })();
    cleanups.push(installShortcuts(() => lib?.focusSearch()), ui.watchViewport());
    const noMenu = (e: Event) => {
      if ((e.target as HTMLElement | null)?.closest('input, textarea')) return;
      e.preventDefault();
    };
    window.addEventListener('contextmenu', noMenu);
    return () => {
      disposed = true;
      cleanups.forEach((c) => c());
      window.removeEventListener('contextmenu', noMenu);
    };
  });
</script>

<svelte:head><title>deckforge</title></svelte:head>

<div class="app">
  <TopBar />
  <div class="scrolls">
    <ScrollWave deck={0} />
    <PhaseMeter />
    <ScrollWave deck={1} />
    <div class="zoom"><ZoomControl /></div>
  </div>
  <div class="perf">
    <DeckTop deck={0} />
    <FxHud />
    <DeckTop deck={1} />
    <DeckControls deck={0} />
    <Mixer />
    <DeckControls deck={1} />
  </div>
  <LibraryPanel bind:this={lib} />
</div>

<SettingsSheet />
<HelpOverlay />
<ConfirmDialog />
<ImportReport />
<DragGhost />
<Toasts />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    min-width: 1180px;
    min-height: 720px;
  }
  .scrolls {
    position: relative;
    flex: 1 1 0;
    min-height: 150px;
    display: flex;
    flex-direction: column;
    border: 8px solid var(--ink);
  }
  .zoom {
    position: absolute;
    right: 10px;
    top: 25px;
    z-index: 5;
  }
  .perf {
    flex: none;
    display: grid;
    grid-template-columns: minmax(0, 1fr) clamp(360px, 25vw, 420px) minmax(0, 1fr);
    grid-template-rows: auto auto;
    gap: 8px;
    padding: 0 8px 8px;
    background: var(--ink);
  }
  :global([data-native-waveforms]) .perf {
    background: transparent;
  }
</style>
