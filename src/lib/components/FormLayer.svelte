<script lang="ts">
  // Interactive HTML controls positioned over PDF form widgets. The page
  // bitmap still shows PDFium's rendering underneath; these controls carry
  // the actual input. Values are committed through the engine so appearance
  // streams regenerate and the render refreshes.
  import { docStore } from "$lib/stores/document.svelte";
  import type { FormField, Rect } from "$lib/api";
  import { rectToCss } from "$lib/viewer/geometry";

  interface Props {
    index: number;
  }
  let { index }: Props = $props();

  const size = $derived(docStore.pageSizes[index] ?? { width: 1, height: 1 });
  const zoom = $derived(docStore.zoom);
  const rot = $derived(docStore.rotation);
  const fields = $derived(docStore.formFields[index] ?? []);

  $effect(() => {
    if (docStore.doc && docStore.formMode) void docStore.ensureFormFields(index);
  });

  // Local text being edited, committed on blur/Enter so each keystroke does
  // not round-trip through the engine.
  let editing = $state<{ annot: number; value: string } | null>(null);

  function css(r: Rect): Rect {
    return rectToCss(r, size, zoom, rot);
  }

  function commitText(f: FormField) {
    if (editing?.annot !== f.annot_index) return;
    const v = editing.value;
    editing = null;
    if (v !== f.value) void docStore.setFormField(index, f.annot_index, v);
  }

  const fontPx = $derived(Math.max(9, 11 * zoom));

  // Multi-select listboxes render as a dropdown button with a checkbox
  // popup (a native multiple <select> is a big scroll box, which crowds
  // the page). Tracks which field's popup is open.
  let openList = $state<number | null>(null);
  const CONTROL_H = 24; // px, dropdown button height at zoom 1

  function toggleListOption(f: FormField, label: string) {
    const sel = new Set(f.options.filter((o) => o.selected).map((o) => o.label));
    if (sel.has(label)) sel.delete(label);
    else sel.add(label);
    void docStore.setFormField(index, f.annot_index, [...sel].join("\n"));
  }
  function listSummary(f: FormField): string {
    const sel = f.options.filter((o) => o.selected).map((o) => o.label);
    return sel.length ? sel.join(", ") : "—";
  }
  function onWindowPointer(e: PointerEvent) {
    if (openList !== null && !(e.target as HTMLElement).closest("[data-listbox-popup]")) openList = null;
  }
</script>

<svelte:window
  onpointerdown={onWindowPointer}
  onkeydown={(e) => {
    if (e.key === "Escape") openList = null;
  }}
/>

