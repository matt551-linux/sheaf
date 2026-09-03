<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    title?: string;
    disabled?: boolean;
    children: Snippet<[() => void]>;
  }
  let { label, title, disabled = false, children }: Props = $props();

  let open = $state(false);
  let root: HTMLDivElement;

  function close() {
    open = false;
  }
  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) close();
  }
  function onWindowKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") close();
  }
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onWindowKey} />

<div class="relative" bind:this={root}>
  <button
    class="inline-flex h-8 shrink-0 items-center gap-0.5 whitespace-nowrap rounded px-2 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700 {open ? 'bg-neutral-200 dark:bg-neutral-700' : ''}"
    {disabled}
    {title}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    {label}
    <svg class="h-3 w-3 opacity-60" viewBox="0 0 12 12" fill="currentColor"><path d="M3 4.5l3 3 3-3z" /></svg>
  </button>
  {#if open}
    <div
      class="absolute left-0 top-full z-40 mt-0.5 min-w-44 rounded border border-neutral-300 bg-white py-1 shadow-lg dark:border-neutral-600 dark:bg-neutral-900"
      role="menu"
    >
      {@render children(close)}
    </div>
  {/if}
</div>
