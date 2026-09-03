<script lang="ts">
  // Organize Pages: thumbnail grid with multi-select, drag to reorder,
  // rotate/delete/insert/extract, crop, and header/footer/Bates/watermark
  // stamping. Every operation goes through the engine and is undoable.
  import { docStore } from "$lib/stores/document.svelte";
  import { api, errorMessage, type StampSpec } from "$lib/api";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { colorToHex, hexToColor } from "$lib/viewer/geometry";

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  const doc = $derived(docStore.doc);
  let selected = $state<Set<number>>(new Set());
  let anchor = $state(-1);
  let busy = $state(false);
  let panel = $state<"none" | "crop" | "stamp">("none");

  // Thumbnails (independent of the nav panel cache; lower res is fine).
  let thumbs = $state<Record<number, string>>({});
  const SCALE = 0.35;
  let thumbToken = 0;
  $effect(() => {
    const d = docStore.doc;
    const ver = docStore.renderVersion;
    if (!d) return;
    const token = ++thumbToken;
    void ver;
    (async () => {
      const fresh: Record<number, string> = {};
      for (let i = 0; i < d.page_count; i++) {
        if (token !== thumbToken || docStore.doc?.id !== d.id) return;
        const r = await api.renderPage(d.id, i, SCALE, 0).catch(() => null);
        if (r) {
          fresh[i] = `data:image/png;base64,${r.png_base64}`;
          thumbs = { ...fresh };
        }
      }
    })();
  });

  function toggle(i: number, e: MouseEvent) {
    const next = new Set(selected);
    if (e.shiftKey && anchor >= 0) {
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      for (let p = a; p <= b; p++) next.add(p);
    } else if (e.ctrlKey || e.metaKey) {
      if (next.has(i)) next.delete(i);
      else next.add(i);
      anchor = i;
    } else {
      next.clear();
      next.add(i);
      anchor = i;
    }
    selected = next;
  }

  const sel = $derived([...selected].sort((a, b) => a - b));
  const hasSel = $derived(sel.length > 0);

  async function run(fn: () => Promise<unknown>, keepSelection = false) {
    if (busy) return;
    busy = true;
    try {
      await fn();
    } catch (e) {
      docStore.showToast(errorMessage(e));
    } finally {
      busy = false;
      if (!keepSelection) selected = new Set();
    }
  }

  const rotate = (delta: number) =>
    run(async () => {
      if (!doc || !hasSel) return;
      docStore.applyStructure(await api.rotatePages(doc.id, sel, delta));
    }, true);

  const del = () =>
    run(async () => {
      if (!doc || !hasSel) return;
      docStore.applyStructure(await api.deletePages(doc.id, sel));
      docStore.showToast(`Deleted ${sel.length} page${sel.length === 1 ? "" : "s"}`);
    });

  const extract = () =>
    run(async () => {
      if (!doc || !hasSel) return;
      const path = await save({
        defaultPath: doc.path.replace(/\.pdf$/i, "-extract.pdf"),
        filters: [{ name: "PDF document", extensions: ["pdf"] }],
        title: `Extract ${sel.length} page${sel.length === 1 ? "" : "s"}`,
      });
      if (!path) return;
      await api.extractPages(doc.id, sel, path);
      docStore.showToast(`Extracted to ${path.split(/[\\/]/).pop()}`);
    }, true);

  const insert = () =>
    run(async () => {
      if (!doc) return;
      const picked = await open({ multiple: false, filters: [{ name: "PDF documents", extensions: ["pdf"] }], title: "Insert pages from PDF" });
      if (typeof picked !== "string") return;
      const at = hasSel ? sel[sel.length - 1] + 1 : doc.page_count;
      docStore.applyStructure(await api.insertPages(doc.id, picked, at));
      docStore.showToast("Pages inserted");
    });

  // ---- drag to reorder ----
  let dragFrom = $state(-1);
  let dropAt = $state(-1);
  function onDrop(target: number) {
    const moving = selected.has(dragFrom) ? sel : [dragFrom];
    dragFrom = -1;
    dropAt = -1;
    if (!doc || moving.length === 0) return;
    // Dest is in post-removal coordinates per FPDF_MovePages semantics: it
    // takes the block's final start index directly.
    void run(async () => {
      docStore.applyStructure(await api.movePages(doc.id, moving, target));
    });
  }

  // ---- crop panel ----
  let cropMargin = $state(36);
  const crop = () =>
    run(async () => {
      if (!doc || !hasSel) return;
      const p0 = doc.pages[sel[0]];
      const m = cropMargin;
      docStore.applyStructure(await api.cropPages(doc.id, sel, [m, m, p0.width - m, p0.height - m]));
      panel = "none";
      docStore.showToast("Cropped");
    }, true);

  // ---- stamp panel ----
  let stampText = $state("Page {n} of {total}");
  let stampPos = $state<StampSpec["position"]>("footer-center");
  let stampSize = $state(11);
  let stampColor = $state({ r: 90, g: 90, b: 90 });
  let stampOpacity = $state(255);
  let stampStart = $state(1);
  let stampDigits = $state(6);
  const presets: { label: string; text: string; pos: StampSpec["position"]; size: number; opacity: number }[] = [
    { label: "Page numbers", text: "Page {n} of {total}", pos: "footer-center", size: 11, opacity: 255 },
    { label: "Bates", text: "BATES-{bates}", pos: "footer-right", size: 10, opacity: 255 },
    { label: "Watermark", text: "CONFIDENTIAL", pos: "watermark", size: 60, opacity: 50 },
  ];
  const stamp = () =>
    run(async () => {
      if (!doc) return;
      docStore.applyStructure(
        await api.stampPages(doc.id, {
          pages: hasSel ? sel : [],
          text: stampText,
          position: stampPos,
          font_size: stampSize,
          color: stampColor,
          opacity: stampOpacity,
          start_at: stampStart,
          bates_digits: stampDigits,
        }),
      );
      panel = "none";
      docStore.showToast("Stamped");
    }, true);

  const btn =
    "inline-flex h-8 items-center whitespace-nowrap rounded px-2.5 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700";
  const field = "h-7 rounded border border-neutral-300 bg-white px-1 text-sm dark:border-neutral-600 dark:bg-neutral-800";
