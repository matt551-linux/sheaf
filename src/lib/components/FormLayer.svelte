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
</script>

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
        <select
          multiple={f.multiselect}
          size={Math.max(2, f.options.length)}
          class="absolute border border-blue-300/70 bg-[#eef3fd] text-neutral-900 focus:outline focus:outline-2 focus:outline-blue-500"
          style="left:{r.x}px;top:{r.y}px;width:{r.w}px;height:{r.h}px;font-size:{fontPx}px"
          disabled={f.readonly}
          aria-label={f.alt_name || f.name}
          onchange={(e) => {
            const sel = [...(e.currentTarget as HTMLSelectElement).selectedOptions].map((o) => o.value);
            void docStore.setFormField(index, f.annot_index, sel.join("\n"));
          }}
        >
          {#each f.options as o}
            <option value={o.label} selected={o.selected}>{o.label}</option>
          {/each}
        </select>
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
