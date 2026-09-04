<script lang="ts">
  // In-place paragraph editor. Overlays the page in edit mode: hover shows
  // paragraph outlines, click opens a textarea over the paragraph with the
  // same font size and leading. Ctrl+Enter or clicking away commits;
  // Escape cancels. Reflow happens in the engine (word wrap to the block's
  // width), then the page re-renders.
  import { docStore } from "$lib/stores/document.svelte";
  import { errorMessage, type TextBlock } from "$lib/api";
  import { rectToCss, pxPerPt, colorToCss } from "$lib/viewer/geometry";

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

  function begin(b: TextBlock) {
    if (busy) return;
    draft = b.text;
    docStore.editingBlock = { page: index, id: b.id };
    queueMicrotask(() => {
      ta?.focus();
      ta?.setSelectionRange(draft.length, draft.length);
    });
  }
  async function commit() {
    const b = editing;
    if (!b || busy) return;
    if (draft === b.text) {
      docStore.editingBlock = null;
      return;
    }
    busy = true;
    error = null;
    try {
      await docStore.commitBlock(index, { id: b.id, text: draft });
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
    }
    e.stopPropagation();
  }

  // Editor box: the block rect plus room for a couple of extra lines so
  // typing more does not immediately clip.
  const box = $derived(editing ? rectToCss(editing.rect, size, zoom, rot) : null);
  const fontPx = $derived(editing ? editing.font_size * scale : 12);
  const leadPx = $derived(editing ? editing.leading * scale : 14);
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
      <div class="absolute" style="left:{box.x - 6}px;top:{box.y - 4}px;width:{Math.max(box.w + 12, 160)}px">
        <textarea
          bind:this={ta}
          bind:value={draft}
          class="block w-full resize-none rounded-sm border-2 border-blue-500 shadow-lg outline-none {docStore.nightMode ? 'bg-black' : 'bg-white'}"
          style="padding:2px 4px;font-size:{fontPx}px;line-height:{leadPx}px;min-height:{box.h + leadPx * 2 + 8}px;color:{docStore.nightMode ? '#fff' : colorToCss(editing.color)};font-family:Helvetica, Arial, sans-serif;font-weight:{/bold/i.test(editing.font) ? 700 : 400};font-style:{/italic|oblique/i.test(editing.font) ? 'italic' : 'normal'}"
          spellcheck="true"
          disabled={busy}
          onkeydown={onKey}
          onblur={() => void commit()}
        ></textarea>
        <div class="mt-1 flex w-max max-w-[60vw] items-center gap-3 whitespace-nowrap rounded bg-neutral-800 px-2 py-1 text-xs text-white shadow">
          <span class="opacity-70">{editing.font} {editing.font_size.toFixed(1)}pt</span>
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
