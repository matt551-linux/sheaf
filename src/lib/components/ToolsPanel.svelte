<script lang="ts">
  // M7: Redact, Compare, OCR, Accessibility.
  import { docStore } from "$lib/stores/document.svelte";
  import { api, errorMessage, type AccessibilityReport, type CompareResult, type Rect } from "$lib/api";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onClose: () => void;
    initialTab?: "redact" | "compare" | "ocr" | "access";
  }
  let { onClose, initialTab = "redact" }: Props = $props();
  type Tab = "redact" | "compare" | "ocr" | "access";
  // svelte-ignore state_referenced_locally
  let tab = $state<Tab>(initialTab);
  $effect(() => {
    tab = initialTab;
  });

  const doc = $derived(docStore.doc);
  let busy = $state(false);
  let error = $state<string | null>(null);
  async function run(fn: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await fn();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  const applyInfo = (info: NonNullable<typeof doc>) => {
    docStore.applyStructure(info);
    void docStore.refreshSignatures(info.id);
  };

  // ---- redact ----
  let redactQuery = $state("");
  let marking = $state(false);
  const markCount = $derived(Object.values(docStore.redactMarks).reduce((n, r) => n + r.length, 0));
  function startMark() {
    marking = true;
    docStore.tool = "select";
    docStore.placingSignature = (pg, r) => {
      marking = false;
      const cur = docStore.redactMarks[pg] ?? [];
      docStore.redactMarks = { ...docStore.redactMarks, [pg]: [...cur, r] };
    };
  }
  function cancelMark() {
    marking = false;
    docStore.placingSignature = null;
  }
  const markSearch = () =>
    run(async () => {
      if (!doc || !redactQuery.trim()) return;
      const hits = await api.redactSearch(doc.id, redactQuery.trim());
      const next: Record<number, Rect[]> = { ...docStore.redactMarks };
      for (const h of hits) next[h.page] = [...(next[h.page] ?? []), h.rect];
      docStore.redactMarks = next;
      docStore.showToast(hits.length ? `Marked ${hits.length} occurrence${hits.length === 1 ? "" : "s"}` : "No matches");
    });
  const clearMarks = () => (docStore.redactMarks = {});
  const applyRedactions = () =>
    run(async () => {
      if (!doc) return;
      const marks = docStore.redactMarks;
      let info = doc;
      for (const [pg, rects] of Object.entries(marks)) {
        if (rects.length) info = await api.redact(info.id, Number(pg), rects);
      }
      docStore.redactMarks = {};
      applyInfo(info);
      docStore.showToast("Redactions applied. Save to make them permanent.");
    });

  // ---- compare ----
  let otherPath = $state<string | null>(null);
  let otherId = $state<number | null>(null);
  let cmp = $state<CompareResult | null>(null);
  let cmpPage = $state(0);
  let visual = $state<string | null>(null);
  const pickOther = () =>
    run(async () => {
      if (!doc) return;
      const picked = await open({ multiple: false, filters: [{ name: "PDF", extensions: ["pdf"] }] });
      if (!picked || Array.isArray(picked)) return;
      if (otherId != null) await api.closeDocument(otherId).catch(() => {});
      const other = await api.openDocument(picked);
      otherId = other.id;
      otherPath = picked;
      cmp = await api.compareText(doc.id, other.id);
      cmpPage = cmp.pages.find((p) => p.inserted + p.deleted > 0)?.page ?? 0;
      await loadVisual();
    });
  async function loadVisual() {
    if (!doc || otherId == null) return;
    const r = await api.compareVisual(doc.id, otherId, cmpPage, 1);
    visual = `data:image/png;base64,${r.png_base64}`;
  }
  $effect(() => {
    return () => {
      if (otherId != null) void api.closeDocument(otherId).catch(() => {});
    };
  });

  // ---- OCR ----
  let modelsReady = $state<boolean | null>(null);
  let ocrScope = $state<"page" | "all">("page");
  let ocrText = $state<string | null>(null);
  $effect(() => {
    void api.ocrModelsReady().then((r) => (modelsReady = r)).catch(() => (modelsReady = false));
  });
  const downloadModels = () =>
    run(async () => {
      await api.ocrDownloadModels();
      modelsReady = await api.ocrModelsReady();
    });
  const runOcr = () =>
    run(async () => {
      if (!doc) return;
      const pages = ocrScope === "page" ? [docStore.currentPage] : doc.pages.map((_, i) => i);
      const res = await api.ocrPages(doc.id, pages);
      ocrText = res.text;
      const info = await api.documentInfo(doc.id);
      applyInfo(info);
      docStore.showToast(res.lines ? `Recognised ${res.lines} line${res.lines === 1 ? "" : "s"}` : "No text found");
    });

  // ---- accessibility ----
  let report = $state<AccessibilityReport | null>(null);
  const runReport = () =>
    run(async () => {
      if (!doc) return;
      report = await api.accessibilityReport(doc.id);
    });
  $effect(() => {
    if (tab === "access" && doc && !report) void runReport();
  });

  const btn =
    "inline-flex h-8 items-center justify-center whitespace-nowrap rounded px-2.5 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700";
  const primary = "inline-flex h-8 items-center justify-center whitespace-nowrap rounded bg-blue-600 px-3 text-sm text-white hover:bg-blue-700 disabled:opacity-40";
  const outline = `${btn} border border-neutral-300 dark:border-neutral-600`;
  const field = "h-8 w-full rounded border border-neutral-300 bg-white px-2 text-sm dark:border-neutral-600 dark:bg-neutral-800";
  const tabBtn = (t: Tab) =>
    `flex-1 border-b-2 px-1 py-1.5 text-xs ${tab === t ? "border-blue-600 font-semibold" : "border-transparent text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200"}`;
</script>

<aside class="flex w-80 shrink-0 flex-col border-l border-neutral-300 bg-neutral-50 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100" aria-label="Tools" data-panel="tools">
  <div class="flex h-11 shrink-0 items-center border-b border-neutral-300 px-2 dark:border-neutral-700">
    <span class="flex-1 text-sm font-semibold">Tools</span>
    <button class={btn} onclick={() => (cancelMark(), onClose())} aria-label="Close">✕</button>
  </div>
  <div class="flex shrink-0 border-b border-neutral-200 dark:border-neutral-800" role="tablist">
    <button class={tabBtn("redact")} role="tab" aria-selected={tab === "redact"} onclick={() => (tab = "redact")}>Redact{markCount ? ` (${markCount})` : ""}</button>
    <button class={tabBtn("compare")} role="tab" aria-selected={tab === "compare"} onclick={() => (tab = "compare")}>Compare</button>
    <button class={tabBtn("ocr")} role="tab" aria-selected={tab === "ocr"} onclick={() => (tab = "ocr")}>OCR</button>
    <button class={tabBtn("access")} role="tab" aria-selected={tab === "access"} onclick={() => (tab = "access")}>Access.</button>
  </div>
  {#if error}
    <div class="m-2 rounded bg-red-100 px-2 py-1 text-xs text-red-800 dark:bg-red-900 dark:text-red-100" role="alert">{error}</div>
  {/if}

  <div class="flex-1 overflow-y-auto p-3 text-sm">
    {#if tab === "redact"}
      <p class="text-xs text-neutral-500">Mark areas, then apply. Applying removes the underlying text, images and annotations from the file; it cannot be undone after saving.</p>
      <div class="mt-2 flex gap-1">
        <input class={field} placeholder="Find text to redact…" bind:value={redactQuery} onkeydown={(e) => e.key === "Enter" && markSearch()} />
        <button class={outline} disabled={busy || !doc} onclick={markSearch}>Mark all</button>
      </div>
      {#if marking}
        <div class="mt-2 rounded bg-blue-50 p-2 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">Drag on the page over the area to redact.</div>
        <button class="{outline} mt-1 w-full" onclick={cancelMark}>Cancel</button>
      {:else}
        <button class="{outline} mt-2 w-full" disabled={busy || !doc} onclick={startMark}>Mark an area by dragging…</button>
      {/if}
      <div class="mt-3 flex items-center gap-2">
        <span class="text-xs text-neutral-500">{markCount} mark{markCount === 1 ? "" : "s"} pending</span>
        <span class="flex-1"></span>
        <button class={btn} disabled={!markCount || busy} onclick={clearMarks}>Clear</button>
        <button class="{primary} bg-red-600 hover:bg-red-700" disabled={!markCount || busy} onclick={applyRedactions}>Apply</button>
      </div>

    {:else if tab === "compare"}
      <button class="{outline} w-full" disabled={busy || !doc} onclick={pickOther}>{otherPath ? "Choose another PDF…" : "Choose PDF to compare with…"}</button>
      {#if otherPath}
        <div class="mt-1 truncate text-xs text-neutral-500" title={otherPath}>vs {otherPath.split(/[\\/]/).pop()}</div>
      {/if}
      {#if cmp}
        <div class="mt-2 text-xs"><span class="text-green-700 dark:text-green-300">+{cmp.inserted} words</span> · <span class="text-red-700 dark:text-red-300">−{cmp.deleted} words</span> across {cmp.pages.length} page{cmp.pages.length === 1 ? "" : "s"}</div>
        <div class="mt-2 flex items-center gap-1 text-xs">
          <span>Page</span>
          <select class="{field} w-20" bind:value={cmpPage} onchange={loadVisual}>
            {#each cmp.pages as p}<option value={p.page}>{p.page + 1}{p.inserted + p.deleted ? " *" : ""}</option>{/each}
          </select>
          <span class="text-neutral-500">red: only in this, green: only in other</span>
        </div>
        {#if visual}
          <img src={visual} alt="Visual difference for page {cmpPage + 1}" class="mt-2 w-full rounded border border-neutral-300 dark:border-neutral-700" />
        {/if}
        {#if cmp.pages[cmpPage]}
          <div class="mt-2 max-h-64 overflow-y-auto rounded border border-neutral-200 p-2 text-xs leading-5 dark:border-neutral-700">
            {#each cmp.pages[cmpPage].segments as s}
              {#if s.kind === "insert"}<mark class="bg-green-200 dark:bg-green-800">{s.text}</mark>
              {:else if s.kind === "delete"}<mark class="bg-red-200 line-through dark:bg-red-800">{s.text}</mark>
              {:else}<span>{s.text}</span>{/if}
            {/each}
          </div>
        {/if}
      {/if}

    {:else if tab === "ocr"}
      <p class="text-xs text-neutral-500">Recognises text on scanned pages and adds an invisible, searchable text layer. Runs locally; the page image never leaves this machine.</p>
      {#if modelsReady === false}
        <div class="mt-2 rounded bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">The recognition models (about 20 MB) are not installed yet.</div>
        <button class="{primary} mt-2 w-full" disabled={busy} onclick={downloadModels}>{busy ? "Downloading…" : "Download models"}</button>
      {:else if modelsReady}
        <div class="mt-2 flex gap-3 text-xs">
          <label class="flex items-center gap-1"><input type="radio" bind:group={ocrScope} value="page" /> This page</label>
          <label class="flex items-center gap-1"><input type="radio" bind:group={ocrScope} value="all" /> All pages</label>
        </div>
        <button class="{primary} mt-2 w-full" disabled={busy || !doc} onclick={runOcr}>{busy ? "Recognising…" : "Run OCR"}</button>
        {#if ocrText != null}
          <div class="mt-2 max-h-72 overflow-y-auto whitespace-pre-wrap rounded border border-neutral-200 p-2 text-xs dark:border-neutral-700">{ocrText || "(no text found)"}</div>
        {/if}
      {/if}

    {:else}
      <button class={outline} disabled={busy || !doc} onclick={runReport}>Re-check</button>
      {#if report}
        <ul class="mt-2 space-y-2">
          {#each report.checks as c}
            <li class="rounded border border-neutral-200 p-2 dark:border-neutral-700">
              <div class="flex items-center gap-2 text-sm"><span class={c.ok ? "text-green-600" : "text-amber-600"}>{c.ok ? "✓" : "!"}</span><span class="font-semibold">{c.name}</span></div>
              <div class="mt-0.5 text-xs text-neutral-600 dark:text-neutral-300">{c.detail}</div>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>
</aside>
