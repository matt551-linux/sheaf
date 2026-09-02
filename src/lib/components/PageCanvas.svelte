<script lang="ts">
  import { docStore } from "$lib/stores/document.svelte";
  import { currentPage as pageAt, totalHeight, visiblePages } from "$lib/viewer/layout";
  import { onMount } from "svelte";

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let images = $state<Record<number, string>>({});
  let renderGen = 0;

  const layout = $derived(docStore.layout);
  const height = $derived(totalHeight(layout));
  const visible = $derived(visiblePages(layout, scrollTop, docStore.viewportHeight, 1));

  // Re-render visible pages whenever they, or the zoom/rotation, change.
  $effect(() => {
    const doc = docStore.doc;
    const idx = visible;
    const scale = docStore.renderScale;
    const rot = docStore.rotation;
    if (!doc) {
      images = {};
      return;
    }
    const gen = ++renderGen;
    void scale;
    void rot;
    for (const i of idx) {
      docStore.pageImage(i).then((url) => {
        if (gen !== renderGen) return;
        images = { ...images, [i]: url };
      });
    }
  });

  // Track the viewport size for fit modes.
  onMount(() => {
    const ro = new ResizeObserver(() => {
      docStore.setViewport(scroller.clientWidth, scroller.clientHeight);
    });
    ro.observe(scroller);
    docStore.setViewport(scroller.clientWidth, scroller.clientHeight);
    return () => ro.disconnect();
  });

  function onScroll() {
    scrollTop = scroller.scrollTop;
    docStore.currentPage = pageAt(layout, scrollTop, docStore.viewportHeight);
  }

  export function scrollToPage(index: number) {
    const l = layout[index];
    if (!l || !scroller) return;
    scroller.scrollTo({ top: Math.max(0, l.top - 8), behavior: "auto" });
  }

  // Keep the current page anchored when zoom changes.
  let lastZoom = docStore.zoom;
  $effect(() => {
    const z = docStore.zoom;
    if (z !== lastZoom && scroller && layout.length) {
      const ratio = z / lastZoom;
      lastZoom = z;
      requestAnimationFrame(() => {
        scroller.scrollTop = scroller.scrollTop * ratio;
      });
    }
  });

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    if (e.deltaY < 0) docStore.zoomIn();
    else docStore.zoomOut();
  }
</script>

<div
  class="relative h-full w-full overflow-auto bg-neutral-300 dark:bg-neutral-800"
  bind:this={scroller}
  onscroll={onScroll}
  onwheel={onWheel}
  role="document"
>
  {#if docStore.doc}
    <div class="relative mx-auto" style="height:{height}px; width:100%">
      {#each layout as l (l.index)}
        <div
          class="absolute left-1/2 -translate-x-1/2 bg-white shadow-md"
          style="top:{l.top}px; width:{l.width}px; height:{l.height}px"
          data-page={l.index}
        >
          {#if images[l.index]}
            <img
              src={images[l.index]}
              alt="Page {l.index + 1}"
              width={l.width}
              height={l.height}
              class="block h-full w-full select-none"
              draggable="false"
            />
          {:else}
            <div class="flex h-full w-full items-center justify-center text-sm text-neutral-400">
              {l.index + 1}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
