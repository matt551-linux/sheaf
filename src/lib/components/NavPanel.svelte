<script lang="ts">
  import { docStore } from "$lib/stores/document.svelte";
  import { api, type OutlineNode } from "$lib/api";

  interface Props {
    onGoToPage: (index: number) => void;
  }
  let { onGoToPage }: Props = $props();

  // Thumbnails: rendered at a small fixed scale, independent of the main zoom.
  let thumbs = $state<Record<number, string>>({});
  let thumbDocId = -1;
  const THUMB_SCALE = 0.25;

  $effect(() => {
    const doc = docStore.doc;
    if (!doc) {
      thumbs = {};
      thumbDocId = -1;
      return;
    }
    if (docStore.navPanel !== "thumbnails" || thumbDocId === doc.id) return;
    thumbDocId = doc.id;
    thumbs = {};
    (async () => {
      for (let i = 0; i < doc.page_count; i++) {
        if (docStore.doc?.id !== doc.id) return;
        const r = await api.renderPage(doc.id, i, THUMB_SCALE * (96 / 72)).catch(() => null);
        if (r) thumbs = { ...thumbs, [i]: `data:image/png;base64,${r.png_base64}` };
      }
    })();
  });

  let query = $state("");
  let searching = $state(false);
  async function doSearch(e?: Event) {
    e?.preventDefault();
    searching = true;
    await docStore.runSearch(query);
    searching = false;
    const hit = docStore.searchHits[docStore.searchIndex];
    if (hit) onGoToPage(hit.page_index);
  }
  function jump(dir: 1 | -1) {
    docStore.nextHit(dir);
    const hit = docStore.searchHits[docStore.searchIndex];
    if (hit) onGoToPage(hit.page_index);
  }

  export function focusSearch() {
    docStore.navPanel = "search";
    requestAnimationFrame(() => document.getElementById("sheaf-search-input")?.focus());
  }
</script>

{#snippet outlineTree(nodes: OutlineNode[], depth: number)}
  <ul class="text-sm">
    {#each nodes as n}
      <li>
        <button
          class="block w-full truncate rounded px-1 py-0.5 text-left hover:bg-neutral-200 disabled:text-neutral-400 dark:hover:bg-neutral-700"
          style="padding-left:{depth * 12 + 4}px"
          disabled={n.page_index === null}
          onclick={() => n.page_index !== null && onGoToPage(n.page_index)}
          title={n.title}>{n.title || "(untitled)"}</button
        >
        {#if n.children.length}
          {@render outlineTree(n.children, depth + 1)}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#if docStore.doc && docStore.navPanel !== "none"}
  <aside
    class="flex h-full w-56 shrink-0 flex-col overflow-hidden border-r border-neutral-300 bg-neutral-50 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
  >
    {#if docStore.navPanel === "thumbnails"}
      <div class="border-b border-neutral-200 px-2 py-1 text-xs font-semibold uppercase text-neutral-500 dark:border-neutral-700">Pages</div>
      <div class="flex-1 overflow-auto p-2">
        {#each docStore.doc.pages as p (p.index)}
          <button
            class="mb-3 block w-full rounded p-1 text-center hover:bg-neutral-200 dark:hover:bg-neutral-700 {docStore.currentPage === p.index ? 'ring-2 ring-blue-500' : ''}"
            onclick={() => onGoToPage(p.index)}
            aria-current={docStore.currentPage === p.index ? "page" : undefined}
          >
            {#if thumbs[p.index]}
              <img src={thumbs[p.index]} alt="Page {p.index + 1}" class="mx-auto max-h-48 shadow" draggable="false" />
            {:else}
              <div class="mx-auto flex h-40 w-28 items-center justify-center bg-white text-xs text-neutral-400 shadow">...</div>
            {/if}
            <div class="mt-1 text-xs">{p.index + 1}</div>
          </button>
        {/each}
      </div>
    {:else if docStore.navPanel === "bookmarks"}
      <div class="border-b border-neutral-200 px-2 py-1 text-xs font-semibold uppercase text-neutral-500 dark:border-neutral-700">Bookmarks</div>
      <div class="flex-1 overflow-auto p-1">
        {#if docStore.outline.length}
          {@render outlineTree(docStore.outline, 0)}
        {:else}
          <p class="p-2 text-sm text-neutral-500">This document has no bookmarks.</p>
        {/if}
      </div>
    {:else if docStore.navPanel === "search"}
      <div class="border-b border-neutral-200 px-2 py-1 text-xs font-semibold uppercase text-neutral-500 dark:border-neutral-700">Find</div>
      <form class="flex gap-1 p-2" onsubmit={doSearch}>
        <input
          id="sheaf-search-input"
          class="h-7 min-w-0 flex-1 rounded border border-neutral-300 bg-white px-1 text-sm dark:border-neutral-600 dark:bg-neutral-800"
          placeholder="Find in document"
          bind:value={query}
        />
        <button class="h-7 rounded bg-blue-600 px-2 text-sm text-white" type="submit" disabled={searching}>Go</button>
      </form>
      {#if docStore.searchHits.length}
        <div class="flex items-center gap-1 px-2 text-xs text-neutral-500">
          <button class="rounded px-1 hover:bg-neutral-200 dark:hover:bg-neutral-700" onclick={() => jump(-1)}>&#9650;</button>
          <button class="rounded px-1 hover:bg-neutral-200 dark:hover:bg-neutral-700" onclick={() => jump(1)}>&#9660;</button>
          <span>{docStore.searchIndex + 1} of {docStore.searchHits.length}</span>
        </div>
      {:else if docStore.searchQuery && !searching}
        <p class="px-2 text-xs text-neutral-500">No matches.</p>
      {/if}
      <div class="flex-1 overflow-auto">
        {#each docStore.searchHits as h, i}
          <button
            class="block w-full border-b border-neutral-100 px-2 py-1 text-left text-xs hover:bg-neutral-200 dark:border-neutral-800 dark:hover:bg-neutral-700 {i === docStore.searchIndex ? 'bg-blue-100 dark:bg-blue-900' : ''}"
            onclick={() => {
              docStore.searchIndex = i;
              onGoToPage(h.page_index);
            }}
          >
            <span class="font-semibold">p.{h.page_index + 1}</span>
            <span class="text-neutral-600 dark:text-neutral-300">{h.context}</span>
          </button>
        {/each}
      </div>
    {/if}
  </aside>
{/if}
