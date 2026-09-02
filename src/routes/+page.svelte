<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import NavPanel from "$lib/components/NavPanel.svelte";
  import PageCanvas from "$lib/components/PageCanvas.svelte";
  import { docStore } from "$lib/stores/document.svelte";

  let canvas: PageCanvas;
  let nav: NavPanel;
  let password = $state("");

  function goToPage(i: number) {
    canvas?.scrollToPage(i);
    docStore.currentPage = i;
  }

  async function openDialog() {
    const picked = await open({ multiple: false, filters: [{ name: "PDF documents", extensions: ["pdf"] }] });
    if (typeof picked === "string") await docStore.open(picked);
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    const t = e.target as HTMLElement | null;
    const typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT");
    if (mod && e.key.toLowerCase() === "o") return void (e.preventDefault(), openDialog());
    if (mod && e.key.toLowerCase() === "f") return void (e.preventDefault(), nav?.focusSearch());
    if (mod && e.key.toLowerCase() === "w") return void (e.preventDefault(), docStore.close());
    if (!docStore.doc) return;
    if (mod && (e.key === "=" || e.key === "+")) return void (e.preventDefault(), e.shiftKey ? docStore.rotateView(90) : docStore.zoomIn());
    if (mod && (e.key === "-" || e.key === "_")) return void (e.preventDefault(), e.shiftKey ? docStore.rotateView(-90) : docStore.zoomOut());
    if (mod && e.key === "0") return void (e.preventDefault(), docStore.setFit("page"));
    if (mod && e.key === "1") return void (e.preventDefault(), docStore.setZoom(1));
    if (mod && e.key === "2") return void (e.preventDefault(), docStore.setFit("width"));
    if (mod && e.shiftKey && e.key.toLowerCase() === "n") {
      e.preventDefault();
      document.querySelector<HTMLInputElement>('input[aria-label="Page number"]')?.select();
      return;
    }
    if (typing) return;
    if (e.key === "PageDown" || e.key === "ArrowRight") return void (e.preventDefault(), goToPage(Math.min(docStore.doc.page_count - 1, docStore.currentPage + 1)));
    if (e.key === "PageUp" || e.key === "ArrowLeft") return void (e.preventDefault(), goToPage(Math.max(0, docStore.currentPage - 1)));
    if (e.key === "Home") return void (e.preventDefault(), goToPage(0));
    if (e.key === "End") return void (e.preventDefault(), goToPage(docStore.doc.page_count - 1));
  }

  onMount(() => {
    // Drag a PDF onto the window to open it.
    const unlisten = getCurrentWebview().onDragDropEvent((ev) => {
      if (ev.payload.type === "drop") {
        const pdf = ev.payload.paths.find((p) => p.toLowerCase().endsWith(".pdf"));
        if (pdf) void docStore.open(pdf);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="flex h-screen w-screen flex-col overflow-hidden bg-neutral-200 dark:bg-neutral-800">
  <Toolbar onGoToPage={goToPage} />
  <div class="flex min-h-0 flex-1">
    <NavPanel bind:this={nav} onGoToPage={goToPage} />
    <div class="relative min-w-0 flex-1">
      <PageCanvas bind:this={canvas} />
      {#if !docStore.doc && !docStore.busy}
        <div class="absolute inset-0 flex flex-col items-center justify-center gap-3 text-neutral-600 dark:text-neutral-300">
          <div class="text-2xl font-semibold">Sheaf</div>
          <p class="text-sm">Open a PDF or drop one here.</p>
          <button class="rounded bg-blue-600 px-4 py-2 text-white hover:bg-blue-700" onclick={openDialog}>Open PDF</button>
          {#if docStore.error}
            <p class="max-w-md rounded bg-red-100 px-3 py-2 text-sm text-red-800">{docStore.error}</p>
          {/if}
        </div>
      {/if}
      {#if docStore.busy}
        <div class="absolute inset-x-0 top-0 h-0.5 animate-pulse bg-blue-500"></div>
      {/if}
      {#if docStore.passwordPrompt}
        <div class="absolute inset-0 flex items-center justify-center bg-black/40">
          <form
            class="w-80 rounded bg-white p-4 shadow-xl dark:bg-neutral-900 dark:text-neutral-100"
            onsubmit={(e) => {
              e.preventDefault();
              const p = docStore.passwordPrompt?.path;
              if (p) void docStore.open(p, password);
              password = "";
            }}
          >
            <h2 class="mb-2 font-semibold">Password required</h2>
            <p class="mb-3 text-sm text-neutral-600 dark:text-neutral-300">This document is protected. Enter the password to open it.</p>
            <input class="mb-3 h-8 w-full rounded border border-neutral-300 px-2 dark:border-neutral-600 dark:bg-neutral-800" type="password" bind:value={password} />
            <div class="flex justify-end gap-2">
              <button type="button" class="rounded px-3 py-1 text-sm hover:bg-neutral-200 dark:hover:bg-neutral-700" onclick={() => (docStore.passwordPrompt = null)}>Cancel</button>
              <button type="submit" class="rounded bg-blue-600 px-3 py-1 text-sm text-white">Open</button>
            </div>
          </form>
        </div>
      {/if}
    </div>
  </div>
</div>
