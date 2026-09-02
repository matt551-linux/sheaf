<script lang="ts">
  import { docStore, type Tool } from "$lib/stores/document.svelte";
  import { colorToHex, hexToColor } from "$lib/viewer/geometry";

  interface Props {
    onGoToPage: (index: number) => void;
    onOpen: () => void;
    onSave: () => void;
    onSaveAs: () => void;
    onPrint: () => void;
    onProperties: () => void;
  }
  let { onGoToPage, onOpen, onSave, onSaveAs, onPrint, onProperties }: Props = $props();

  let pageInput = $state("1");
  $effect(() => {
    pageInput = String(docStore.currentPage + 1);
  });

  function submitPage(e: Event) {
    e.preventDefault();
    const n = parseInt(pageInput, 10);
    if (!docStore.doc || Number.isNaN(n)) return;
    onGoToPage(Math.min(docStore.doc.page_count, Math.max(1, n)) - 1);
  }

  const zoomPct = $derived(Math.round(docStore.zoom * 100));
  const hasDoc = $derived(!!docStore.doc);
  const presets = [50, 75, 100, 125, 150, 200, 300, 400];

  const btn =
    "inline-flex h-8 min-w-8 shrink-0 items-center justify-center whitespace-nowrap rounded px-2 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700";
  const toolBtn = (t: Tool) =>
    `${btn} ${docStore.tool === t ? "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-100" : ""}`;
  const sep = "mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700";

  const tools: { id: Tool; label: string; title: string }[] = [
    { id: "select", label: "Select", title: "Select text and annotations (V)" },
    { id: "hand", label: "Hand", title: "Pan (H)" },
    { id: "highlight", label: "Highlight", title: "Highlight text (U)" },
    { id: "underline", label: "Underline", title: "Underline text" },
    { id: "strikeout", label: "Strike", title: "Strikethrough text" },
    { id: "squiggly", label: "Squiggle", title: "Squiggly underline" },
    { id: "note", label: "Note", title: "Sticky note (N)" },
    { id: "freetext", label: "Text box", title: "Add text box (T)" },
    { id: "ink", label: "Pen", title: "Draw freehand (P)" },
    { id: "square", label: "Rect", title: "Rectangle (R)" },
    { id: "circle", label: "Ellipse", title: "Ellipse (E)" },
    { id: "eraser", label: "Eraser", title: "Delete annotations by clicking" },
  ];
  const styleable = $derived(!["select", "hand", "eraser"].includes(docStore.tool));
  const showWidth = $derived(["ink", "square", "circle"].includes(docStore.tool));
  const showFill = $derived(["square", "circle"].includes(docStore.tool));
  const showFont = $derived(docStore.tool === "freetext");
  const panel = (p: typeof docStore.navPanel) => (docStore.navPanel = docStore.navPanel === p ? "none" : p);
</script>

