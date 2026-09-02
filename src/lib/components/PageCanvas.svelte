<script lang="ts">
  import { docStore } from "$lib/stores/document.svelte";
  import { currentPage as pageAt, totalHeight, visiblePages } from "$lib/viewer/layout";
  import { onMount } from "svelte";
  import PageView from "./PageView.svelte";
  import type { Annotation } from "$lib/api";

  interface Props {
    onOpenNote: (a: Annotation) => void;
  }
  let { onOpenNote }: Props = $props();

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let images = $state<Record<number, string>>({});
  let renderGen = 0;
  let pageViews: Record<number, PageView> = {};

  const layout = $derived(docStore.layout);
  const height = $derived(totalHeight(layout));
  const visible = $derived(visiblePages(layout, scrollTop, docStore.viewportHeight, 1));

  $effect(() => {
    const doc = docStore.doc;
    const idx = visible;
    const scale = docStore.renderScale;
    const rot = docStore.rotation;
    const ver = docStore.renderVersion;
    void scale;
    void rot;
    void ver;
    if (!doc) {
      images = {};
      return;
    }
    const gen = ++renderGen;
    for (const i of idx) {
      docStore.pageImage(i).then((url) => {
        if (gen !== renderGen) return;
        images = { ...images, [i]: url };
      });
    }
  });

  onMount(() => {
    const ro = new ResizeObserver(() => docStore.setViewport(scroller.clientWidth, scroller.clientHeight));
    ro.observe(scroller);
    docStore.setViewport(scroller.clientWidth, scroller.clientHeight);
    return () => ro.disconnect();
  });

  function onScroll() {
    scrollTop = scroller.scrollTop;
    if (docStore.viewMode !== "single") {
      docStore.currentPage = pageAt(layout, scrollTop, docStore.viewportHeight);
    }
  }

  export function scrollToPage(index: number) {
    if (docStore.viewMode === "single") {
      docStore.currentPage = index;
      scroller?.scrollTo({ top: 0 });
      return;
    }
    const l = layout.find((p) => p.index === index);
    if (!l || !scroller) return;
    scroller.scrollTo({ top: Math.max(0, l.top - 8), behavior: "auto" });
  }

  export function copySelection(): string | null {
    const s = docStore.selection;
    if (!s) return null;
    return pageViews[s.page]?.copySelection() ?? null;
  }

  let lastZoom = docStore.zoom;
  $effect(() => {
    const z = docStore.zoom;
    if (z !== lastZoom && scroller && layout.length) {
      const ratio = z / lastZoom;
      lastZoom = z;
      requestAnimationFrame(() => (scroller.scrollTop = scroller.scrollTop * ratio));
    }
  });

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    if (e.deltaY < 0) docStore.zoomIn();
    else docStore.zoomOut();
  }

  // Hand tool: drag to pan.
  let pan: { x: number; y: number; sx: number; sy: number } | null = null;
  function onPointerDown(e: PointerEvent) {
    if (docStore.tool !== "hand" || e.button !== 0) return;
    pan = { x: e.clientX, y: e.clientY, sx: scroller.scrollLeft, sy: scroller.scrollTop };
    scroller.setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent) {
    if (!pan) return;
    scroller.scrollLeft = pan.sx - (e.clientX - pan.x);
    scroller.scrollTop = pan.sy - (e.clientY - pan.y);
  }
  function onPointerUp() {
    pan = null;
  }
</script>

<div
  class="relative h-full w-full overflow-auto bg-neutral-300 dark:bg-neutral-800"
  bind:this={scroller}
  onscroll={onScroll}
  onwheel={onWheel}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  role="document"
>
  {#if docStore.doc}
    <div class="relative mx-auto" style="height:{height}px; width:100%">
      {#each layout as l (l.index)}
        <PageView bind:this={pageViews[l.index]} layout={l} image={images[l.index]} {onOpenNote} />
      {/each}
    </div>
  {/if}
</div>
