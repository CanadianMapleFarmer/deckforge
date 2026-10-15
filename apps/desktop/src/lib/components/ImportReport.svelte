<script lang="ts">
  import { library } from '$lib/state/library.svelte';
  import Sheet from './Sheet.svelte';

  const report = $derived(library.importReport);
  const incomplete = $derived(report?.playlists.filter((p) => p.missing.length) ?? []);
  const close = () => (library.importReport = null);
</script>

<Sheet open={report !== null} title="Import finished" onclose={close} width={480}>
  {#if report}
    <p class="lead">
      {report.playlists.length} playlist{report.playlists.length === 1 ? '' : 's'} imported · {report.added} new track{report.added === 1 ? '' : 's'}.
      Some tracks couldn't be found on this computer:
    </p>
    {#each incomplete as p (p.id)}
      <section>
        <h3><span class="name">{p.name}</span><span class="num">{p.found} of {p.total} found</span></h3>
        <ul>
          {#each p.missing as m, i (i)}<li>{m}</li>{/each}
        </ul>
      </section>
    {/each}
    <p class="note">Import the same folder or file again once they're available — the playlists update in place.</p>
    <div class="actions"><button class="btn btn-primary" onclick={close}>Done</button></div>
  {/if}
</Sheet>

<style>
  .lead,
  .note {
    margin: 0 0 14px;
    color: var(--dim);
    font-size: 13px;
    line-height: 1.5;
  }
  .note {
    margin-top: 14px;
    font-size: 12px;
    color: var(--faint);
  }
  section + section {
    margin-top: 14px;
  }
  h3 {
    display: flex;
    gap: 10px;
    align-items: baseline;
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
  }
  h3 .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  h3 .num {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--warn);
  }
  ul {
    margin: 0;
    padding: 6px 10px;
    max-height: 180px;
    overflow-y: auto;
    list-style: none;
    border-radius: var(--radius);
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--line);
  }
  li {
    padding: 2px 0;
    font-size: 12px;
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
