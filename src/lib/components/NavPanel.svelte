<script lang="ts">
  import { docStore } from "$lib/stores/document.svelte";
  import { api, type Annotation, type OutlineNode } from "$lib/api";
  import { colorToCss } from "$lib/viewer/geometry";
  import { save } from "@tauri-apps/plugin-dialog";

  interface Props {
    onGoToPage: (index: number) => void;
    onOpenNote: (a: Annotation) => void;
  }
  let { onGoToPage, onOpenNote }: Props = $props();

  // Thumbnails
  let thumbs = $state<Record<number, string>>({});
  let thumbDocId = -1;
  let thumbVersion = -1;
  const THUMB_SCALE = 0.25 * (96 / 72);
  $effect(() => {
    const doc = docStore.doc;
    const ver = docStore.renderVersion;
    if (!doc) {
      thumbs = {};
      thumbDocId = -1;
      return;
    }
    if (docStore.navPanel !== "thumbnails" || (thumbDocId === doc.id && thumbVersion === ver)) return;
    thumbDocId = doc.id;
    thumbVersion = ver;
    (async () => {
      for (let i = 0; i < doc.page_count; i++) {
        if (docStore.doc?.id !== doc.id || docStore.renderVersion !== ver) return;
        const r = await api.renderPage(doc.id, i, THUMB_SCALE, 0).catch(() => null);
        if (r) thumbs = { ...thumbs, [i]: `data:image/png;base64,${r.png_base64}` };
      }
    })();
  });

  // Search
  let query = $state("");
  let caseSensitive = $state(false);
  let wholeWord = $state(false);
  let searching = $state(false);
  async function doSearch(e?: Event) {
    e?.preventDefault();
    searching = true;
    await docStore.runSearch(query, caseSensitive, wholeWord);
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
  export function findNext(dir: 1 | -1) {
    jump(dir);
  }

  // Comments: load annotations for every page when the panel opens.
  let commentsLoaded = -1;
  $effect(() => {
    const doc = docStore.doc;
    if (!doc || docStore.navPanel !== "comments" || commentsLoaded === doc.id) return;
    commentsLoaded = doc.id;
    (async () => {
      for (let i = 0; i < doc.page_count; i++) {
        if (docStore.doc?.id !== doc.id) return;
        await docStore.ensureAnnots(i);
      }
    })();
  });
  $effect(() => {
    if (!docStore.doc) commentsLoaded = -1;
  });
  const allComments = $derived.by(() =>
    Object.values(docStore.annots)
      .flat()
      .filter((a) => a.editable)
      .sort((a, b) => a.page_index - b.page_index || a.rect.y < b.rect.y ? 1 : -1),
  );
  const kindLabel: Record<string, string> = {
    text: "Note",
    freetext: "Text box",
    highlight: "Highlight",
    underline: "Underline",
    strikeout: "Strikethrough",
    squiggly: "Squiggly",
    ink: "Drawing",
    square: "Rectangle",
    circle: "Ellipse",
    stamp: "Stamp",
    line: "Line",
    polygon: "Polygon",
    polyline: "Polyline",
  };
  function fmtDate(d: string): string {
    const m = /^D:(\d{4})(\d{2})(\d{2})(\d{2})?(\d{2})?/.exec(d);
    if (!m) return d;
    return `${m[1]}-${m[2]}-${m[3]}${m[4] ? ` ${m[4]}:${m[5] ?? "00"}` : ""}`;
  }

  async function saveAttachment(index: number, name: string) {
    if (!docStore.doc) return;
    const path = await save({ defaultPath: name });
    if (!path) return;
    try {
      await api.saveAttachment(docStore.doc.id, index, path);
      docStore.showToast(`Saved ${name}`);
    } catch (e) {
      docStore.showToast(`Could not save attachment: ${e}`);
    }
  }

  const head = "border-b border-neutral-200 px-2 py-1 text-xs font-semibold uppercase text-neutral-500 dark:border-neutral-700";

  const railTabs: { id: Exclude<typeof docStore.navPanel, "none">; glyph: string; label: string }[] = [
    { id: "thumbnails", glyph: "▦", label: "Pages" },
    { id: "bookmarks", glyph: "☰", label: "Bookmarks" },
    { id: "search", glyph: "⌕", label: "Find (Ctrl+F)" },
    { id: "comments", glyph: "🗨", label: "Comments" },
    { id: "attachments", glyph: "📎", label: "Attachments" },
  ];
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
        {#if n.children.length}{@render outlineTree(n.children, depth + 1)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#if docStore.doc}
  <div class="flex h-full w-9 shrink-0 flex-col items-center gap-1 border-r border-neutral-300 bg-neutral-100 py-2 text-neutral-600 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-300">
    {#each railTabs as t}
      <button
        class="flex h-8 w-8 items-center justify-center rounded text-base hover:bg-neutral-200 dark:hover:bg-neutral-700 {docStore.navPanel === t.id ? 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-100' : ''}"
        title={t.label}
        aria-pressed={docStore.navPanel === t.id}
        onclick={() => (docStore.navPanel = docStore.navPanel === t.id ? "none" : t.id)}
      >{t.glyph}</button>
    {/each}
  </div>
{/if}

{#if docStore.doc && docStore.navPanel !== "none"}
  <aside class="flex h-full w-64 shrink-0 flex-col overflow-hidden border-r border-neutral-300 bg-neutral-50 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100">
    {#if docStore.navPanel === "thumbnails"}
      <div class={head}>Pages</div>
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
      <div class={head}>Bookmarks</div>
      <div class="flex-1 overflow-auto p-1">
        {#if docStore.outline.length}
          {@render outlineTree(docStore.outline, 0)}
        {:else}
          <p class="p-2 text-sm text-neutral-500">This document has no bookmarks.</p>
        {/if}
      </div>
    {:else if docStore.navPanel === "search"}
      <div class={head}>Find</div>
      <form class="flex flex-col gap-1 p-2" onsubmit={doSearch}>
        <div class="flex gap-1">
          <input id="sheaf-search-input" class="h-7 min-w-0 flex-1 rounded border border-neutral-300 bg-white px-1 text-sm dark:border-neutral-600 dark:bg-neutral-800" placeholder="Find in document" bind:value={query} />
          <button class="h-7 rounded bg-blue-600 px-2 text-sm text-white" type="submit" disabled={searching}>Go</button>
        </div>
        <div class="flex gap-3 text-xs text-neutral-600 dark:text-neutral-300">
          <label class="flex items-center gap-1"><input type="checkbox" bind:checked={caseSensitive} /> Match case</label>
          <label class="flex items-center gap-1"><input type="checkbox" bind:checked={wholeWord} /> Whole word</label>
        </div>
      </form>
      {#if docStore.searchHits.length}
        <div class="flex items-center gap-1 px-2 text-xs text-neutral-500">
          <button class="rounded px-1 hover:bg-neutral-200 dark:hover:bg-neutral-700" onclick={() => jump(-1)} title="Previous (Shift+F3)">&#9650;</button>
          <button class="rounded px-1 hover:bg-neutral-200 dark:hover:bg-neutral-700" onclick={() => jump(1)} title="Next (F3)">&#9660;</button>
          <span>{docStore.searchIndex + 1} of {docStore.searchHits.length}</span>
        </div>
      {:else if docStore.searchQuery && !searching}
        <p class="px-2 text-xs text-neutral-500">No matches.</p>
      {/if}
      <div class="flex-1 overflow-auto">
        {#each docStore.searchHits as h, i}
          <button
            class="block w-full border-b border-neutral-100 px-2 py-1 text-left text-xs hover:bg-neutral-200 dark:border-neutral-800 dark:hover:bg-neutral-700 {i === docStore.searchIndex ? 'bg-blue-100 dark:bg-blue-900' : ''}"
            onclick={() => ((docStore.searchIndex = i), onGoToPage(h.page_index))}
          >
            <span class="font-semibold">p.{h.page_index + 1}</span>
            <span class="text-neutral-600 dark:text-neutral-300">{h.context}</span>
          </button>
        {/each}
      </div>
    {:else if docStore.navPanel === "comments"}
      <div class="{head} flex items-center justify-between">
        <span>Comments ({allComments.length})</span>
      </div>
      <div class="flex-1 overflow-auto">
        {#if !allComments.length}
          <p class="p-2 text-sm text-neutral-500">No comments yet. Pick a tool above and mark up the page.</p>
        {/if}
        {#each allComments as a (a.page_index + ":" + a.index)}
          {@const sel = docStore.selected?.page === a.page_index && docStore.selected.index === a.index}
          <div
            class="cursor-pointer border-b border-neutral-200 px-2 py-1.5 text-xs hover:bg-neutral-200 dark:border-neutral-800 dark:hover:bg-neutral-700 {sel ? 'bg-blue-100 dark:bg-blue-900' : ''}"
            role="button"
            tabindex="0"
            onclick={() => {
              docStore.selected = { page: a.page_index, index: a.index };
              onGoToPage(a.page_index);
            }}
            ondblclick={() => onOpenNote(a)}
            onkeydown={(e) => e.key === "Enter" && onOpenNote(a)}
          >
            <div class="flex items-center gap-1">
              <span class="inline-block h-3 w-3 rounded-sm" style="background:{colorToCss(a.color, '#999')}"></span>
              <span class="font-semibold">{kindLabel[a.kind] ?? a.kind}</span>
              <span class="text-neutral-500">p.{a.page_index + 1}</span>
              <span class="flex-1"></span>
              <button class="rounded px-1 text-neutral-500 hover:bg-red-100 hover:text-red-700" title="Delete" onclick={(e) => (e.stopPropagation(), docStore.deleteAnnotation(a.page_index, a.index))}>&times;</button>
            </div>
            <div class="text-neutral-500">{a.author || "Unknown"}{a.modified ? ` · ${fmtDate(a.modified)}` : ""}</div>
            {#if a.contents}<div class="mt-0.5 whitespace-pre-wrap">{a.contents}</div>{/if}
          </div>
        {/each}
      </div>
    {:else if docStore.navPanel === "attachments"}
      <div class={head}>Attachments</div>
      <div class="flex-1 overflow-auto">
        {#if !docStore.doc.attachments.length}
          <p class="p-2 text-sm text-neutral-500">No attachments.</p>
        {/if}
        {#each docStore.doc.attachments as at}
          <div class="flex items-center gap-2 border-b border-neutral-200 px-2 py-1 text-xs dark:border-neutral-800">
            <span class="flex-1 truncate" title={at.name}>{at.name}</span>
            <span class="text-neutral-500">{(at.size / 1024).toFixed(1)} KB</span>
            <button class="rounded bg-blue-600 px-2 py-0.5 text-white" onclick={() => saveAttachment(at.index, at.name)}>Save</button>
          </div>
        {/each}
      </div>
    {/if}
  </aside>
{/if}
