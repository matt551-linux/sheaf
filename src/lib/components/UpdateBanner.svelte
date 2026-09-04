<script lang="ts">
  import { updater } from "$lib/stores/updater.svelte";
  const s = $derived(updater.state);
  const show = $derived(
    (s.kind === "available" && updater.dismissed !== s.version) || s.kind === "downloading" || s.kind === "ready" || (s.kind === "error" && s.message),
  );
  const pct = $derived(s.kind === "downloading" && s.total ? Math.min(100, Math.round((s.received / s.total) * 100)) : null);
  const mb = (n: number) => (n / 1048576).toFixed(1);
</script>

{#if show}
  <div class="flex items-center gap-3 border-b border-blue-200 bg-blue-50 px-3 py-1.5 text-sm text-blue-900 dark:border-blue-900 dark:bg-blue-950 dark:text-blue-100" role="status">
    {#if s.kind === "available"}
      <span class="font-semibold">Sheaf {s.version} is available.</span>
      {#if s.notes}<span class="truncate text-xs opacity-80" title={s.notes}>{s.notes.split("\n")[0]}</span>{/if}
      <span class="flex-1"></span>
      <button class="rounded bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700" onclick={() => updater.install()}>Update and restart</button>
      <button class="rounded px-2 py-1 text-xs hover:bg-blue-100 dark:hover:bg-blue-900" onclick={() => updater.dismiss()}>Later</button>
    {:else if s.kind === "downloading"}
      <span>Downloading Sheaf {s.version}…</span>
      <div class="h-1.5 w-40 overflow-hidden rounded bg-blue-200 dark:bg-blue-900">
        <div class="h-full bg-blue-600 transition-[width]" style="width: {pct ?? 30}%"></div>
      </div>
      <span class="text-xs opacity-80">{s.total ? `${mb(s.received)} / ${mb(s.total)} MB` : `${mb(s.received)} MB`}</span>
    {:else if s.kind === "ready"}
      <span>Installing Sheaf {s.version} and restarting…</span>
    {:else if s.kind === "error"}
      <span class="text-red-800 dark:text-red-200">Update failed: {s.message}</span>
      <span class="flex-1"></span>
      <button class="rounded px-2 py-1 text-xs hover:bg-blue-100 dark:hover:bg-blue-900" onclick={() => (updater.state = { kind: "idle" })}>Dismiss</button>
    {/if}
  </div>
{/if}
