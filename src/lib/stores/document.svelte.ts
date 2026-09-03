// Document session state: the open document, viewport, tools, annotations,
// render cache. Svelte 5 runes in a .svelte.ts module so components share it.
import {
  api,
  errorMessage,
  isSheafError,
  type Annotation,
  type AnnotationPatch,
  type AnnotationSpec,
  type Color,
  type DocumentInfo,
  type FormField,
  type OutlineNode,
  type PageText,
  type Rect,
  type SearchHit,
  type SignatureInfo,
} from "$lib/api";
import {
  clampZoom,
  fitPageZoom,
  fitWidthZoom,
  layoutContinuous,
  layoutTwoUp,
  nextZoom,
  type FitMode,
  type PageLayout,
} from "$lib/viewer/layout";
import { LazyStore } from "@tauri-apps/plugin-store";

export type NavPanel = "none" | "thumbnails" | "bookmarks" | "search" | "comments" | "attachments";
export type ViewMode = "continuous" | "single" | "twoup";
export type Theme = "light" | "dark";
export type Tool =
  | "select"
  | "hand"
  | "highlight"
  | "underline"
  | "strikeout"
  | "squiggly"
  | "note"
  | "freetext"
  | "ink"
  | "square"
  | "circle"
  | "eraser";

export interface ToolStyle {
  color: Color;
  interior: Color | null;
  width: number;
  fontSize: number;
}

export interface Selection {
  page: number;
  start: number;
  end: number;
}

export interface Selected {
  page: number;
  index: number;
}

const DEFAULT_STYLES: Record<Tool, ToolStyle> = {
  select: { color: { r: 0, g: 0, b: 0 }, interior: null, width: 1, fontSize: 12 },
  hand: { color: { r: 0, g: 0, b: 0 }, interior: null, width: 1, fontSize: 12 },
  highlight: { color: { r: 255, g: 235, b: 59 }, interior: null, width: 1, fontSize: 12 },
  underline: { color: { r: 33, g: 150, b: 243 }, interior: null, width: 1, fontSize: 12 },
  strikeout: { color: { r: 244, g: 67, b: 54 }, interior: null, width: 1, fontSize: 12 },
  squiggly: { color: { r: 76, g: 175, b: 80 }, interior: null, width: 1, fontSize: 12 },
  note: { color: { r: 255, g: 193, b: 7 }, interior: null, width: 1, fontSize: 12 },
  freetext: { color: { r: 0, g: 0, b: 0 }, interior: null, width: 1, fontSize: 12 },
  ink: { color: { r: 211, g: 47, b: 47 }, interior: null, width: 2, fontSize: 12 },
  square: { color: { r: 211, g: 47, b: 47 }, interior: null, width: 2, fontSize: 12 },
  circle: { color: { r: 211, g: 47, b: 47 }, interior: null, width: 2, fontSize: 12 },
  eraser: { color: { r: 0, g: 0, b: 0 }, interior: null, width: 1, fontSize: 12 },
};

const prefs = new LazyStore("sheaf-prefs.json");

class DocumentStore {
  doc = $state<DocumentInfo | null>(null);
  outline = $state<OutlineNode[]>([]);
  error = $state<string | null>(null);
  toast = $state<string | null>(null);
  passwordPrompt = $state<{ path: string; wrong: boolean } | null>(null);
  busy = $state(false);

  zoom = $state(1);
  fitMode = $state<FitMode>("width");
  rotation = $state(0);
  viewMode = $state<ViewMode>("continuous");
  theme = $state<Theme>("light");
  nightMode = $state(false);
  currentPage = $state(0);
  navPanel = $state<NavPanel>("thumbnails");
  recents = $state<string[]>([]);

  tool = $state<Tool>("select");
  styles = $state<Record<Tool, ToolStyle>>(structuredClone(DEFAULT_STYLES));
  author = $state("");

  /** Text layer per page, loaded lazily. */
  texts = $state<Record<number, PageText>>({});
  selection = $state<Selection | null>(null);

  /** Annotations per page. */
  annots = $state<Record<number, Annotation[]>>({});
  selected = $state<Selected | null>(null);

  /** Form fields per page (widgets), loaded lazily like annotations. */
  formFields = $state<Record<number, FormField[]>>({});
  /** Show interactive form field overlays. */
  formMode = $state(true);

