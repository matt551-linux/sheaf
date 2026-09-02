<script lang="ts">
  import { docStore } from "$lib/stores/document.svelte";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onGoToPage: (index: number) => void;
  }
  let { onGoToPage }: Props = $props();

  let pageInput = $state("1");
  $effect(() => {
    pageInput = String(docStore.currentPage + 1);
  });

  async function openFile() {
    const picked = await open({
      multiple: false,
      filters: [{ name: "PDF documents", extensions: ["pdf"] }],
    });
    if (typeof picked === "string") await docStore.open(picked);
  }

  function submitPage(e: Event) {
    e.preventDefault();
    const n = parseInt(pageInput, 10);
    if (!docStore.doc || Number.isNaN(n)) return;
    const idx = Math.min(docStore.doc.page_count, Math.max(1, n)) - 1;
    onGoToPage(idx);
  }

  const zoomPct = $derived(Math.round(docStore.zoom * 100));
  const hasDoc = $derived(!!docStore.doc);
  const btn =
    "inline-flex h-8 min-w-8 items-center justify-center rounded px-2 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700";
</script>

<div
  class="flex h-11 items-center gap-1 border-b border-neutral-300 bg-neutral-100 px-2 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
  role="toolbar"
  aria-label="Main toolbar"
>
  <button class={btn} onclick={openFile} title="Open (Ctrl+O)">Open</button>
  <span class="mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700"></span>

  <button
    class={btn}
    disabled={!hasDoc}
    onclick={() => (docStore.navPanel = docStore.navPanel === "thumbnails" ? "none" : "thumbnails")}
    title="Page thumbnails"
    aria-pressed={docStore.navPanel === "thumbnails"}>Pages</button
  >
  <button
    class={btn}
    disabled={!hasDoc}
    onclick={() => (docStore.navPanel = docStore.navPanel === "bookmarks" ? "none" : "bookmarks")}
    title="Bookmarks"
    aria-pressed={docStore.navPanel === "bookmarks"}>Bookmarks</button
  >
  <button
    class={btn}
    disabled={!hasDoc}
    onclick={() => (docStore.navPanel = docStore.navPanel === "search" ? "none" : "search")}
    title="Find (Ctrl+F)"
    aria-pressed={docStore.navPanel === "search"}>Find</button
  >
  <span class="mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700"></span>

  <button class={btn} disabled={!hasDoc || docStore.currentPage === 0} onclick={() => onGoToPage(docStore.currentPage - 1)} title="Previous page">&#9650;</button>
  <form onsubmit={submitPage} class="flex items-center gap-1 text-sm">
    <input
      class="h-7 w-12 rounded border border-neutral-300 bg-white px-1 text-center dark:border-neutral-600 dark:bg-neutral-800"
      bind:value={pageInput}
      disabled={!hasDoc}
      aria-label="Page number"
    />
    <span class="text-neutral-500">/ {docStore.doc?.page_count ?? 0}</span>
  </form>
  <button class={btn} disabled={!hasDoc || !docStore.doc || docStore.currentPage >= docStore.doc.page_count - 1} onclick={() => onGoToPage(docStore.currentPage + 1)} title="Next page">&#9660;</button>
  <span class="mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700"></span>

  <button class={btn} disabled={!hasDoc} onclick={() => docStore.zoomOut()} title="Zoom out (Ctrl+-)">&minus;</button>
  <select
    class="h-7 rounded border border-neutral-300 bg-white px-1 text-sm dark:border-neutral-600 dark:bg-neutral-800"
    disabled={!hasDoc}
    value={docStore.fitMode === "custom" ? String(zoomPct) : docStore.fitMode}
    onchange={(e) => {
      const v = (e.currentTarget as HTMLSelectElement).value;
      if (v === "width" || v === "page") docStore.setFit(v);
      else docStore.setZoom(parseInt(v, 10) / 100);
    }}
    aria-label="Zoom"
  >
    <option value="width">Fit width</option>
    <option value="page">Fit page</option>
    {#each [50, 75, 100, 125, 150, 200, 300, 400] as p}
      <option value={String(p)}>{p}%</option>
    {/each}
    {#if docStore.fitMode === "custom" && ![50, 75, 100, 125, 150, 200, 300, 400].includes(zoomPct)}
      <option value={String(zoomPct)}>{zoomPct}%</option>
    {/if}
  </select>
  <button class={btn} disabled={!hasDoc} onclick={() => docStore.zoomIn()} title="Zoom in (Ctrl++)">+</button>
  <span class="mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700"></span>
  <button class={btn} disabled={!hasDoc} onclick={() => docStore.rotateView(-90)} title="Rotate view counterclockwise (Ctrl+Shift+-)">&#8634;</button>
  <button class={btn} disabled={!hasDoc} onclick={() => docStore.rotateView(90)} title="Rotate view clockwise (Ctrl+Shift++)">&#8635;</button>

  <span class="flex-1"></span>
  {#if docStore.doc}
    <span class="truncate text-sm text-neutral-500" title={docStore.doc.path}>{docStore.doc.file_name}</span>
  {/if}
</div>
