<script lang="ts">
  import { ui } from '$lib/state/ui.svelte';
  import Sheet from './Sheet.svelte';
  import AudioSettings from './AudioSettings.svelte';
  import ControllerPanel from './ControllerPanel.svelte';
  import RecordingSettings from './RecordingSettings.svelte';
</script>

<Sheet open={ui.settingsOpen} title="Settings" onclose={() => (ui.settingsOpen = false)} width={620}>
  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={ui.settingsTab === 'audio'} class:on={ui.settingsTab === 'audio'} onclick={() => (ui.settingsTab = 'audio')}>Audio</button>
    <button role="tab" aria-selected={ui.settingsTab === 'controller'} class:on={ui.settingsTab === 'controller'} onclick={() => (ui.settingsTab = 'controller')}>Controller &amp; MIDI</button>
    <button role="tab" aria-selected={ui.settingsTab === 'recording'} class:on={ui.settingsTab === 'recording'} onclick={() => (ui.settingsTab = 'recording')}>Recording</button>
  </div>
  {#if ui.settingsTab === 'audio'}
    <AudioSettings />
  {:else if ui.settingsTab === 'controller'}
    <ControllerPanel />
  {:else}
    <RecordingSettings />
  {/if}
</Sheet>

<style>
  .tabs {
    display: flex;
    gap: 4px;
    padding: 10px 20px 0;
    border-bottom: 1px solid var(--line);
    position: sticky;
    top: 0;
    background: var(--panel);
    z-index: 2;
  }
  .tabs button {
    height: 32px;
    padding: 0 12px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--dim);
    font-weight: 620;
    font-size: 12.5px;
  }
  .tabs button:hover {
    color: var(--fg);
  }
  .tabs button.on {
    color: #fff;
    border-bottom-color: var(--fg);
  }
</style>