  /** Digital signatures found in the open document (refreshed on open/sign/save). */
  signatures = $state<SignatureInfo[]>([]);
  /** While set, the next drag on a page places a signature there. */
  placingSignature = $state<((page: number, rect: Rect) => void) | null>(null);

  searchQuery = $state("");
  searchHits = $state<SearchHit[]>([]);
  searchIndex = $state(-1);

  viewportWidth = $state(800);
  viewportHeight = $state(600);

  /** Bumped whenever page bitmaps must be re-fetched (annotation edits, undo). */
  renderVersion = $state(0);

  private cache = new Map<string, string>();
  private inflight = new Map<string, Promise<string>>();
  private cacheOrder: string[] = [];
  private readonly cacheLimit = 40;
  private toastTimer: ReturnType<typeof setTimeout> | null = null;

  get renderScale(): number {
    const dpr = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    return this.zoom * dpr * (96 / 72);
  }

  /** Page sizes in points after view rotation. */
  pageSizes = $derived.by(() => {
    if (!this.doc) return [];
    const rotated = this.rotation % 180 !== 0;
    return this.doc.pages.map((p) =>
      rotated ? { width: p.height, height: p.width } : { width: p.width, height: p.height },
    );
  });

  layout = $derived.by((): PageLayout[] => {
    const sizes = this.pageSizes;
    if (!sizes.length) return [];
    if (this.viewMode === "twoup") return layoutTwoUp(sizes, this.zoom);
    if (this.viewMode === "single") {
      const one = layoutContinuous([sizes[this.currentPage] ?? sizes[0]], this.zoom);
      return one.map((l) => ({ ...l, index: Math.min(this.currentPage, sizes.length - 1) }));
    }
    return layoutContinuous(sizes, this.zoom);
  });

  selectedAnnotation = $derived.by((): Annotation | null => {
    const s = this.selected;
    if (!s) return null;
    return this.annots[s.page]?.find((a) => a.index === s.index) ?? null;
  });

  // ----- prefs -----

  async loadPrefs() {
    try {
      this.recents = ((await prefs.get<string[]>("recents")) ?? []).slice(0, 10);
      this.theme = (await prefs.get<Theme>("theme")) ?? this.theme;
      this.author = (await prefs.get<string>("author")) ?? "";
      const styles = await prefs.get<Partial<Record<Tool, ToolStyle>>>("styles");
      if (styles) this.styles = { ...this.styles, ...styles };
      const vm = await prefs.get<ViewMode>("viewMode");
      if (vm) this.viewMode = vm;
    } catch {
      /* first run */
    }
    this.applyTheme();
  }

  private async savePrefs() {
    try {
      await prefs.set("recents", $state.snapshot(this.recents));
      await prefs.set("theme", this.theme);
      await prefs.set("author", this.author);
      await prefs.set("styles", $state.snapshot(this.styles));
      await prefs.set("viewMode", this.viewMode);
      await prefs.save();
    } catch {
      /* ignore */
    }
  }

  setTheme(t: Theme) {
    this.theme = t;
    this.applyTheme();
    void this.savePrefs();
  }
  private applyTheme() {
    if (typeof document === "undefined") return;
    document.documentElement.classList.toggle("dark", this.theme === "dark");
  }
  setAuthor(a: string) {
    this.author = a;
    void this.savePrefs();
  }
  setStyle(tool: Tool, patch: Partial<ToolStyle>) {
    this.styles = { ...this.styles, [tool]: { ...this.styles[tool], ...patch } };
    void this.savePrefs();
  }

