<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ask, open, save } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import NavPanel from "$lib/components/NavPanel.svelte";
  import PageCanvas from "$lib/components/PageCanvas.svelte";
  import NoteEditor from "$lib/components/NoteEditor.svelte";
  import PropertiesDialog from "$lib/components/PropertiesDialog.svelte";
  import { docStore, type Tool } from "$lib/stores/document.svelte";
  import { api, type Annotation } from "$lib/api";

  let canvas: PageCanvas;
  let nav: NavPanel;
  let password = $state("");
  let noteTarget = $state<Annotation | null>(null);
  let showProps = $state(false);

  function goToPage(i: number) {
    canvas?.scrollToPage(i);
    docStore.currentPage = i;
  }

  async function openDialog() {
    const picked = await open({ multiple: false, filters: [{ name: "PDF documents", extensions: ["pdf"] }] });
    if (typeof picked === "string") await docStore.open(picked);
  }

  async function saveAs(flatten = false) {
    if (!docStore.doc) return false;
    const path = await save({
      defaultPath: docStore.doc.path,
      filters: [{ name: "PDF document", extensions: ["pdf"] }],
      title: flatten ? "Save flattened copy" : "Save As",
    });
    if (!path) return false;
    return docStore.save(path, flatten);
  }

  async function doSave() {
    if (!docStore.doc) return false;
    return docStore.save(null, false);
  }

  async function print() {
    if (!docStore.doc) return;
    try {
      const p = await api.exportForPrint(docStore.doc.id);
      // Hand the print-ready copy to the OS default handler; the user picks
      // the printer there. A native print dialog is a later milestone.
      await openPath(p);
      docStore.showToast("Opened a print copy in your system PDF handler");
    } catch (e) {
      docStore.showToast(`Print failed: ${e}`);
    }
  }

  async function copy() {
    const t = canvas?.copySelection();
    if (t) {
      await writeText(t);
      docStore.showToast("Copied");
    }
  }

  async function confirmDiscard(): Promise<boolean> {
    if (!docStore.doc?.modified) return true;
    const ok = await ask(`Save changes to ${docStore.doc.file_name}?`, { title: "Unsaved changes", kind: "warning", okLabel: "Save", cancelLabel: "Discard" });
    if (ok) return doSave();
    return true;
  }

  async function closeDoc() {
    if (await confirmDiscard()) await docStore.close();
  }

  const toolKeys: Record<string, Tool> = { v: "select", h: "hand", u: "highlight", n: "note", t: "freetext", p: "ink", r: "square", e: "circle" };

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    const t = e.target as HTMLElement | null;
    const typing = !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
    const k = e.key.toLowerCase();
    if (mod && k === "o") return void (e.preventDefault(), openDialog());
    if (mod && k === "f") return void (e.preventDefault(), nav?.focusSearch());
    if (mod && k === "w") return void (e.preventDefault(), closeDoc());
    if (!docStore.doc) return;
    if (mod && k === "s") return void (e.preventDefault(), e.shiftKey ? saveAs() : doSave());
    if (mod && k === "p") return void (e.preventDefault(), print());
    if (mod && k === "d") return void (e.preventDefault(), (showProps = true));
    if (mod && k === "z" && !typing) return void (e.preventDefault(), e.shiftKey ? docStore.redo() : docStore.undo());
    if (mod && k === "y" && !typing) return void (e.preventDefault(), docStore.redo());
    if (mod && k === "c" && !typing) return void (e.preventDefault(), copy());
    if (mod && (e.key === "=" || e.key === "+")) return void (e.preventDefault(), e.shiftKey ? docStore.rotateView(90) : docStore.zoomIn());
    if (mod && (e.key === "-" || e.key === "_")) return void (e.preventDefault(), e.shiftKey ? docStore.rotateView(-90) : docStore.zoomOut());
    if (mod && e.key === "0") return void (e.preventDefault(), docStore.setFit("page"));
    if (mod && e.key === "1") return void (e.preventDefault(), docStore.setZoom(1));
    if (mod && e.key === "2") return void (e.preventDefault(), docStore.setFit("width"));
    if (mod && e.shiftKey && k === "n") {
      e.preventDefault();
      document.querySelector<HTMLInputElement>('input[aria-label="Page number"]')?.select();
      return;
    }
    if (e.key === "F3") return void (e.preventDefault(), nav?.findNext(e.shiftKey ? -1 : 1));
    if (typing) return;
    if (e.key === "Escape") {
      docStore.selected = null;
      docStore.selection = null;
      docStore.tool = "select";
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") return void docStore.deleteSelected();
    if (!mod && toolKeys[k]) return void (docStore.tool = toolKeys[k]);
    if (e.key === "PageDown" || e.key === "ArrowRight") return void (e.preventDefault(), goToPage(Math.min(docStore.doc.page_count - 1, docStore.currentPage + 1)));
    if (e.key === "PageUp" || e.key === "ArrowLeft") return void (e.preventDefault(), goToPage(Math.max(0, docStore.currentPage - 1)));
    if (e.key === "Home") return void (e.preventDefault(), goToPage(0));
    if (e.key === "End") return void (e.preventDefault(), goToPage(docStore.doc.page_count - 1));
  }

  onMount(() => {
    void docStore
      .loadPrefs()
      .then(() => api.launchFiles())
      .then((files) => {
        if (files[0]) void docStore.open(files[0]);
      });
    const unlistenDrop = getCurrentWebview().onDragDropEvent((ev) => {
      if (ev.payload.type === "drop") {
        const pdf = ev.payload.paths.find((p) => p.toLowerCase().endsWith(".pdf"));
        if (pdf) void docStore.open(pdf);
      }
    });
    const win = getCurrentWindow();
    const unlistenClose = win.onCloseRequested(async (ev) => {
      if (!docStore.doc?.modified) return;
      ev.preventDefault();
      if (await confirmDiscard()) await win.destroy();
    });
    return () => {
      unlistenDrop.then((f) => f());
      unlistenClose.then((f) => f());
    };
  });

  $effect(() => {
    const title = docStore.doc ? `${docStore.doc.modified ? "* " : ""}${docStore.doc.file_name} - Sheaf` : "Sheaf";
    getCurrentWindow().setTitle(title).catch(() => {});
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="flex h-screen w-screen flex-col overflow-hidden bg-neutral-200 dark:bg-neutral-800">
  <Toolbar onGoToPage={goToPage} onOpen={openDialog} onSave={doSave} onSaveAs={() => saveAs()} onPrint={print} onProperties={() => (showProps = true)} />
  <div class="flex min-h-0 flex-1">
    <NavPanel bind:this={nav} onGoToPage={goToPage} onOpenNote={(a) => (noteTarget = a)} />
    <div class="relative min-w-0 flex-1">
      <PageCanvas bind:this={canvas} onOpenNote={(a) => (noteTarget = a)} />

      {#if !docStore.doc && !docStore.busy}
        <div class="absolute inset-0 flex flex-col items-center justify-center gap-3 text-neutral-600 dark:text-neutral-300">
          <div class="text-2xl font-semibold">Sheaf</div>
          <p class="text-sm">Open a PDF or drop one here.</p>
          <button class="rounded bg-blue-600 px-4 py-2 text-white hover:bg-blue-700" onclick={openDialog}>Open PDF</button>
          {#if docStore.recents.length}
            <div class="mt-4 w-96 max-w-full">
              <div class="mb-1 text-xs font-semibold uppercase text-neutral-500">Recent</div>
              {#each docStore.recents as r}
                <button class="block w-full truncate rounded px-2 py-1 text-left text-sm hover:bg-neutral-300 dark:hover:bg-neutral-700" title={r} onclick={() => docStore.open(r)}>
                  {r.split(/[\\/]/).pop()}
                  <span class="block truncate text-xs text-neutral-500">{r}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if docStore.error}
            <p class="max-w-md rounded bg-red-100 px-3 py-2 text-sm text-red-800">{docStore.error}</p>
          {/if}
        </div>
      {/if}

      {#if docStore.busy}
        <div class="absolute inset-x-0 top-0 h-0.5 animate-pulse bg-blue-500"></div>
      {/if}

      {#if docStore.toast}
        <div class="pointer-events-none absolute bottom-4 left-1/2 z-30 -translate-x-1/2 rounded bg-neutral-900/90 px-3 py-1.5 text-sm text-white shadow">{docStore.toast}</div>
      {/if}

      {#if noteTarget}
        <NoteEditor annotation={noteTarget} onClose={() => (noteTarget = null)} />
      {/if}
      {#if showProps}
        <PropertiesDialog onClose={() => (showProps = false)} />
      {/if}

      {#if docStore.passwordPrompt}
        <div class="absolute inset-0 z-20 flex items-center justify-center bg-black/40">
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
            <p class="mb-3 text-sm text-neutral-600 dark:text-neutral-300">
              {docStore.passwordPrompt.wrong ? "That password did not work. Try again." : "This document is protected. Enter the password to open it."}
            </p>
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
