<script lang="ts">
  import { docStore, type Tool } from "$lib/stores/document.svelte";
  import { colorToHex, hexToColor } from "$lib/viewer/geometry";
  import Menu from "./Menu.svelte";

  interface Props {
    onGoToPage: (index: number) => void;
    onOpen: () => void;
    onSave: () => void;
    onSaveAs: () => void;
    onPrint: () => void;
    onProperties: () => void;
    onExportForm: () => void;
    onImportForm: () => void;
    onValidateForm: () => void;
    onOrganize: () => void;
  }
  let { onGoToPage, onOpen, onSave, onSaveAs, onPrint, onProperties, onExportForm, onImportForm, onValidateForm, onOrganize }: Props = $props();

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
  const item =
    "flex w-full items-center justify-between gap-6 whitespace-nowrap px-3 py-1.5 text-left text-sm hover:bg-neutral-100 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-800";
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

  // The annotation bar opens on demand, and also whenever an annotation tool
  // is chosen elsewhere (keyboard shortcut) so the style controls are visible.
  let annotateOpen = $state(false);
  $effect(() => {
    if (hasDoc && !["select", "hand"].includes(docStore.tool)) annotateOpen = true;
  });
  $effect(() => {
    if (!hasDoc) annotateOpen = false;
  });
  function toggleAnnotate() {
    annotateOpen = !annotateOpen;
    if (!annotateOpen) docStore.tool = "select";
  }
</script>

<div class="border-b border-neutral-300 bg-neutral-100 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100">
  <div class="flex h-11 items-center gap-1 overflow-x-auto px-2" role="toolbar" aria-label="Main toolbar">
    <Menu label="File">
      {#snippet children(close)}
        <button class={item} role="menuitem" onclick={() => (close(), onOpen())}>Open… <span class="text-xs text-neutral-400">Ctrl+O</span></button>
        <button class={item} role="menuitem" disabled={!hasDoc || !docStore.doc?.modified} onclick={() => (close(), onSave())}>Save <span class="text-xs text-neutral-400">Ctrl+S</span></button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), onSaveAs())}>Save As… <span class="text-xs text-neutral-400">Ctrl+Shift+S</span></button>
        <div class="my-1 h-px bg-neutral-200 dark:bg-neutral-700"></div>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), onPrint())}>Print… <span class="text-xs text-neutral-400">Ctrl+P</span></button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), onProperties())}>Properties… <span class="text-xs text-neutral-400">Ctrl+D</span></button>
        <div class="my-1 h-px bg-neutral-200 dark:bg-neutral-700"></div>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), onValidateForm())}>Validate form</button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), onImportForm())}>Import form data…</button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), onExportForm())}>Export form data…</button>
      {/snippet}
    </Menu>

    <Menu label="View">
      {#snippet children(close)}
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), docStore.setViewMode("continuous"))}>{docStore.viewMode === "continuous" ? "✓" : "\u00a0\u00a0"} Continuous</button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), docStore.setViewMode("single"))}>{docStore.viewMode === "single" ? "✓" : "\u00a0\u00a0"} Single page</button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), docStore.setViewMode("twoup"))}>{docStore.viewMode === "twoup" ? "✓" : "\u00a0\u00a0"} Two-up</button>
        <div class="my-1 h-px bg-neutral-200 dark:bg-neutral-700"></div>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), docStore.rotateView(-90))}>Rotate view left <span class="text-xs text-neutral-400">Ctrl+Shift+-</span></button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), docStore.rotateView(90))}>Rotate view right <span class="text-xs text-neutral-400">Ctrl+Shift++</span></button>
        <div class="my-1 h-px bg-neutral-200 dark:bg-neutral-700"></div>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), (docStore.nightMode = !docStore.nightMode))}>{docStore.nightMode ? "✓" : "\u00a0\u00a0"} Night mode</button>
        <button class={item} role="menuitem" disabled={!hasDoc} onclick={() => (close(), (docStore.formMode = !docStore.formMode))}>{docStore.formMode ? "✓" : "\u00a0\u00a0"} Form fields</button>
        <button class={item} role="menuitem" onclick={() => (close(), docStore.setTheme(docStore.theme === "dark" ? "light" : "dark"))}>{docStore.theme === "dark" ? "Light theme" : "Dark theme"}</button>
      {/snippet}
    </Menu>

    <span class={sep}></span>
    <button class={btn} disabled={!docStore.doc?.can_undo} onclick={() => docStore.undo()} title="Undo (Ctrl+Z)">&#8630;</button>
    <button class={btn} disabled={!docStore.doc?.can_redo} onclick={() => docStore.redo()} title="Redo (Ctrl+Y)">&#8631;</button>
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

    <button
      class="{btn} {annotateOpen ? 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-100' : ''}"
      disabled={!hasDoc}
      onclick={toggleAnnotate}
      aria-pressed={annotateOpen}
      title="Show or hide annotation tools"
    >Annotate</button>
    <button class={btn} disabled={!hasDoc} onclick={onOrganize} title="Reorder, rotate, delete, insert, extract, crop, and stamp pages">Organize</button>

    <span class="flex-1"></span>
    {#if docStore.doc}
      <button class="truncate text-sm text-neutral-500 hover:underline" title="Document properties (Ctrl+D)" onclick={onProperties}>
        {docStore.doc.modified ? "* " : ""}{docStore.doc.file_name}
      </button>
    {/if}
  </div>

  {#if annotateOpen && hasDoc}
    <div class="flex h-10 items-center gap-1 border-t border-neutral-200 px-2 dark:border-neutral-800" role="toolbar" aria-label="Annotation tools">
      {#each tools as t}
        <button class={toolBtn(t.id)} onclick={() => (docStore.tool = t.id)} title={t.title} aria-pressed={docStore.tool === t.id}>{t.label}</button>
      {/each}
      {#if styleable}
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
  {/if}
</div>
