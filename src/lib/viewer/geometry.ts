// Coordinate conversion between PDF user space (points, origin bottom-left,
// y up) and CSS page space (pixels, origin top-left, y down), including view
// rotation. Also selection geometry over the text layer. Pure functions.
import type { Rect, TextChar } from "$lib/api";

export interface PagePoints {
  width: number;
  height: number;
}

/** CSS px per PDF point at a given zoom (96 DPI). */
export function pxPerPt(zoom: number): number {
  return (zoom * 96) / 72;
}

/**
 * Convert a PDF user-space point to unrotated CSS page space (top-left origin).
 */
export function pdfToCss(x: number, y: number, page: PagePoints, zoom: number, rotation = 0): [number, number] {
  const k = pxPerPt(zoom);
  // First to unrotated top-left space, in points.
  let ux = x;
  let uy = page.height - y;
  // Then rotate around the page for the view rotation.
  let rx = ux;
  let ry = uy;
  switch (rotation % 360) {
    case 90:
      rx = page.height - uy;
      ry = ux;
      break;
    case 180:
      rx = page.width - ux;
      ry = page.height - uy;
      break;
    case 270:
      rx = uy;
      ry = page.width - ux;
      break;
  }
  return [rx * k, ry * k];
}

/** Inverse of pdfToCss. */
export function cssToPdf(px: number, py: number, page: PagePoints, zoom: number, rotation = 0): [number, number] {
  const k = pxPerPt(zoom);
  const rx = px / k;
  const ry = py / k;
  let ux = rx;
  let uy = ry;
  switch (rotation % 360) {
    case 90:
      ux = ry;
      uy = page.height - rx;
      break;
    case 180:
      ux = page.width - rx;
      uy = page.height - ry;
      break;
    case 270:
      ux = page.width - ry;
      uy = rx;
      break;
  }
  return [ux, page.height - uy];
}

/** Convert a PDF rect (bottom-left origin) to a CSS rect (top-left origin). */
export function rectToCss(r: Rect, page: PagePoints, zoom: number, rotation = 0): Rect {
  const [x1, y1] = pdfToCss(r.x, r.y, page, zoom, rotation);
  const [x2, y2] = pdfToCss(r.x + r.w, r.y + r.h, page, zoom, rotation);
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
}

/** Convert a CSS rect back to a PDF rect. */
export function rectToPdf(r: Rect, page: PagePoints, zoom: number, rotation = 0): Rect {
  const [x1, y1] = cssToPdf(r.x, r.y, page, zoom, rotation);
  const [x2, y2] = cssToPdf(r.x + r.w, r.y + r.h, page, zoom, rotation);
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
}

/** Normalize a drag from (x0,y0) to (x1,y1) into a positive rect. */
export function normRect(x0: number, y0: number, x1: number, y1: number): Rect {
  return { x: Math.min(x0, x1), y: Math.min(y0, y1), w: Math.abs(x1 - x0), h: Math.abs(y1 - y0) };
}

/** Index of the character whose box contains the point, or the nearest on the same line, or -1. */
export function charAt(chars: TextChar[], x: number, y: number): number {
  let best = -1;
  let bestD = Infinity;
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i];
    if (c.w <= 0 || c.h <= 0) continue;
    if (x >= c.x && x <= c.x + c.w && y >= c.y && y <= c.y + c.h) return i;
    if (y >= c.y && y <= c.y + c.h) {
      const d = x < c.x ? c.x - x : x - (c.x + c.w);
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    }
  }
  return best;
}

/**
 * Character index nearest to a point for selection extension: same-line match
 * preferred, else the closest line's edge char.
 */
export function nearestChar(chars: TextChar[], x: number, y: number): number {
  const direct = charAt(chars, x, y);
  if (direct >= 0) return direct;
  let best = -1;
  let bestD = Infinity;
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i];
    if (c.w <= 0 || c.h <= 0) continue;
    const cy = c.y + c.h / 2;
    const dy = Math.abs(cy - y);
    const dx = x < c.x ? c.x - x : x > c.x + c.w ? x - (c.x + c.w) : 0;
    const d = dy * 4 + dx;
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  }
  return best;
}

/** Merge consecutive char boxes into per-line rects (PDF space). */
export function lineRects(chars: TextChar[], start: number, end: number): Rect[] {
  const out: Rect[] = [];
  const lo = Math.max(0, Math.min(start, end));
  const hi = Math.min(chars.length - 1, Math.max(start, end));
  for (let i = lo; i <= hi; i++) {
    const c = chars[i];
    if (c.w <= 0 || c.h <= 0) continue;
    const mid = c.y + c.h / 2;
    const last = out[out.length - 1];
    if (last && mid >= last.y && mid <= last.y + last.h) {
      const x0 = Math.min(last.x, c.x);
      const y0 = Math.min(last.y, c.y);
      const x1 = Math.max(last.x + last.w, c.x + c.w);
      const y1 = Math.max(last.y + last.h, c.y + c.h);
      out[out.length - 1] = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    } else {
      out.push({ x: c.x, y: c.y, w: c.w, h: c.h });
    }
  }
  return out;
}

/** Selected text between two char indices, inclusive, with line breaks preserved. */
export function selectedText(chars: TextChar[], start: number, end: number): string {
  const lo = Math.max(0, Math.min(start, end));
  const hi = Math.min(chars.length - 1, Math.max(start, end));
  let s = "";
  for (let i = lo; i <= hi; i++) s += chars[i].ch;
  return s.replace(/\r\n?/g, "\n");
}

/** PDF QuadPoints (x1 y1 x2 y2 x3 y3 x4 y4: UL, UR, LL, LR) from a rect. */
export function rectToQuad(r: Rect): number[] {
  return [r.x, r.y + r.h, r.x + r.w, r.y + r.h, r.x, r.y, r.x + r.w, r.y];
}

/** Union of rects. */
export function unionRects(rs: Rect[]): Rect {
  if (!rs.length) return { x: 0, y: 0, w: 0, h: 0 };
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const r of rs) {
    x0 = Math.min(x0, r.x);
    y0 = Math.min(y0, r.y);
    x1 = Math.max(x1, r.x + r.w);
    y1 = Math.max(y1, r.y + r.h);
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Bounding rect of ink paths, padded by half the stroke width. */
export function inkBounds(paths: number[][][], strokeWidth: number): Rect {
  const pts = paths.flat();
  if (!pts.length) return { x: 0, y: 0, w: 0, h: 0 };
  const pad = strokeWidth / 2 + 1;
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const [x, y] of pts) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return { x: x0 - pad, y: y0 - pad, w: x1 - x0 + pad * 2, h: y1 - y0 + pad * 2 };
}

export function colorToCss(c: { r: number; g: number; b: number } | null | undefined, fallback = "#000"): string {
  return c ? `rgb(${c.r},${c.g},${c.b})` : fallback;
}

export function hexToColor(hex: string): { r: number; g: number; b: number } {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h, 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

export function colorToHex(c: { r: number; g: number; b: number } | null | undefined): string {
  if (!c) return "#000000";
  return "#" + [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("");
}