</script>

<div class="absolute inset-0 z-20 flex flex-col bg-neutral-100 text-neutral-800 dark:bg-neutral-900 dark:text-neutral-100" role="dialog" aria-label="Organize pages">
  <div class="flex h-11 shrink-0 items-center gap-1 border-b border-neutral-300 px-2 dark:border-neutral-700">
    <span class="mr-2 text-sm font-semibold">Organize pages</span>
    <button class={btn} disabled={!hasSel || busy} onclick={() => rotate(-90)} title="Rotate selection counterclockwise">&#8634; Rotate</button>
    <button class={btn} disabled={!hasSel || busy} onclick={() => rotate(90)} title="Rotate selection clockwise">&#8635; Rotate</button>
    <button class={btn} disabled={!hasSel || busy || (doc?.page_count ?? 0) <= sel.length} onclick={del}>Delete</button>
    <span class="mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700"></span>
    <button class={btn} disabled={busy} onclick={insert} title="Insert all pages of another PDF after the selection">Insert PDF…</button>
    <button class={btn} disabled={!hasSel || busy} onclick={extract} title="Save the selected pages as a new PDF">Extract…</button>
    <span class="mx-1 h-6 w-px bg-neutral-300 dark:bg-neutral-700"></span>
    <button class="{btn} {panel === 'crop' ? 'bg-neutral-200 dark:bg-neutral-700' : ''}" disabled={!hasSel || busy} onclick={() => (panel = panel === "crop" ? "none" : "crop")}>Crop…</button>
    <button class="{btn} {panel === 'stamp' ? 'bg-neutral-200 dark:bg-neutral-700' : ''}" disabled={busy} onclick={() => (panel = panel === "stamp" ? "none" : "stamp")}>Header / Watermark…</button>
    <span class="flex-1"></span>
    <span class="mr-2 text-xs text-neutral-500">{hasSel ? `${sel.length} selected` : "Click to select; Ctrl/Shift for more; drag to reorder"}</span>
    <button class={btn} onclick={onClose}>Done</button>
  </div>

  {#if panel === "crop"}
    <div class="flex shrink-0 items-center gap-2 border-b border-neutral-300 px-3 py-2 text-sm dark:border-neutral-700">
      <span>Crop margin</span>
      <input type="number" min="0" max="200" class="{field} w-16" bind:value={cropMargin} />
      <span class="text-neutral-500">points, on each edge of the selected pages (based on page 1 of the selection)</span>
      <button class="{btn} bg-blue-600 text-white hover:bg-blue-700" disabled={busy} onclick={crop}>Apply crop</button>
    </div>
  {:else if panel === "stamp"}
    <div class="flex shrink-0 flex-wrap items-center gap-2 border-b border-neutral-300 px-3 py-2 text-sm dark:border-neutral-700">
      {#each presets as p}
        <button class="{btn} border border-neutral-300 dark:border-neutral-600" onclick={() => ((stampText = p.text), (stampPos = p.pos), (stampSize = p.size), (stampOpacity = p.opacity))}>{p.label}</button>
      {/each}
      <input class="{field} w-48" bind:value={stampText} aria-label="Stamp text" title="Supports {'{n}'}, {'{total}'}, {'{bates}'}" />
      <select class={field} bind:value={stampPos} aria-label="Position">
        <option value="header-left">Header left</option>
        <option value="header-center">Header center</option>
        <option value="header-right">Header right</option>
        <option value="footer-left">Footer left</option>
        <option value="footer-center">Footer center</option>
        <option value="footer-right">Footer right</option>
        <option value="watermark">Watermark</option>
      </select>
      <label class="flex items-center gap-1">Size <input type="number" min="6" max="144" class="{field} w-16" bind:value={stampSize} /></label>
      <input type="color" class="h-7 w-9 cursor-pointer border-0 bg-transparent p-0" value={colorToHex(stampColor)} oninput={(e) => (stampColor = hexToColor((e.currentTarget as HTMLInputElement).value))} aria-label="Color" />
      <label class="flex items-center gap-1">Opacity <input type="range" min="10" max="255" class="w-20" bind:value={stampOpacity} /></label>
      {#if stampText.includes("{bates}") || stampText.includes("{n}")}
        <label class="flex items-center gap-1">Start at <input type="number" min="0" class="{field} w-20" bind:value={stampStart} /></label>
      {/if}
      {#if stampText.includes("{bates}")}
        <label class="flex items-center gap-1">Digits <input type="number" min="1" max="12" class="{field} w-14" bind:value={stampDigits} /></label>
      {/if}
      <span class="text-neutral-500">{hasSel ? `${sel.length} selected page${sel.length === 1 ? "" : "s"}` : "all pages"}</span>
      <button class="{btn} bg-blue-600 text-white hover:bg-blue-700" disabled={busy} onclick={stamp}>Apply</button>
    </div>
  {/if}

  <div class="min-h-0 flex-1 overflow-auto p-4">
    {#if doc}
      <div class="flex flex-wrap gap-3">
        {#each doc.pages as p (p.index)}
          <div
            class="relative rounded border-2 p-1 transition-colors {selected.has(p.index) ? 'border-blue-500 bg-blue-50 dark:bg-blue-950' : 'border-transparent hover:border-neutral-300 dark:hover:border-neutral-600'} {dropAt === p.index ? 'outline outline-2 outline-blue-400' : ''}"
            role="button"
            tabindex="0"
            aria-pressed={selected.has(p.index)}
            draggable="true"
            onclick={(e) => toggle(p.index, e)}
            onkeydown={(e) => {
              if (e.key === " " || e.key === "Enter") toggle(p.index, { shiftKey: e.shiftKey, ctrlKey: e.ctrlKey, metaKey: e.metaKey } as MouseEvent);
            }}
            ondragstart={(e) => {
              dragFrom = p.index;
              e.dataTransfer!.effectAllowed = "move";
            }}
            ondragover={(e) => {
              e.preventDefault();
              dropAt = p.index;
            }}
            ondragleave={() => (dropAt = dropAt === p.index ? -1 : dropAt)}
            ondrop={(e) => {
              e.preventDefault();
              onDrop(p.index);
            }}
          >
            {#if thumbs[p.index]}
              <img src={thumbs[p.index]} alt="Page {p.index + 1}" class="pointer-events-none max-h-44 shadow" draggable="false" />
            {:else}
              <div class="flex h-40 w-32 items-center justify-center bg-white text-sm text-neutral-400 shadow">{p.index + 1}</div>
            {/if}
            <div class="mt-0.5 text-center text-xs">{p.index + 1}{p.rotation ? ` · ${p.rotation}°` : ""}</div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if busy}
    <div class="absolute inset-x-0 top-11 h-0.5 animate-pulse bg-blue-500"></div>
  {/if}
</div>
