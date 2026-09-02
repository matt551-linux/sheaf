// Document session state: the open document, viewport, render cache.
// Svelte 5 runes in a .svelte.ts module so components share one instance.
import { api, isSheafError, type DocumentInfo, type OutlineNode, type SearchHit } from "$lib/api";
import {
  clampZoom,
  fitPageZoom,
  fitWidthZoom,
  layoutContinuous,
  nextZoom,
  type FitMode,
  type PageLayout,
} from "$lib/viewer/layout";

export type NavPanel = "none" | "thumbnails" | "bookmarks" | "search";

class DocumentStore {
  doc = $state<DocumentInfo | null>(null);
  outline = $state<OutlineNode[]>([]);
  error = $state<string | null>(null);
  passwordPrompt = $state<{ path: string } | null>(null);
  busy = $state(false);

  zoom = $state(1);
  fitMode = $state<FitMode>("width");
  rotation = $state(0); // view-only rotation, degrees
  currentPage = $state(0);
  navPanel = $state<NavPanel>("thumbnails");

  searchQuery = $state("");
  searchHits = $state<SearchHit[]>([]);
  searchIndex = $state(-1);

  viewportWidth = $state(800);
  viewportHeight = $state(600);

  /** Rendered page bitmaps keyed by `${page}@${scaleKey}`. */
  private cache = new Map<string, string>();
  private inflight = new Map<string, Promise<string>>();
  private cacheOrder: string[] = [];
  private readonly cacheLimit = 40;

  /** Device-pixel scale for rendering the current zoom crisply. */
  get renderScale(): number {
    const dpr = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    return this.zoom * dpr * (96 / 72);
  }

  layout = $derived.by((): PageLayout[] => {
    if (!this.doc) return [];
    const rotated = this.rotation % 180 !== 0;
    const pages = this.doc.pages.map((p) =>
      rotated ? { width: p.height, height: p.width } : { width: p.width, height: p.height },
    );
    return layoutContinuous(pages, this.zoom);
  });

  async open(path: string, password?: string) {
    this.busy = true;
    this.error = null;
    try {
      if (this.doc) await api.closeDocument(this.doc.id).catch(() => {});
      this.resetView();
      const doc = await api.openDocument(path, password);
      this.doc = doc;
      this.passwordPrompt = null;
      this.applyFit();
      this.outline = await api.outline(doc.id).catch(() => []);
    } catch (e) {
      if (isSheafError(e) && e.kind === "password_required") {
        this.passwordPrompt = { path };
      } else {
        this.error = isSheafError(e) ? e.message : String(e);
      }
    } finally {
      this.busy = false;
    }
  }

  async close() {
    if (this.doc) await api.closeDocument(this.doc.id).catch(() => {});
    this.doc = null;
    this.resetView();
  }

  private resetView() {
    this.cache.clear();
    this.inflight.clear();
    this.cacheOrder = [];
    this.outline = [];
    this.currentPage = 0;
    this.rotation = 0;
    this.searchHits = [];
    this.searchIndex = -1;
    this.searchQuery = "";
  }

  setViewport(w: number, h: number) {
    if (w === this.viewportWidth && h === this.viewportHeight) return;
    this.viewportWidth = w;
    this.viewportHeight = h;
    this.applyFit();
  }

  private applyFit() {
    if (!this.doc) return;
    const rotated = this.rotation % 180 !== 0;
    const pages = this.doc.pages.map((p) =>
      rotated ? { width: p.height, height: p.width } : { width: p.width, height: p.height },
    );
    if (this.fitMode === "width") this.zoom = fitWidthZoom(pages, this.viewportWidth);
    else if (this.fitMode === "page") this.zoom = fitPageZoom(pages, this.viewportWidth, this.viewportHeight);
  }

  setFit(mode: FitMode) {
    this.fitMode = mode;
    this.applyFit();
  }

  setZoom(z: number) {
    this.fitMode = "custom";
    this.zoom = clampZoom(z);
  }

  zoomIn() {
    this.setZoom(nextZoom(this.zoom, 1));
  }
  zoomOut() {
    this.setZoom(nextZoom(this.zoom, -1));
  }

  rotateView(delta: 90 | -90) {
    this.rotation = (this.rotation + delta + 360) % 360;
    this.cache.clear();
    this.cacheOrder = [];
    this.applyFit();
  }

  /** Returns a data URL for the page at the current render scale, cached. */
  async pageImage(index: number): Promise<string> {
    const doc = this.doc;
    if (!doc) throw new Error("no document");
    const scale = Math.round(this.renderScale * 1000) / 1000;
    const key = `${doc.id}:${index}@${scale}r${this.rotation}`;
    const hit = this.cache.get(key);
    if (hit) return hit;
    const pending = this.inflight.get(key);
    if (pending) return pending;
    const p = api
      .renderPage(doc.id, index, scale, this.rotation)
      .then((r) => {
        const url = `data:image/png;base64,${r.png_base64}`;
        this.cache.set(key, url);
        this.cacheOrder.push(key);
        while (this.cacheOrder.length > this.cacheLimit) {
          const old = this.cacheOrder.shift();
          if (old) this.cache.delete(old);
        }
        return url;
      })
      .finally(() => this.inflight.delete(key));
    this.inflight.set(key, p);
    return p;
  }

  async runSearch(query: string) {
    this.searchQuery = query;
    if (!this.doc || !query.trim()) {
      this.searchHits = [];
      this.searchIndex = -1;
      return;
    }
    const hits = await api.search(this.doc.id, query, false).catch(() => []);
    this.searchHits = hits;
    this.searchIndex = hits.length ? 0 : -1;
  }

  nextHit(dir: 1 | -1) {
    if (!this.searchHits.length) return;
    const n = this.searchHits.length;
    this.searchIndex = (this.searchIndex + dir + n) % n;
  }
}

export const docStore = new DocumentStore();
