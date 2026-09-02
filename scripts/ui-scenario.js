// Drives Sheaf's UI through real DOM events (run via scripts/cdp.py).
// Returns a JSON summary. Used for on-screen verification during development.
new Promise(async (done) => {
  const sleep = (ms) => new Promise((z) => setTimeout(z, ms));
  const st = await import("http://localhost:1420/src/lib/stores/document.svelte.ts");
  const d = st.docStore;
  const tools = () => [...document.querySelectorAll("[aria-label=Tools] button")];
  const pick = (name) => tools().find((b) => b.textContent.trim() === name).click();
  const pg = () => document.querySelector('[data-page="0"]');
  const mk = (t, x, y, id = 1) => {
    const b = pg().getBoundingClientRect();
    return new PointerEvent(t, { bubbles: true, clientX: b.left + x, clientY: b.top + y, button: 0, buttons: 1, pointerId: id, isPrimary: true, pointerType: "mouse" });
  };
  const drag = async (fx0, fy0, fx1, fy1, steps = 8) => {
    const b = pg().getBoundingClientRect();
    const w = b.width, h = b.height;
    pg().dispatchEvent(mk("pointerdown", w * fx0, h * fy0));
    for (let i = 1; i <= steps; i++) {
      pg().dispatchEvent(mk("pointermove", w * (fx0 + ((fx1 - fx0) * i) / steps), h * (fy0 + ((fy1 - fy0) * i) / steps)));
      await sleep(15);
    }
    pg().dispatchEvent(mk("pointerup", w * fx1, h * fy1));
    await sleep(900);
  };
  const log = [];
  await sleep(300);
  pick("Highlight"); await sleep(100);
  await drag(0.14, 0.312, 0.84, 0.312);
  log.push("highlight", d.annots[0]?.length);
  pick("Underline"); await sleep(100);
  await drag(0.14, 0.331, 0.6, 0.331);
  log.push("underline", d.annots[0]?.length);
  pick("Rect"); await sleep(100);
  await drag(0.5, 0.13, 0.9, 0.2);
  log.push("rect", d.annots[0]?.length);
  pick("Pen"); await sleep(100);
  const b = pg().getBoundingClientRect();
  pg().dispatchEvent(mk("pointerdown", b.width * 0.2, b.height * 0.42));
  for (let i = 1; i <= 30; i++) {
    pg().dispatchEvent(mk("pointermove", b.width * (0.2 + i * 0.01), b.height * (0.42 + Math.sin(i / 3) * 0.02)));
    await sleep(10);
  }
  pg().dispatchEvent(mk("pointerup", b.width * 0.5, b.height * 0.42));
  await sleep(1600);
  log.push("ink", d.annots[0]?.length);
  pick("Note"); await sleep(100);
  pg().dispatchEvent(mk("pointerdown", b.width * 0.92, b.height * 0.27));
  await sleep(900);
  log.push("note", d.annots[0]?.length);
  // Close the note editor that opened, after typing a comment.
  const ta = document.querySelector("textarea");
  if (ta) {
    ta.value = "Reviewed by Albert";
    ta.dispatchEvent(new Event("input", { bubbles: true }));
    ta.closest("form").requestSubmit();
    await sleep(900);
  }
  pick("Select");
  const summary = (d.annots[0] || []).map((a) => `${a.kind}${a.contents ? `(${a.contents})` : ""}`);
  done(JSON.stringify({ log, summary, modified: d.doc?.modified, canUndo: d.doc?.can_undo }));
});
