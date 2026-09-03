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
  let trigger: HTMLButtonElement;
  // Fixed positioning so the menu overlays the page instead of being clipped
  // by (and scrolling inside) the toolbar's overflow-x-auto container.
  let menuLeft = $state(0);
  let menuTop = $state(0);

  function toggle() {
    if (!open) {
      const r = trigger.getBoundingClientRect();
      menuLeft = r.left;
      menuTop = r.bottom + 2;
    }
    open = !open;
  }
  function close() {
    open = false;
  }
  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) close();
  }
  function onWindowKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") close();
  }
  function onScroll(e: Event) {
    if (open && root && !root.contains(e.target as Node)) close();
  }
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onWindowKey} onresize={close} onscrollcapture={onScroll} />

<div class="relative" bind:this={root}>
  <button
    bind:this={trigger}
    class="inline-flex h-8 shrink-0 items-center gap-0.5 whitespace-nowrap rounded px-2 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700 {open ? 'bg-neutral-200 dark:bg-neutral-700' : ''}"
    {disabled}
    {title}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={toggle}
  >
    {label}
    <svg class="h-3 w-3 opacity-60" viewBox="0 0 12 12" fill="currentColor"><path d="M3 4.5l3 3 3-3z" /></svg>
  </button>
  {#if open}
    <div
      class="fixed z-50 min-w-44 rounded border border-neutral-300 bg-white py-1 shadow-lg dark:border-neutral-600 dark:bg-neutral-900"
      style="left:{menuLeft}px;top:{menuTop}px"
      role="menu"
    >
      {@render children(close)}
    </div>
  {/if}
</div>
