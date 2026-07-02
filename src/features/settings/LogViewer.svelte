<script lang="ts">
  import { onMount } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import { logsStore } from "$lib/stores/logs.svelte";
  import type { LogLevel } from "$lib/stores/logs.svelte";

  const levels: { value: LogLevel | ""; key: string }[] = [
    { value: "", key: "allLevels" },
    { value: "error", key: "levelError" },
    { value: "warn", key: "levelWarn" },
    { value: "info", key: "levelInfo" },
    { value: "debug", key: "levelDebug" },
  ];

  let autoRefresh = $state(false);
  let listEl = $state<HTMLElement | null>(null);

  async function refresh() {
    await logsStore.load();
    // Nejnovější záznamy jsou dole — po načtení sjeď na konec.
    if (listEl) listEl.scrollTop = listEl.scrollHeight;
  }

  onMount(() => {
    void refresh();
  });

  $effect(() => {
    if (!autoRefresh) return;
    const id = setInterval(() => void refresh(), 2000);
    return () => clearInterval(id);
  });

  function levelClass(level: string): string {
    return `lvl-${level.toLowerCase()}`;
  }

  /** Z „2026-07-02T13:00:00.123456Z" udělá čitelné „02.07. 13:00:00". */
  function formatTimestamp(ts: string): string {
    const d = new Date(ts);
    if (Number.isNaN(d.getTime())) return ts;
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${pad(d.getDate())}.${pad(d.getMonth() + 1)}. ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }
</script>

<div class="log-viewer">
  <div class="filters">
    <select
      bind:value={logsStore.minLevel}
      onchange={() => void refresh()}
      aria-label={i18n.m.settings.logs.level}
    >
      {#each levels as level (level.value)}
        <option value={level.value}>
          {i18n.t(`settings.logs.${level.key}`)}
        </option>
      {/each}
    </select>

    <select
      bind:value={logsStore.target}
      onchange={() => void refresh()}
      aria-label={i18n.m.settings.logs.module}
    >
      <option value="">{i18n.m.settings.logs.allModules}</option>
      {#each logsStore.targets as t (t)}
        <option value={t}>{t}</option>
      {/each}
    </select>

    <input
      type="search"
      placeholder={i18n.m.settings.logs.searchPlaceholder}
      bind:value={logsStore.search}
      onchange={() => void refresh()}
    />

    <button class="refresh-btn" onclick={() => void refresh()} disabled={logsStore.loading}>
      {i18n.m.settings.logs.refresh}
    </button>

    <label class="auto-refresh">
      <input type="checkbox" bind:checked={autoRefresh} />
      {i18n.m.settings.logs.autoRefresh}
    </label>
  </div>

  <div class="log-list" bind:this={listEl}>
    {#if logsStore.entries.length === 0}
      <p class="empty">{i18n.m.settings.logs.empty}</p>
    {:else}
      {#each logsStore.entries as entry, i (i)}
        <div class="log-line">
          <span class="ts">{formatTimestamp(entry.timestamp)}</span>
          <span class="level {levelClass(entry.level)}">{entry.level}</span>
          <span class="target" title={entry.target}>{entry.target.split("::")[0]}</span>
          <span class="msg">{entry.message}</span>
        </div>
      {/each}
    {/if}
  </div>

  <p class="hint">{i18n.m.settings.logs.hint}</p>
</div>

<style>
  .log-viewer {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-height: 0;
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    align-items: center;
  }

  .filters select,
  .filters input[type="search"] {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.3rem 0.5rem;
    font-size: 0.82rem;
  }

  .filters input[type="search"] {
    flex: 1;
    min-width: 140px;
  }

  .refresh-btn {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.3rem 0.7rem;
    font-size: 0.82rem;
    cursor: pointer;
  }

  .refresh-btn:hover:not(:disabled) {
    border-color: var(--color-accent);
  }

  .auto-refresh {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.82rem;
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .log-list {
    height: 380px;
    overflow: auto;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.5rem;
    font-family: "JetBrains Mono", "Fira Code", monospace;
    font-size: 0.75rem;
    line-height: 1.5;
  }

  .log-line {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .ts {
    color: var(--color-text-muted);
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .level {
    flex-shrink: 0;
    font-weight: 700;
    width: 3.2rem;
  }

  .lvl-error { color: #e5534b; }
  .lvl-warn { color: #d29922; }
  .lvl-info { color: #57ab5a; }
  .lvl-debug { color: var(--color-text-muted); }
  .lvl-trace { color: var(--color-text-muted); }

  .target {
    color: var(--color-accent);
    flex-shrink: 0;
    max-width: 9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .msg {
    flex: 1;
  }

  .empty {
    color: var(--color-text-muted);
    text-align: center;
    padding: 2rem 0;
  }

  .hint {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    margin: 0;
  }
</style>