{#if docStore.formMode && fields.length}
  <div class="absolute inset-0" data-form-layer={index}>
    {#each fields as f (f.annot_index)}
      {@const r = css(f.rect)}
      {#if f.kind === "text"}
        {#if f.multiline}
          <textarea
            class="absolute resize-none border border-blue-300/70 bg-[#eef3fd] dark:bg-[#eef3fd] px-0.5 text-neutral-900 focus:bg-white/90 focus:outline focus:outline-2 focus:outline-blue-500"
            style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px;font-size:{fontPx}px"
            value={editing?.annot === f.annot_index ? editing.value : f.value}
            disabled={f.readonly}
            aria-label={f.alt_name || f.name}
            aria-required={f.required}
            oninput={(e) => (editing = { annot: f.annot_index, value: (e.currentTarget as HTMLTextAreaElement).value })}
            onfocus={() => (editing = { annot: f.annot_index, value: f.value })}
            onblur={() => commitText(f)}
          ></textarea>
        {:else}
          <input
            type={f.password ? "password" : "text"}
            class="absolute border border-blue-300/70 bg-[#eef3fd] dark:bg-[#eef3fd] px-0.5 text-neutral-900 focus:bg-white/90 focus:outline focus:outline-2 focus:outline-blue-500"
            style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px;font-size:{fontPx}px"
            value={editing?.annot === f.annot_index ? editing.value : f.value}
            disabled={f.readonly}
            aria-label={f.alt_name || f.name}
            aria-required={f.required}
            oninput={(e) => (editing = { annot: f.annot_index, value: (e.currentTarget as HTMLInputElement).value })}
            onfocus={() => (editing = { annot: f.annot_index, value: f.value })}
            onblur={() => commitText(f)}
            onkeydown={(e) => {
              if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
            }}
          />
        {/if}
      {:else if f.kind === "checkbox"}
        <input
          type="checkbox"
          class="absolute cursor-pointer accent-blue-600"
          style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px"
          checked={f.checked}
          disabled={f.readonly}
          aria-label={f.alt_name || f.name}
          onchange={(e) =>
            void docStore.setFormField(index, f.annot_index, (e.currentTarget as HTMLInputElement).checked ? "on" : "off")}
        />
      {:else if f.kind === "radio"}
        <input
          type="radio"
          name="form-{index}-{f.name}"
          class="absolute cursor-pointer accent-blue-600"
          style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px"
          checked={f.checked}
          disabled={f.readonly}
          aria-label="{f.alt_name || f.name}: {f.export_value}"
          onchange={() => void docStore.setFormField(index, f.annot_index, "on")}
        />
      {:else if f.kind === "combo"}
        <select
          class="absolute border border-blue-300/70 bg-[#eef3fd] text-neutral-900 focus:outline focus:outline-2 focus:outline-blue-500"
          style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px;font-size:{fontPx}px"
          disabled={f.readonly}
          aria-label={f.alt_name || f.name}
          onchange={(e) => void docStore.setFormField(index, f.annot_index, (e.currentTarget as HTMLSelectElement).value)}
        >
          {#if !f.options.some((o) => o.selected)}<option value="" selected></option>{/if}
          {#each f.options as o}
            <option value={o.label} selected={o.selected}>{o.label}</option>
          {/each}
        </select>
      {:else if f.kind === "listbox"}
        {@const btnH = Math.min(r.h, CONTROL_H * zoom)}
        <!-- PDFium paints the original full-height list into the page bitmap;
             cover the whole widget rect so it doesn't show behind the compact
             dropdown. Night mode inverts the page image, so match its paper. -->
        <div class="absolute {docStore.nightMode ? 'bg-black' : 'bg-white'}" style="left:{r.x - 2}px;top:{r.y - 2}px;width:{r.w + 4}px;height:{r.h + 4}px" aria-hidden="true"></div>
        {#if f.multiselect}
          <div data-listbox-popup class="absolute" style="left:{r.x}px;top:{r.y}px;width:{r.w}px">
            <button
              type="button"
              class="flex w-full items-center justify-between gap-1 border border-blue-300/70 bg-[#eef3fd] px-1 text-left text-neutral-900 focus:outline focus:outline-2 focus:outline-blue-500"
              style="height:{btnH}px;font-size:{fontPx}px"
              disabled={f.readonly}
              aria-label={f.alt_name || f.name}
              aria-haspopup="listbox"
              aria-expanded={openList === f.annot_index}
              onclick={() => (openList = openList === f.annot_index ? null : f.annot_index)}
            >
              <span class="truncate">{listSummary(f)}</span>
              <svg class="h-3 w-3 shrink-0 opacity-60" viewBox="0 0 12 12" fill="currentColor"><path d="M3 4.5l3 3 3-3z" /></svg>
            </button>
            {#if openList === f.annot_index}
              <div
                class="absolute left-0 z-30 mt-0.5 max-h-48 w-full min-w-28 overflow-auto rounded border border-neutral-300 bg-white py-0.5 shadow-lg"
                style="top:{btnH}px;font-size:{fontPx}px"
                role="listbox"
                aria-multiselectable="true"
              >
                {#each f.options as o}
                  <label class="flex cursor-pointer items-center gap-1.5 px-1.5 py-0.5 text-neutral-900 hover:bg-blue-50">
                    <input type="checkbox" class="accent-blue-600" role="option" aria-selected={o.selected} checked={o.selected} onchange={() => toggleListOption(f, o.label)} />
                    <span class="truncate">{o.label}</span>
                  </label>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <select
            class="absolute border border-blue-300/70 bg-[#eef3fd] text-neutral-900 focus:outline focus:outline-2 focus:outline-blue-500"
            style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{btnH}px;font-size:{fontPx}px"
            disabled={f.readonly}
            aria-label={f.alt_name || f.name}
            onchange={(e) => void docStore.setFormField(index, f.annot_index, (e.currentTarget as HTMLSelectElement).value)}
          >
            {#if !f.options.some((o) => o.selected)}<option value="" selected></option>{/if}
            {#each f.options as o}
              <option value={o.label} selected={o.selected}>{o.label}</option>
            {/each}
          </select>
        {/if}
      {:else if f.kind === "signature"}
        <div
          class="absolute border border-dashed border-amber-400 bg-amber-50/30"
          style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px"
          title="Signature field (signing lands in M5)"
        ></div>
      {/if}
    {/each}
  </div>
{/if}
