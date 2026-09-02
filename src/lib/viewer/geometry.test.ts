import { describe, expect, it } from "vitest";
import {
  charAt,
  cssToPdf,
  hexToColor,
  inkBounds,
  lineRects,
  nearestChar,
  pdfToCss,
  rectToCss,
  rectToPdf,
  rectToQuad,
  selectedText,
  colorToHex,
} from "./geometry";

const page = { width: 600, height: 800 };

describe("coordinate transforms", () => {
  it("maps PDF bottom-left origin to CSS top-left at zoom 1", () => {
    const [x, y] = pdfToCss(0, 0, page, 1, 0);
    expect(x).toBeCloseTo(0);
    expect(y).toBeCloseTo(800 * (96 / 72));
    const [x2, y2] = pdfToCss(600, 800, page, 1, 0);
    expect(x2).toBeCloseTo(600 * (96 / 72));
    expect(y2).toBeCloseTo(0);
  });
  it("round-trips through every rotation", () => {
    for (const rot of [0, 90, 180, 270]) {
      for (const [px, py] of [
        [10, 20],
        [300, 400],
        [590, 790],
      ]) {
        const [cx, cy] = pdfToCss(px, py, page, 1.5, rot);
        const [bx, by] = cssToPdf(cx, cy, page, 1.5, rot);
        expect(bx).toBeCloseTo(px, 4);
        expect(by).toBeCloseTo(py, 4);
      }
    }
  });
  it("rotated 90 puts PDF top-left at CSS top-right", () => {
    const [x, y] = pdfToCss(0, 800, page, 1, 90);
    expect(x).toBeCloseTo(800 * (96 / 72));
    expect(y).toBeCloseTo(0);
  });
  it("rect conversions round-trip", () => {
    const r = { x: 50, y: 60, w: 100, h: 20 };
    for (const rot of [0, 90, 180, 270]) {
      const back = rectToPdf(rectToCss(r, page, 2, rot), page, 2, rot);
      expect(back.x).toBeCloseTo(50, 4);
      expect(back.y).toBeCloseTo(60, 4);
      expect(back.w).toBeCloseTo(100, 4);
      expect(back.h).toBeCloseTo(20, 4);
    }
  });
});

const chars = [
  { ch: "H", x: 0, y: 100, w: 10, h: 12 },
  { ch: "i", x: 10, y: 100, w: 4, h: 12 },
  { ch: "\n", x: 14, y: 100, w: 0, h: 0 },
  { ch: "y", x: 0, y: 80, w: 10, h: 12 },
  { ch: "o", x: 10, y: 80, w: 10, h: 12 },
];

describe("text selection", () => {
  it("hit-tests characters", () => {
    expect(charAt(chars, 5, 106)).toBe(0);
    expect(charAt(chars, 12, 106)).toBe(1);
    expect(charAt(chars, 50, 106)).toBe(1); // nearest on the line
    expect(charAt(chars, 5, 50)).toBe(-1);
  });
  it("finds nearest char across lines", () => {
    expect(nearestChar(chars, 5, 50)).toBe(3);
  });
  it("builds per-line rects and extracts text", () => {
    const rects = lineRects(chars, 0, 4);
    expect(rects).toHaveLength(2);
    expect(rects[0]).toEqual({ x: 0, y: 100, w: 14, h: 12 });
    expect(rects[1]).toEqual({ x: 0, y: 80, w: 20, h: 12 });
    expect(selectedText(chars, 0, 4)).toBe("Hi\nyo");
    expect(selectedText(chars, 4, 3)).toBe("yo");
  });
});

describe("annotation helpers", () => {
  it("makes quadpoints UL UR LL LR", () => {
    expect(rectToQuad({ x: 1, y: 2, w: 3, h: 4 })).toEqual([1, 6, 4, 6, 1, 2, 4, 2]);
  });
  it("bounds ink with padding", () => {
    const b = inkBounds([[[10, 10], [20, 30]]], 2);
    expect(b).toEqual({ x: 8, y: 8, w: 14, h: 24 });
  });
  it("converts hex colors", () => {
    expect(hexToColor("#ff8000")).toEqual({ r: 255, g: 128, b: 0 });
    expect(hexToColor("#f00")).toEqual({ r: 255, g: 0, b: 0 });
    expect(colorToHex({ r: 255, g: 128, b: 0 })).toBe("#ff8000");
  });
});
