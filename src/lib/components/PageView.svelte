<script lang="ts">
  // One page: bitmap plus interactive overlay (text selection, search hits,
  // annotation outlines, in-progress tool drawing). All geometry in this
  // component is CSS page space; conversion to PDF space happens at the edges.
  import { docStore, type Tool } from "$lib/stores/document.svelte";
  import type { Annotation, Rect } from "$lib/api";
  import {
    charAt,
    colorToCss,
    cssToPdf,
    inkBounds,
    lineRects,
    nearestChar,
    normRect,
    pdfToCss,
    rectToCss,
    rectToPdf,
    rectToQuad,
    selectedText,
    unionRects,
  } from "$lib/viewer/geometry";
  import type { PageLayout } from "$lib/viewer/layout";

  interface Props {
    layout: PageLayout;
    image: string | undefined;
    onOpenNote: (a: Annotation) => void;
  }
  let { layout, image, onOpenNote }: Props = $props();

  const index = $derived(layout.index);
  const size = $derived(docStore.pageSizes[index] ?? { width: 1, height: 1 });
  const zoom = $derived(docStore.zoom);
  const rot = $derived(docStore.rotation);
  const text = $derived(docStore.texts[index]);
  const annots = $derived(docStore.annots[index] ?? []);
  const tool = $derived(docStore.tool);

  let el: HTMLDivElement;

  // Lazy-load text + annotations when the page becomes visible.
  $effect(() => {
    if (!docStore.doc) return;
    void docStore.ensureText(index);
    void docStore.ensureAnnots(index);
  });

  // ---- derived overlays ----
  const selectionRects = $derived.by((): Rect[] => {
    const s = docStore.selection;
    if (!s || s.page !== index || !text) return [];
    return lineRects(text.chars, s.start, s.end).map((r) => rectToCss(r, size, zoom, rot));
  });

  const searchRects = $derived.by((): { r: Rect; current: boolean }[] => {
    const out: { r: Rect; current: boolean }[] = [];
    docStore.searchHits.forEach((h, i) => {
      if (h.page_index !== index) return;
      for (const r of h.rects) out.push({ r: rectToCss(r, size, zoom, rot), current: i === docStore.searchIndex });
    });
    return out;
  });

  const annotBoxes = $derived.by(() =>
    annots
      .filter((a) => a.editable && !a.hidden)
      .map((a) => ({ a, r: rectToCss(a.rect, size, zoom, rot) })),
  );

  // ---- pointer handling ----
  let drag = $state<{ x: number; y: number; cx: number; cy: number } | null>(null);
  let inkPath = $state<number[][]>([]);
  let inkPaths = $state<number[][][]>([]);
  let moving = $state<{ a: Annotation; ox: number; oy: number; start: Rect } | null>(null);

  const isMarkup = (t: Tool) => t === "highlight" || t === "underline" || t === "strikeout" || t === "squiggly";
  const isShape = (t: Tool) => t === "square" || t === "circle";

  function local(e: PointerEvent): [number, number] {
    const b = el.getBoundingClientRect();
    return [e.clientX - b.left, e.clientY - b.top];
  }
  function toPdf(x: number, y: number): [number, number] {
    return cssToPdf(x, y, size, zoom, rot);
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || !docStore.doc) return;
    const [x, y] = local(e);
    if (tool === "hand") return; // scroller handles panning via native drag
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      /* synthetic or already-released pointer */
    }

    // Selecting / moving an existing annotation with the select tool.
    if (tool === "select" || tool === "eraser") {
      const hit = [...annotBoxes].reverse().find(({ r }) => x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h);
      if (hit) {
        if (tool === "eraser") {
          void docStore.deleteAnnotation(index, hit.a.index);
          return;
        }
        docStore.selected = { page: index, index: hit.a.index };
        docStore.selection = null;
        if (hit.a.kind !== "highlight" && hit.a.kind !== "underline" && hit.a.kind !== "strikeout" && hit.a.kind !== "squiggly") {
          moving = { a: hit.a, ox: x, oy: y, start: hit.r };
        }
        e.preventDefault();
        return;
      }
      docStore.selected = null;
    }

    if (tool === "note") {
      const [px, py] = toPdf(x, y);
      void docStore.addAnnotation(index, {
        kind: "text",
        rect: { x: px - 10, y: py - 10, w: 20, h: 20 },
        color: docStore.styles.note.color,
        contents: "",
      }).then((a) => a && onOpenNote(a));
      return;
    }

    if (tool === "ink") {
      inkPath = [[x, y]];
      drag = { x, y, cx: x, cy: y };
      return;
    }

    if (tool === "select" || isMarkup(tool)) {
      if (!text) return;
      const [px, py] = toPdf(x, y);
      const i = charAt(text.chars, px, py);
      if (i >= 0) {
        docStore.selection = { page: index, start: i, end: i };
        drag = { x, y, cx: x, cy: y };
        e.preventDefault();
      } else {
        docStore.selection = null;
      }
      return;
    }

    if (isShape(tool) || tool === "freetext") {
      drag = { x, y, cx: x, cy: y };
      e.preventDefault();
    }
  }

  function onPointerMove(e: PointerEvent) {
    const [x, y] = local(e);
    if (moving) {
      moving = { ...moving, start: { ...moving.start, x: moving.start.x + (x - moving.ox), y: moving.start.y + (y - moving.oy) }, ox: x, oy: y };
      return;
    }
    if (!drag) return;
    if (tool === "ink") {
      inkPath = [...inkPath, [x, y]];
      return;
    }
    drag = { ...drag, cx: x, cy: y };
    if ((tool === "select" || isMarkup(tool)) && text && docStore.selection?.page === index) {
      const [px, py] = toPdf(x, y);
      const i = nearestChar(text.chars, px, py);
      if (i >= 0) docStore.selection = { ...docStore.selection, end: i };
    }
  }

  async function onPointerUp(e: PointerEvent) {
    const [x, y] = local(e);
    if (moving) {
      const m = moving;
      moving = null;
      const moved = Math.abs(x - m.start.x) + Math.abs(y - m.start.y);
      void moved;
      const pdfRect = rectToPdf(m.start, size, zoom, rot);
      if (Math.abs(pdfRect.x - m.a.rect.x) > 0.5 || Math.abs(pdfRect.y - m.a.rect.y) > 0.5) {
        await docStore.updateAnnotation(index, m.a.index, { rect: pdfRect });
      }
      return;
    }
    if (!drag) return;
    const d = drag;
    drag = null;

    if (tool === "ink") {
      const path = inkPath;
      inkPath = [];
      if (path.length > 1) inkPaths = [...inkPaths, path];
      // Commit on a short idle so multi-stroke drawings become one annotation.
      scheduleInkCommit();
      return;
    }

    if (isMarkup(tool)) {
      const s = docStore.selection;
      if (s && s.page === index && text) {
        const rects = lineRects(text.chars, s.start, s.end);
        if (rects.length) {
          const st = docStore.styles[tool];
          await docStore.addAnnotation(index, {
            kind: tool,
            rect: unionRects(rects),
            quads: rects.map(rectToQuad),
            color: st.color,
          });
        }
      }
      docStore.selection = null;
      return;
    }

    if (isShape(tool) || tool === "freetext") {
      const r = normRect(d.x, d.y, d.cx, d.cy);
      if (r.w < 4 || r.h < 4) return;
      const pdfRect = rectToPdf(r, size, zoom, rot);
      const st = docStore.styles[tool];
      const a = await docStore.addAnnotation(index, {
        kind: tool === "freetext" ? "freetext" : tool,
        rect: pdfRect,
        color: st.color,
        interior_color: st.interior,
        border_width: st.width,
        font_size: st.fontSize,
        contents: "",
      });
      if (a && tool === "freetext") onOpenNote(a);
    }
  }

  let inkTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleInkCommit() {
    if (inkTimer) clearTimeout(inkTimer);
    inkTimer = setTimeout(commitInk, 700);
  }
  async function commitInk() {
    const paths = inkPaths;
    inkPaths = [];
    if (!paths.length) return;
    const st = docStore.styles.ink;
    const pdfPaths = paths.map((p) => p.map(([x, y]) => toPdf(x, y)));
    await docStore.addAnnotation(index, {
      kind: "ink",
      rect: inkBounds(pdfPaths, st.width),
      ink: pdfPaths,
      color: st.color,
      border_width: st.width,
    });
  }

  function onDblClick(e: MouseEvent) {
    const [x, y] = [e.clientX - el.getBoundingClientRect().left, e.clientY - el.getBoundingClientRect().top];
    const hit = [...annotBoxes].reverse().find(({ r }) => x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h);
    if (hit) {
      onOpenNote(hit.a);
      return;
    }
    // Double-click selects a word.
    if (!text) return;
    const [px, py] = toPdf(x, y);
    const i = charAt(text.chars, px, py);
    if (i < 0) return;
    const isWord = (ch: string) => /[\p{L}\p{N}_'-]/u.test(ch);
    let s = i;
    let en = i;
    while (s > 0 && isWord(text.chars[s - 1].ch)) s--;
    while (en < text.chars.length - 1 && isWord(text.chars[en + 1].ch)) en++;
    docStore.selection = { page: index, start: s, end: en };
  }

  export function copySelection(): string | null {
    const s = docStore.selection;
    if (!s || s.page !== index || !text) return null;
    return selectedText(text.chars, s.start, s.end);
  }

  const cursor = $derived(
    tool === "hand" ? "grab" : tool === "select" || isMarkup(tool) ? "text" : tool === "eraser" ? "not-allowed" : "crosshair",
  );
  const dragRect = $derived(drag && (isShape(tool) || tool === "freetext") ? normRect(drag.x, drag.y, drag.cx, drag.cy) : null);
  const inkPx = (p: number[][]) => p.map(([x, y]) => `${x},${y}`).join(" ");
  const strokeCss = $derived(colorToCss(docStore.styles.ink.color));
  const strokeW = $derived(docStore.styles.ink.width * ((zoom * 96) / 72));
</script>

<div
  bind:this={el}
  class="absolute bg-white shadow-md {docStore.nightMode ? 'night' : ''}"
  style="top:{layout.top}px; left:calc(50% + {layout.left}px); width:{layout.width}px; height:{layout.height}px; cursor:{cursor}; touch-action:none"
  data-page={index}
  role="img"
  aria-label="Page {index + 1}"
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={() => ((drag = null), (moving = null))}
  ondblclick={onDblClick}
>
  {#if image}
    <img src={image} alt="" width={layout.width} height={layout.height} class="block h-full w-full select-none" draggable="false" />
  {:else}
    <div class="flex h-full w-full items-center justify-center text-sm text-neutral-400">{index + 1}</div>
  {/if}

  <svg class="pointer-events-none absolute inset-0 h-full w-full" width={layout.width} height={layout.height} aria-hidden="true">
    {#each searchRects as s}
      <rect x={s.r.x} y={s.r.y} width={s.r.w} height={s.r.h} fill={s.current ? "rgba(255,140,0,0.55)" : "rgba(255,220,0,0.4)"} />
    {/each}
    {#each selectionRects as r}
      <rect x={r.x} y={r.y} width={r.w} height={r.h} fill="rgba(0,120,255,0.3)" />
    {/each}
    {#each annotBoxes as { a, r }}
      {@const sel = docStore.selected?.page === index && docStore.selected.index === a.index}
      {@const live = moving && moving.a.index === a.index ? moving.start : r}
      {#if sel}
        <rect x={live.x - 2} y={live.y - 2} width={live.w + 4} height={live.h + 4} fill="none" stroke="#2563eb" stroke-width="1.5" stroke-dasharray="4 3" />
      {/if}
    {/each}
    {#if dragRect}
      {#if tool === "circle"}
        <ellipse cx={dragRect.x + dragRect.w / 2} cy={dragRect.y + dragRect.h / 2} rx={dragRect.w / 2} ry={dragRect.h / 2} fill="none" stroke={colorToCss(docStore.styles.circle.color)} stroke-width="2" />
      {:else}
        <rect x={dragRect.x} y={dragRect.y} width={dragRect.w} height={dragRect.h} fill="none" stroke={colorToCss(docStore.styles[tool].color)} stroke-width="2" stroke-dasharray={tool === "freetext" ? "4 3" : undefined} />
      {/if}
    {/if}
    {#each inkPaths as p}
      <polyline points={inkPx(p)} fill="none" stroke={strokeCss} stroke-width={strokeW} stroke-linecap="round" stroke-linejoin="round" />
    {/each}
    {#if inkPath.length > 1}
      <polyline points={inkPx(inkPath)} fill="none" stroke={strokeCss} stroke-width={strokeW} stroke-linecap="round" stroke-linejoin="round" />
    {/if}
  </svg>
</div>

<style>
  .night img {
    filter: invert(1) hue-rotate(180deg);
  }
</style>
