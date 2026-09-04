<script lang="ts">
  // M6: content editing side panel. Lists the objects on the current page,
  // edits text runs, nudges/scales/deletes objects, inserts images, adds links.
  import { docStore } from "$lib/stores/document.svelte";
  import { api, errorMessage, type LinkInfo, type PageObject } from "$lib/api";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  const doc = $derived(docStore.doc);
  const page = $derived(docStore.currentPage);
  let objects = $state<PageObject[]>([]);
  let links = $state<LinkInfo[]>([]);
  let selected = $state<number | null>(null);
  let filter = $state<"all" | "text" | "image">("all");
  /** "text": edit paragraphs directly on the page. "objects": raw object list. */
  let mode = $state<"text" | "objects">("text");
  $effect(() => {
    docStore.editMode = mode === "text" && !!doc;
    return () => {
      docStore.editMode = false;
      docStore.editingBlock = null;
    };
  });
  const blockCount = $derived((docStore.textBlocks[page] ?? []).length);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let editText = $state("");
  let editSize = $state<number | null>(null);
  let nudge = $state(5);
  let linkUrl = $state("");
  let linkPage = $state("");

  async function run(fn: () => Promise<void>, refresh = true) {
    busy = true;
    error = null;
    try {
      await fn();
      if (refresh) await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function load() {
    if (!doc) return;
    const id = doc.id;
    const [o, l] = await Promise.all([api.listPageObjects(id, page), api.listLinks(id, page)]);
    if (docStore.doc?.id !== id) return;
    objects = o;
    links = l;
    if (selected != null && !objects.some((x) => x.index === selected)) selected = null;
  }
  $effect(() => {
    void doc?.id;
    void page;
    void docStore.renderVersion;
    void load().catch((e) => (error = errorMessage(e)));
  });

  const shown = $derived(objects.filter((o) => filter === "all" || o.kind === filter));
  const sel = $derived(selected == null ? null : (objects.find((o) => o.index === selected) ?? null));
  $effect(() => {
    if (sel?.kind === "text") {
      editText = sel.text ?? "";
      editSize = sel.font_size;
    }
  });
  $effect(() => {
    // Mirror selection onto the page so the highlight box follows.
    docStore.editHighlight = sel ? { page, rect: sel.rect } : null;
    return () => {
      docStore.editHighlight = null;
    };
  });

  const apply = (info: Awaited<ReturnType<typeof api.setTextObject>>) => {
    docStore.doc = info;
    docStore.texts = {};
    docStore.invalidateRenders();
  };

  const saveText = () =>
    run(async () => {
      if (!doc || sel?.kind !== "text") return;
      apply(await api.setTextObject(doc.id, page, sel.index, editText, editSize));
    });
  const move = (dx: number, dy: number, scale = 1) =>
    run(async () => {
      if (!doc || !sel) return;
      apply(await api.movePageObject(doc.id, page, sel.index, dx, dy, scale));
    });
  const del = () =>
    run(async () => {
      if (!doc || !sel) return;
      apply(await api.deletePageObject(doc.id, page, sel.index));
      selected = null;
    });
  const extract = () =>
    run(async () => {
      if (!doc || sel?.kind !== "image") return;
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ defaultPath: `image-${page + 1}-${sel.index}.png`, filters: [{ name: "PNG image", extensions: ["png"] }] });
      if (!path) return;
      await api.extractImage(doc.id, page, sel.index, path);
      docStore.showToast("Image saved");
    }, false);

  let placingImage = $state<string | null>(null);
  const pickImage = () =>
    run(async () => {
      const picked = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff"] }] });
      if (!picked || Array.isArray(picked)) return;
      placingImage = picked;
      docStore.tool = "select";
      docStore.placingSignature = (pg, r) => {
        const path = placingImage;
        placingImage = null;
        if (!path) return;
        void run(async () => {
          if (!doc) return;
          apply(await api.insertImage(doc.id, pg, path, { x: r.x, y: r.y, w: r.w, h: 0 }));
        });
      };
    }, false);
  function cancelPlacing() {
    placingImage = null;
    placingLink = false;
    placingText = false;
    docStore.placingSignature = null;
  }

  let newText = $state("");
  let newTextSize = $state(12);
  let placingText = $state(false);
  function startText() {
    if (!newText.trim()) {
      error = "Type the text to add first.";
      return;
    }
    error = null;
    placingText = true;
    docStore.tool = "select";
    docStore.placingSignature = (pgIdx, r) => {
      placingText = false;
      void run(async () => {
        if (!doc) return;
        apply(await api.addText(doc.id, pgIdx, newText, r.x, r.y, newTextSize));
      });
    };
  }

  let placingLink = $state(false);
  function startLink() {
    const pg = parseInt(linkPage, 10);
    if (!linkUrl.trim() && Number.isNaN(pg)) {
      error = "Enter a URL or a page number for the link.";
      return;
    }
    error = null;
    placingLink = true;
    docStore.tool = "select";
    docStore.placingSignature = (pgIdx, r) => {
      placingLink = false;
      void run(async () => {
        if (!doc) return;
        apply(await api.addLink(doc.id, pgIdx, r, linkUrl.trim() || null, Number.isNaN(pg) ? null : pg - 1));
      });
    };
  }

  const btn =
    "inline-flex h-8 items-center justify-center whitespace-nowrap rounded px-2.5 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700";
  const primary = "inline-flex h-8 items-center justify-center whitespace-nowrap rounded bg-blue-600 px-3 text-sm text-white hover:bg-blue-700 disabled:opacity-40";
  const field = "h-8 w-full rounded border border-neutral-300 bg-white px-2 text-sm dark:border-neutral-600 dark:bg-neutral-800";
  const chip = (on: boolean) => `rounded px-2 py-0.5 text-xs ${on ? "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-100" : "text-neutral-500 hover:bg-neutral-200 dark:hover:bg-neutral-700"}`;
  const short = (s: string | null, n = 40) => (s ?? "").replace(/\s+/g, " ").trim().slice(0, n) || "(empty)";