<div class="border-b border-neutral-300 bg-neutral-100 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100">
  <div class="flex h-11 items-center gap-1 overflow-x-auto px-2" role="toolbar" aria-label="Main toolbar">
    <button class={btn} onclick={onOpen} title="Open (Ctrl+O)">Open</button>
    <button class={btn} disabled={!hasDoc || !docStore.doc?.modified} onclick={onSave} title="Save (Ctrl+S)">Save</button>
    <button class={btn} disabled={!hasDoc} onclick={onSaveAs} title="Save As (Ctrl+Shift+S)">Save As</button>
    <button class={btn} disabled={!hasDoc} onclick={onPrint} title="Print (Ctrl+P)">Print</button>
    <span class={sep}></span>
    <button class={btn} disabled={!docStore.doc?.can_undo} onclick={() => docStore.undo()} title="Undo (Ctrl+Z)">Undo</button>
    <button class={btn} disabled={!docStore.doc?.can_redo} onclick={() => docStore.redo()} title="Redo (Ctrl+Y)">Redo</button>
    <span class={sep}></span>
    <button class={btn} disabled={!hasDoc} onclick={() => panel("thumbnails")} aria-pressed={docStore.navPanel === "thumbnails"} title="Page thumbnails">Pages</button>
    <button class={btn} disabled={!hasDoc} onclick={() => panel("bookmarks")} aria-pressed={docStore.navPanel === "bookmarks"} title="Bookmarks">Bookmarks</button>
    <button class={btn} disabled={!hasDoc} onclick={() => panel("comments")} aria-pressed={docStore.navPanel === "comments"} title="Comments">Comments</button>
    <button class={btn} disabled={!hasDoc} onclick={() => panel("search")} aria-pressed={docStore.navPanel === "search"} title="Find (Ctrl+F)">Find</button>
    <span class={sep}></span>
    <button class={btn} disabled={!hasDoc || docStore.currentPage === 0} onclick={() => onGoToPage(docStore.currentPage - 1)} title="Previous page">&#9650;</button>
    <form onsubmit={submitPage} class="flex items-center gap-1 text-sm">
      <input class="h-7 w-12 rounded border border-neutral-300 bg-white px-1 text-center dark:border-neutral-600 dark:bg-neutral-800" bind:value={pageInput} disabled={!hasDoc} aria-label="Page number" />
      <span class="text-neutral-500">/ {docStore.doc?.page_count ?? 0}</span>
    </form>
    <button class={btn} disabled={!hasDoc || !docStore.doc || docStore.currentPage >= docStore.doc.page_count - 1} onclick={() => onGoToPage(docStore.currentPage + 1)} title="Next page">&#9660;</button>
    <span class={sep}></span>
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
      {#each presets as p}<option value={String(p)}>{p}%</option>{/each}
      {#if docStore.fitMode === "custom" && !presets.includes(zoomPct)}<option value={String(zoomPct)}>{zoomPct}%</option>{/if}
    </select>
    <button class={btn} disabled={!hasDoc} onclick={() => docStore.zoomIn()} title="Zoom in (Ctrl++)">+</button>
    <span class={sep}></span>
    <select
      class="h-7 rounded border border-neutral-300 bg-white px-1 text-sm dark:border-neutral-600 dark:bg-neutral-800"
      disabled={!hasDoc}
      value={docStore.viewMode}
      onchange={(e) => docStore.setViewMode((e.currentTarget as HTMLSelectElement).value as typeof docStore.viewMode)}
      aria-label="Page layout"
    >
      <option value="continuous">Continuous</option>
      <option value="single">Single page</option>
      <option value="twoup">Two-up</option>
    </select>
    <button class={btn} disabled={!hasDoc} onclick={() => docStore.rotateView(-90)} title="Rotate view counterclockwise (Ctrl+Shift+-)">&#8634;</button>
    <button class={btn} disabled={!hasDoc} onclick={() => docStore.rotateView(90)} title="Rotate view clockwise (Ctrl+Shift++)">&#8635;</button>
    <button class="{btn} {docStore.nightMode ? 'bg-neutral-300 dark:bg-neutral-700' : ''}" disabled={!hasDoc} onclick={() => (docStore.nightMode = !docStore.nightMode)} title="Night mode (invert page colors)">Night</button>
    <button class={btn} onclick={() => docStore.setTheme(docStore.theme === "dark" ? "light" : "dark")} title="Toggle app theme">{docStore.theme === "dark" ? "Light" : "Dark"}</button>
    <span class="flex-1"></span>
    {#if docStore.doc}
      <button class="truncate text-sm text-neutral-500 hover:underline" title="Document properties (Ctrl+D)" onclick={onProperties}>
        {docStore.doc.modified ? "* " : ""}{docStore.doc.file_name}
      </button>
    {/if}
  </div>

  <div class="flex h-10 items-center gap-1 border-t border-neutral-200 px-2 dark:border-neutral-800" role="toolbar" aria-label="Tools">
    {#each tools as t}
      <button class={toolBtn(t.id)} disabled={!hasDoc} onclick={() => (docStore.tool = t.id)} title={t.title} aria-pressed={docStore.tool === t.id}>{t.label}</button>
    {/each}
    {#if styleable && hasDoc}
      <span class={sep}></span>
      <label class="flex items-center gap-1 text-xs text-neutral-600 dark:text-neutral-300">
        Color
        <input type="color" class="h-6 w-8 cursor-pointer border-0 bg-transparent p-0" value={colorToHex(docStore.styles[docStore.tool].color)} oninput={(e) => docStore.setStyle(docStore.tool, { color: hexToColor((e.currentTarget as HTMLInputElement).value) })} />
      </label>
      {#if showFill}
        <label class="flex items-center gap-1 text-xs text-neutral-600 dark:text-neutral-300">
          Fill
          <input type="checkbox" checked={!!docStore.styles[docStore.tool].interior} onchange={(e) => docStore.setStyle(docStore.tool, { interior: (e.currentTarget as HTMLInputElement).checked ? { r: 255, g: 255, b: 255 } : null })} />
          {#if docStore.styles[docStore.tool].interior}
            <input type="color" class="h-6 w-8 cursor-pointer border-0 bg-transparent p-0" value={colorToHex(docStore.styles[docStore.tool].interior)} oninput={(e) => docStore.setStyle(docStore.tool, { interior: hexToColor((e.currentTarget as HTMLInputElement).value) })} />
          {/if}
        </label>
      {/if}
      {#if showWidth}
        <label class="flex items-center gap-1 text-xs text-neutral-600 dark:text-neutral-300">
          Width
          <input type="range" min="0.5" max="12" step="0.5" class="w-20" value={docStore.styles[docStore.tool].width} oninput={(e) => docStore.setStyle(docStore.tool, { width: parseFloat((e.currentTarget as HTMLInputElement).value) })} />
          <span class="w-6">{docStore.styles[docStore.tool].width}</span>
        </label>
      {/if}
      {#if showFont}
        <label class="flex items-center gap-1 text-xs text-neutral-600 dark:text-neutral-300">
          Size
          <input type="number" min="6" max="72" class="h-6 w-14 rounded border border-neutral-300 px-1 dark:border-neutral-600 dark:bg-neutral-800" value={docStore.styles.freetext.fontSize} oninput={(e) => docStore.setStyle("freetext", { fontSize: parseFloat((e.currentTarget as HTMLInputElement).value) || 12 })} />
        </label>
      {/if}
    {/if}
    <span class="flex-1"></span>
    <label class="flex items-center gap-1 text-xs text-neutral-600 dark:text-neutral-300">
      Author
      <input class="h-6 w-28 rounded border border-neutral-300 px-1 dark:border-neutral-600 dark:bg-neutral-800" value={docStore.author} placeholder="Your name" onchange={(e) => docStore.setAuthor((e.currentTarget as HTMLInputElement).value)} />
    </label>
  </div>
</div>