  showToast(msg: string) {
    this.toast = msg;
    if (this.toastTimer) clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => (this.toast = null), 2500);
  }

  // ----- lifecycle -----

  async open(path: string, password?: string) {
    this.busy = true;
    this.error = null;
    try {
      if (this.doc) await api.closeDocument(this.doc.id).catch(() => {});
      this.resetView();
      const doc = await api.openDocument(path, password);
      this.doc = doc;
      void this.refreshSignatures(doc.id);
      this.passwordPrompt = null;
      this.applyFit();
      this.outline = await api.outline(doc.id).catch(() => []);
      this.recents = [path, ...this.recents.filter((r) => r !== path)].slice(0, 10);
      void this.savePrefs();
    } catch (e) {
      if (isSheafError(e) && e.kind === "password_required") {
        this.passwordPrompt = { path, wrong: password !== undefined };
      } else {
        this.error = errorMessage(e);
      }
    } finally {
      this.busy = false;
    }
  }

  async close() {
    if (this.doc) await api.closeDocument(this.doc.id).catch(() => {});
    this.doc = null;
    this.signatures = [];
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
    this.texts = {};
    this.annots = {};
    this.formFields = {};
    this.selection = null;
    this.selected = null;
  }

  private async refreshInfo(info?: DocumentInfo) {
    if (!this.doc) return;
    this.doc = info ?? (await api.documentInfo(this.doc.id));
  }

  // ----- viewport -----

  setViewport(w: number, h: number) {
    if (w === this.viewportWidth && h === this.viewportHeight) return;
    this.viewportWidth = w;
    this.viewportHeight = h;
    this.applyFit();
  }

  private applyFit() {
    const sizes = this.pageSizes;
    if (!sizes.length) return;
    const cols = this.viewMode === "twoup" ? 2 : 1;
    if (this.fitMode === "width") this.zoom = fitWidthZoom(sizes, this.viewportWidth / cols);
    else if (this.fitMode === "page")
      this.zoom = fitPageZoom(sizes, this.viewportWidth / cols, this.viewportHeight);
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
    this.invalidateRenders();
    this.applyFit();
  }
  setViewMode(m: ViewMode) {
    this.viewMode = m;
    this.applyFit();
    void this.savePrefs();
  }

  invalidateRenders() {
    this.cache.clear();
    this.cacheOrder = [];
    this.renderVersion++;
  }

  async pageImage(index: number): Promise<string> {
    const doc = this.doc;
    if (!doc) throw new Error("no document");
    const scale = Math.round(this.renderScale * 1000) / 1000;
    const key = `${doc.id}:${index}@${scale}r${this.rotation}v${this.renderVersion}`;
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

  // ----- text layer -----

  async ensureText(page: number): Promise<PageText | null> {
    if (!this.doc) return null;
    const have = this.texts[page];
    if (have) return have;
    const t = await api.pageText(this.doc.id, page).catch(() => null);
    if (t && this.doc) this.texts = { ...this.texts, [page]: t };
    return t;
  }

  // ----- annotations -----

  async ensureAnnots(page: number, force = false): Promise<Annotation[]> {
    if (!this.doc) return [];
    if (!force && this.annots[page]) return this.annots[page];
    const list = await api.listAnnotations(this.doc.id, page).catch(() => []);
    if (this.doc) this.annots = { ...this.annots, [page]: list };
    return list;
  }

  async ensureFormFields(page: number, force = false): Promise<FormField[]> {
    if (!this.doc) return [];
    if (!force && this.formFields[page]) return this.formFields[page];
    const list = await api.listFormFields(this.doc.id, page).catch(() => []);
    if (this.doc) this.formFields = { ...this.formFields, [page]: list };
    return list;
  }

  /** Set a field value and refresh that page (radio groups may change siblings). */
  async setFormField(page: number, annotIndex: number, value: string): Promise<boolean> {
    if (!this.doc) return false;
    try {
      await api.setFormFieldValue(this.doc.id, page, annotIndex, value);
      this.doc = await api.documentInfo(this.doc.id);
      await this.ensureFormFields(page, true);
      this.invalidateRenders();
      return true;
    } catch (e) {
      this.showToast(`Could not set field: ${errorMessage(e)}`);
      return false;
    }
  }

  /** Empty required fields across the whole document (loads all pages). */
  async findMissingRequired(): Promise<FormField[]> {
    if (!this.doc) return [];
    const missing: FormField[] = [];
    const byName = new Map<string, boolean>(); // radio groups: any checked?
    const all: FormField[] = [];
    for (let p = 0; p < this.doc.page_count; p++) all.push(...(await this.ensureFormFields(p)));
    for (const f of all) {
      if (f.kind === "radio") byName.set(f.name, (byName.get(f.name) ?? false) || f.checked);
    }
    for (const f of all) {
      if (!f.required || f.readonly) continue;
      if (f.kind === "text" || f.kind === "combo") {
        if (!f.value.trim()) missing.push(f);
      } else if (f.kind === "checkbox") {
        if (!f.checked) missing.push(f);
      } else if (f.kind === "radio") {
        if (!byName.get(f.name) && !missing.some((m) => m.name === f.name)) missing.push(f);
      } else if (f.kind === "listbox") {
        if (!f.options.some((o) => o.selected)) missing.push(f);
      }
    }
    return missing;
  }

  private async afterMutation(page: number) {
    this.annots = { ...this.annots, [page]: await api.listAnnotations(this.doc!.id, page).catch(() => []) };
    this.invalidateRenders();
    await this.refreshInfo();
  }

  async addAnnotation(page: number, spec: AnnotationSpec): Promise<Annotation | null> {
    if (!this.doc) return null;
    try {
      const a = await api.addAnnotation(this.doc.id, page, { author: this.author, ...spec });
      await this.afterMutation(page);
      this.selected = { page, index: a.index };
      return a;
    } catch (e) {
      this.showToast(errorMessage(e));
      return null;
    }
  }

  async updateAnnotation(page: number, index: number, patch: AnnotationPatch) {
    if (!this.doc) return;
    try {
      await api.updateAnnotation(this.doc.id, page, index, patch);
      await this.afterMutation(page);
    } catch (e) {
      this.showToast(errorMessage(e));
    }
  }

  async deleteAnnotation(page: number, index: number) {
    if (!this.doc) return;
    try {
      await api.deleteAnnotation(this.doc.id, page, index);
      if (this.selected?.page === page && this.selected.index === index) this.selected = null;
      await this.afterMutation(page);
    } catch (e) {
      this.showToast(errorMessage(e));
    }
  }

  async deleteSelected() {
    const s = this.selected;
    if (s) await this.deleteAnnotation(s.page, s.index);
  }

  private historyBusy = false;
  async undo() {
    if (!this.doc?.can_undo || this.historyBusy) return;
    this.historyBusy = true;
    try {
      const info = await api.undo(this.doc.id).catch((e) => (this.showToast(errorMessage(e)), null));
      if (info) await this.reloadAfterHistory(info);
    } finally {
      this.historyBusy = false;
    }
  }
  async redo() {
    if (!this.doc?.can_redo || this.historyBusy) return;
    this.historyBusy = true;
    try {
      const info = await api.redo(this.doc.id).catch((e) => (this.showToast(errorMessage(e)), null));
      if (info) await this.reloadAfterHistory(info);
    } finally {
      this.historyBusy = false;
    }
  }
  /** Adopt new document info after a structural change (pages added,
   * removed, moved, rotated, cropped, stamped): all caches are stale. */
  async refreshSignatures(id = this.doc?.id) {
    if (id == null) return;
    try {
      const sigs = await api.listSignatures(id);
      if (this.doc?.id === id) this.signatures = sigs;
    } catch {
      this.signatures = [];
    }
  }

  applyStructure(info: DocumentInfo) {
    this.doc = info;
    this.selected = null;
    this.selection = null;
    this.texts = {};
    this.annots = {};
    this.formFields = {};
    this.currentPage = Math.min(this.currentPage, Math.max(0, info.page_count - 1));
    this.invalidateRenders();
  }

  private async reloadAfterHistory(info: DocumentInfo) {
    // Undo/redo can revert structural changes (page add/remove/move), so
    // every index-keyed cache is suspect. Reset them all.
    this.applyStructure(info);
  }

  async save(path: string | null = null, flatten = false): Promise<boolean> {
    if (!this.doc) return false;
    try {
      const info = await api.saveDocument(this.doc.id, path, flatten);
      this.doc = info;
      void this.refreshSignatures(info.id);
      if (flatten) {
        this.annots = {};
        this.invalidateRenders();
      }
      if (path) this.recents = [info.path, ...this.recents.filter((r) => r !== info.path)].slice(0, 10);
      void this.savePrefs();
      this.showToast(`Saved ${info.file_name}`);
      return true;
    } catch (e) {
      this.showToast(`Save failed: ${errorMessage(e)}`);
      return false;
    }
  }

  // ----- search -----

  async runSearch(query: string, caseSensitive = false, wholeWord = false) {
    this.searchQuery = query;
    if (!this.doc || !query.trim()) {
      this.searchHits = [];
      this.searchIndex = -1;
      return;
    }
    const hits = await api.search(this.doc.id, query, caseSensitive, wholeWord).catch(() => []);
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