</script>

<aside class="flex w-80 shrink-0 flex-col border-l border-neutral-300 bg-neutral-50 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100" aria-label="Edit content" data-panel="edit">
  <div class="flex h-11 shrink-0 items-center border-b border-neutral-300 px-2 dark:border-neutral-700">
    <span class="flex-1 text-sm font-semibold">Edit page {page + 1}</span>
    <button class={btn} onclick={() => (cancelPlacing(), onClose())} aria-label="Close">✕</button>
  </div>

  {#if error}
    <div class="m-2 rounded bg-red-100 px-2 py-1 text-xs text-red-800 dark:bg-red-900 dark:text-red-100" role="alert">{error}</div>
  {/if}

  <div class="flex shrink-0 items-center gap-1 border-b border-neutral-200 px-2 py-1.5 dark:border-neutral-800" role="tablist">
    <button class={chip(mode === "text")} role="tab" aria-selected={mode === "text"} onclick={() => (mode = "text")}>Text</button>
    <button class={chip(mode === "objects")} role="tab" aria-selected={mode === "objects"} onclick={() => (mode = "objects")}>Objects {objects.length}</button>
    <span class="flex-1"></span>
    {#if placingImage || placingLink || placingText}
      <button class="{btn} h-6 text-xs" onclick={cancelPlacing}>Cancel</button>
    {:else}
      <button class="{btn} h-6 text-xs" disabled={busy || !doc} onclick={pickImage} title="Insert an image: pick a file, then drag where it goes">+ Image</button>
    {/if}
  </div>
  {#if placingImage}
    <div class="m-2 rounded bg-blue-50 p-2 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">Drag on the page where the image should go (width sets the size).</div>
  {:else if placingLink}
    <div class="m-2 rounded bg-blue-50 p-2 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">Drag on the page over the area that should be clickable.</div>
  {:else if placingText}
    <div class="m-2 rounded bg-blue-50 p-2 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">Drag a small box on the page; the text starts at its bottom-left corner.</div>
  {/if}

  {#if mode === "text"}
    <div class="min-h-0 flex-1 overflow-y-auto p-3 text-sm">
      <p class="mb-2">Click any paragraph on the page to edit it in place. Text rewraps to the paragraph's width; the font, size, colour and line spacing are kept.</p>
      <ul class="mb-3 list-disc space-y-1 pl-5 text-xs text-neutral-600 dark:text-neutral-300">
        <li><kbd>Ctrl</kbd>+<kbd>Enter</kbd> or click elsewhere applies; <kbd>Esc</kbd> cancels.</li>
        <li>Blank line in the editor starts a new paragraph line.</li>
        <li>Characters the original font lacks fall back to Helvetica.</li>
        <li>Undo (<kbd>Ctrl</kbd>+<kbd>Z</kbd>) reverts an edit.</li>
      </ul>
      <div class="text-xs text-neutral-500">{blockCount} paragraph{blockCount === 1 ? "" : "s"} on this page.</div>
      <div class="mt-3 border-t border-neutral-200 pt-3 dark:border-neutral-800">
        <div class="mb-1 text-xs font-semibold text-neutral-500">Add new text</div>
        <div class="flex gap-1">
          <input class={field} placeholder="New text…" bind:value={newText} />
          <input type="number" min="4" max="144" class="{field} w-16" bind:value={newTextSize} aria-label="Font size" />
          <button class="{btn} shrink-0" disabled={busy || placingText || !doc} onclick={startText} title="Add a Helvetica text run where you drag">+ Text</button>
        </div>
      </div>
    </div>
  {:else}
  <div class="flex shrink-0 items-center gap-1 border-b border-neutral-200 px-2 py-1.5 dark:border-neutral-800">
    <button class={chip(filter === "all")} onclick={() => (filter = "all")}>All {objects.length}</button>
    <button class={chip(filter === "text")} onclick={() => (filter = "text")}>Text {objects.filter((o) => o.kind === "text").length}</button>
    <button class={chip(filter === "image")} onclick={() => (filter = "image")}>Images {objects.filter((o) => o.kind === "image").length}</button>
  </div>
  <ul class="min-h-0 flex-1 overflow-y-auto text-xs" role="listbox" aria-label="Page objects">
    {#each shown as o (o.index)}
      <li>
        <button
          class="flex w-full items-center gap-2 px-2 py-1 text-left hover:bg-neutral-200 dark:hover:bg-neutral-800 {selected === o.index ? 'bg-blue-100 dark:bg-blue-900' : ''}"
          role="option"
          aria-selected={selected === o.index}
          onclick={() => (selected = selected === o.index ? null : o.index)}
        >
          <span class="w-10 shrink-0 rounded bg-neutral-200 px-1 text-center text-[10px] uppercase text-neutral-600 dark:bg-neutral-700 dark:text-neutral-300">{o.kind}</span>
          <span class="min-w-0 flex-1 truncate">
            {#if o.kind === "text"}{short(o.text)}{:else if o.kind === "image"}{o.image_width}×{o.image_height} px{:else}{Math.round(o.rect.w)}×{Math.round(o.rect.h)} pt{/if}
          </span>
        </button>
      </li>
    {/each}
    {#if !shown.length}
      <li class="px-2 py-3 text-neutral-500">Nothing to show on this page.</li>
    {/if}
  </ul>

  {#if sel}
    <div class="shrink-0 space-y-2 border-t border-neutral-300 p-2 text-sm dark:border-neutral-700">
      {#if sel.kind === "text"}
        <div class="text-xs text-neutral-500">{sel.font ?? "font"} · {sel.font_size?.toFixed(1)} pt</div>
        <textarea class="{field} h-16 resize-y py-1" bind:value={editText} spellcheck="true"></textarea>
        <div class="flex items-center gap-2">
          <label class="flex items-center gap-1 text-xs">Size <input type="number" min="1" max="200" step="0.5" class="{field} w-20" bind:value={editSize} /></label>
          <span class="flex-1"></span>
          <button class={primary} disabled={busy} onclick={saveText}>Apply text</button>
        </div>
      {:else if sel.kind === "image"}
        <div class="text-xs text-neutral-500">{sel.image_width}×{sel.image_height} px, placed {Math.round(sel.rect.w)}×{Math.round(sel.rect.h)} pt</div>
        <button class="{btn} w-full border border-neutral-300 dark:border-neutral-600" disabled={busy} onclick={extract}>Save image as PNG…</button>
      {/if}
      <div class="flex items-center gap-1">
        <span class="text-xs text-neutral-500">Move</span>
        <input type="number" min="1" max="200" class="{field} w-14" bind:value={nudge} aria-label="Nudge distance in points" />
        <button class={btn} disabled={busy} onclick={() => move(-nudge, 0)} title="Left">←</button>
        <button class={btn} disabled={busy} onclick={() => move(nudge, 0)} title="Right">→</button>
        <button class={btn} disabled={busy} onclick={() => move(0, nudge)} title="Up">↑</button>
        <button class={btn} disabled={busy} onclick={() => move(0, -nudge)} title="Down">↓</button>
        <span class="flex-1"></span>
        <button class={btn} disabled={busy} onclick={() => move(0, 0, 0.9)} title="Smaller">−</button>
        <button class={btn} disabled={busy} onclick={() => move(0, 0, 1.1)} title="Larger">+</button>
        <button class="{btn} text-red-700 dark:text-red-300" disabled={busy} onclick={del} title="Delete object">🗑</button>
      </div>
    </div>
  {/if}

  {/if}

  <div class="shrink-0 space-y-1 border-t border-neutral-300 p-2 text-sm dark:border-neutral-700">
    <div class="text-xs font-semibold text-neutral-500">Links on this page: {links.length}</div>
    {#each links.slice(0, 4) as l}
      <div class="truncate text-xs text-neutral-600 dark:text-neutral-300" title={l.uri ?? `Page ${(l.page ?? 0) + 1}`}>{l.uri ?? `→ page ${(l.page ?? 0) + 1}`}</div>
    {/each}
    <div class="flex gap-1">
      <input class={field} placeholder="https://… " bind:value={linkUrl} />
      <input class="{field} w-16" placeholder="Page" bind:value={linkPage} />
    </div>
    <button class="{btn} w-full border border-neutral-300 dark:border-neutral-600" disabled={busy || placingLink || !doc} onclick={startLink}>Add link (drag area)…</button>
  </div>
</aside>
