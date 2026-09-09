<script lang="ts">
  // In-place paragraph editor. Overlays the page in edit mode: hover shows
  // paragraph outlines, click opens a formatting toolbar plus a textarea
  // over the paragraph with the same font size and leading. Ctrl+Enter or
  // clicking away commits; Escape cancels. Reflow happens in the engine
  // (word wrap to the block's width), then the page re-renders.
  import { docStore } from "$lib/stores/document.svelte";
  import { errorMessage, type BlockEdit, type Color, type TextBlock } from "$lib/api";
  import { rectToCss, pxPerPt, colorToCss, colorToHex, hexToColor } from "$lib/viewer/geometry";

  interface Props {
    index: number;
    size: { width: number; height: number };
    zoom: number;
    rot: number;
  }
  let { index, size, zoom, rot }: Props = $props();

  const blocks = $derived(docStore.textBlocks[index] ?? []);
  const editing = $derived(docStore.editingBlock?.page === index ? (blocks.find((b) => b.id === docStore.editingBlock!.id) ?? null) : null);
  const scale = $derived(pxPerPt(zoom));

  $effect(() => {
    if (docStore.editMode && docStore.doc) void docStore.ensureBlocks(index).catch(() => {});
  });

  let draft = $state("");
  let ta = $state<HTMLTextAreaElement | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Formatting state for the block currently open. Seeded from the block's
  // inferred style on open; only the fields the user actually touches are
  // sent as overrides so untouched runs keep their embedded font when
  // possible (see textedit.rs force_standard).
  let fmtBold = $state(false);
  let fmtItalic = $state(false);
  let fmtUnderline = $state(false);
  let fmtAlign = $state<"left" | "center" | "right">("left");
  let fmtFamily = $state<"Helvetica" | "Times" | "Courier">("Helvetica");
  let fmtSize = $state(12);
  let fmtColor = $state<Color>({ r: 0, g: 0, b: 0 });
  // Track which formatting fields the user actively changed this session,
  // so we only override what was touched.
  let touched = $state<Set<string>>(new Set());
  // Per-selection style overrides (mixed-run formatting): when the user
  // selects a range of text and hits a toolbar button, the override applies
  // only to that range instead of the whole paragraph. Ranges are UTF-16
  // code-unit offsets into `draft`, same indexing the textarea uses.
  let runs = $state<import("$lib/api").RunStyle[]>([]);

  function begin(b: TextBlock) {
    if (busy) return;
    draft = b.text;
    fmtBold = b.bold;
    fmtItalic = b.italic;
    fmtUnderline = false;
    fmtAlign = "left";
    fmtFamily = /times|serif/i.test(b.font) ? "Times" : /courier|mono/i.test(b.font) ? "Courier" : "Helvetica";
    fmtSize = Math.round(b.font_size * 10) / 10;
    fmtColor = { ...b.color };
    touched = new Set();
    runs = [];
    docStore.editingBlock = { page: index, id: b.id };
    queueMicrotask(() => {
      ta?.focus();
      ta?.setSelectionRange(draft.length, draft.length);
    });
  }

  function touch(field: string) {
    touched = new Set(touched).add(field);
  }

  // Merge a style patch into whatever range is currently selected in the
  // textarea. Overlapping existing runs are split/trimmed so ranges never
  // overlap (the engine assumes non-overlapping, left-to-right runs).
  function applyRunPatch(patch: Partial<import("$lib/api").RunStyle>) {
    const start = ta?.selectionStart ?? 0;
    const end = ta?.selectionEnd ?? 0;
    if (end <= start) return false;
    const kept: import("$lib/api").RunStyle[] = [];
    for (const r of runs) {
      if (r.end <= start || r.start >= end) {
        kept.push(r);
        continue;
      }
      if (r.start < start) kept.push({ ...r, end: start });
      if (r.end > end) kept.push({ ...r, start: end });
    }
    kept.push({ start, end, ...patch });
    kept.sort((a, b) => a.start - b.start);
    runs = kept;
    return true;
  }

  function toggleBold() {
    if (applyRunPatch({ bold: true })) return; // caller flips per-run below
    fmtBold = !fmtBold;
    touch("bold");
  }
  function toggleItalic() {
    if (applyRunPatch({ italic: true })) return;
    fmtItalic = !fmtItalic;
    touch("italic");
  }
  function toggleUnderline() {
    if (applyRunPatch({ underline: true })) return;
    fmtUnderline = !fmtUnderline;
    touch("underline");
  }
  function setAlign(a: "left" | "center" | "right") {
    fmtAlign = a;
    touch("align");
  }
  function setFamily(f: "Helvetica" | "Times" | "Courier") {
    if (applyRunPatch({ font_family: f })) return;
    fmtFamily = f;
    touch("family");
  }
  function setSize(s: number) {
    fmtSize = s;
    touch("size");
  }
  function setColor(c: Color) {
    if (applyRunPatch({ color: c })) return;
    fmtColor = c;
    touch("color");
  }

  async function commit() {
    const b = editing;
    if (!b || busy) return;
    const edit: BlockEdit = { id: b.id, text: draft };
    if (touched.has("bold") || touched.has("italic") || touched.has("family")) {
      edit.bold = fmtBold;
      edit.italic = fmtItalic;
      edit.font_family = fmtFamily;
    }
    if (touched.has("underline")) edit.underline = fmtUnderline;
    if (touched.has("align")) edit.align = fmtAlign;
    if (touched.has("size")) edit.font_size = fmtSize;
    if (touched.has("color")) edit.color = fmtColor;
    if (runs.length > 0) edit.runs = runs;
    if (draft === b.text && touched.size === 0 && runs.length === 0) {
      docStore.editingBlock = null;
      return;
    }
    busy = true;
    error = null;
    try {
      await docStore.commitBlock(index, edit);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  function cancel() {
    docStore.editingBlock = null;
    error = null;
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void commit();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b") {
      e.preventDefault();
      toggleBold();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "i") {
      e.preventDefault();
      toggleItalic();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "u") {
      e.preventDefault();
      toggleUnderline();
    }
    e.stopPropagation();
  }

  // Editor box: the block rect plus room for a couple of extra lines so
  // typing more does not immediately clip.
  const box = $derived(editing ? rectToCss(editing.rect, size, zoom, rot) : null);
  const fontPx = $derived(fmtSize * scale);
  const leadPx = $derived(editing ? (fmtSize / editing.font_size) * editing.leading * scale : 14);

  const tbtn = (on: boolean) =>
    `flex h-6 w-6 items-center justify-center rounded text-xs font-semibold ${on ? "bg-blue-600 text-white" : "text-neutral-200 hover:bg-neutral-700"}`;
</script>

{#if docStore.editMode}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="absolute inset-0" onpointerdown={(e) => e.stopPropagation()} ondblclick={(e) => e.stopPropagation()}>
    {#each blocks as b (b.id)}
      {#if !editing || editing.id !== b.id}
        {@const r = rectToCss(b.rect, size, zoom, rot)}
        <button
          type="button"
          class="absolute rounded-sm border border-transparent hover:border-blue-500 hover:bg-blue-500/10 focus:border-blue-500 focus:outline-none"
          style="left:{r.x - 3}px;top:{r.y - 3}px;width:{r.w + 6}px;height:{r.h + 6}px;cursor:text"
          title="Edit paragraph"
          aria-label="Edit paragraph: {b.text.slice(0, 60)}"
          onclick={() => begin(b)}
        ></button>
      {/if}
    {/each}

    {#if editing && box}
      <!-- Opaque paper behind the editor so the old text does not ghost
           through; the textarea's text starts exactly where the block's did. -->
      <div class="absolute" style="left:{box.x - 6}px;top:{box.y - 4}px;width:{Math.max(box.w + 12, 220)}px" onfocusout={(e) => {
        const next = (e as FocusEvent).relatedTarget as Node | null;
        if (next && (e.currentTarget as HTMLElement).contains(next)) return;
        void commit();
      }}>
        <div class="mb-1 flex flex-wrap items-center gap-1 rounded bg-neutral-800 px-1.5 py-1 text-xs text-white shadow" role="toolbar" aria-label="Text formatting" tabindex="-1" onmousedown={(e) => { if ((e.target as HTMLElement).tagName === "BUTTON") e.preventDefault(); }}>
          <button type="button" class={tbtn(fmtBold)} title="Bold (Ctrl+B)" onclick={toggleBold}><b>B</b></button>
          <button type="button" class="{tbtn(fmtItalic)} italic" title="Italic (Ctrl+I)" onclick={toggleItalic}><i>I</i></button>
          <button type="button" class="{tbtn(fmtUnderline)} underline" title="Underline (Ctrl+U)" onclick={toggleUnderline}>U</button>
          <span class="mx-0.5 h-4 w-px bg-neutral-600"></span>
          <button type="button" class={tbtn(fmtAlign === "left")} title="Align left" onclick={() => setAlign("left")}>≡</button>
          <button type="button" class={tbtn(fmtAlign === "center")} title="Align center" onclick={() => setAlign("center")}>≣</button>
          <button type="button" class={tbtn(fmtAlign === "right")} title="Align right" onclick={() => setAlign("right")}>☰</button>
          <span class="mx-0.5 h-4 w-px bg-neutral-600"></span>
          <select
            class="h-6 rounded border-0 bg-neutral-700 px-1 text-xs"
            value={fmtFamily}
            onchange={(e) => setFamily((e.currentTarget as HTMLSelectElement).value as typeof fmtFamily)}
            aria-label="Font family"
          >
            <option value="Helvetica">Helvetica</option>
            <option value="Times">Times</option>
            <option value="Courier">Courier</option>
          </select>
          <input
            type="number"
            min="4"
            max="144"
            step="0.5"
            class="h-6 w-14 rounded border-0 bg-neutral-700 px-1 text-xs"
            value={fmtSize}
            onchange={(e) => setSize(parseFloat((e.currentTarget as HTMLInputElement).value) || fmtSize)}
            aria-label="Font size"
          />
          <input
            type="color"
            class="h-6 w-7 cursor-pointer rounded border-0 bg-transparent p-0"
            value={colorToHex(fmtColor)}
            oninput={(e) => setColor(hexToColor((e.currentTarget as HTMLInputElement).value))}
            aria-label="Text color"
          />
        </div>
        <textarea
          bind:this={ta}
          bind:value={draft}
          class="block w-full resize-none rounded-sm border-2 border-blue-500 shadow-lg outline-none {docStore.nightMode ? 'bg-black' : 'bg-white'}"
          style="padding:2px 4px;font-size:{fontPx}px;line-height:{leadPx}px;min-height:{box.h + leadPx * 2 + 8}px;color:{docStore.nightMode ? '#fff' : colorToCss(fmtColor)};text-align:{fmtAlign};font-family:{fmtFamily === 'Times' ? 'Georgia, Times, serif' : fmtFamily === 'Courier' ? 'Consolas, Courier, monospace' : 'Helvetica, Arial, sans-serif'};font-weight:{fmtBold ? 700 : 400};font-style:{fmtItalic ? 'italic' : 'normal'};text-decoration:{fmtUnderline ? 'underline' : 'none'}"
          spellcheck="true"
          disabled={busy}
          onkeydown={onKey}
        ></textarea>
        <div class="mt-1 flex w-max max-w-[60vw] items-center gap-3 whitespace-nowrap rounded bg-neutral-800 px-2 py-1 text-xs text-white shadow">
          <span class="opacity-70">{fmtFamily} {fmtSize.toFixed(1)}pt</span>
          <span class="opacity-70">Ctrl+Enter applies, Esc cancels</span>
          {#if busy}<span>Applying…</span>{/if}
        </div>
        {#if error}
          <div class="mt-1 rounded bg-red-600 px-2 py-1 text-xs text-white" role="alert">{error}</div>
        {/if}
      </div>
    {/if}
  </div>
{/if}
