(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const btn = (root, t) => [...root.querySelectorAll('button')].find((b) => b.textContent.trim().startsWith(t));
  const setVal = (el, v) => { el.value = v; el.dispatchEvent(new Event('input', { bubbles: true })); el.dispatchEvent(new Event('change', { bubbles: true })); };
  const log = [];

  [...document.querySelectorAll('button')].find((b) => /^Sign/.test(b.textContent.trim())).click();
  await sleep(400);
  const panel = document.querySelector('[data-panel=security]');
  if (!panel) return 'no panel';
  log.push('panel open');

  // create identity
  btn(panel, 'New or import').click(); await sleep(200);
  const inputs = [...panel.querySelectorAll('input')];
  setVal(inputs.find((i) => i.placeholder === 'Your name'), 'Brian Haywood');
  setVal(inputs.find((i) => i.placeholder.startsWith('Organization')), 'Sheaf');
  btn(panel, 'Create self-signed identity').click();
  await sleep(4000);
  const sel = panel.querySelector('#sig-identity');
  log.push('identity: ' + sel.options[sel.selectedIndex]?.text);

  setVal([...panel.querySelectorAll('input')].find((i) => i.placeholder.startsWith('Reason')), 'UI test');
  btn(panel, 'Place signature').click();
  await sleep(200);
  log.push('placing: ' + !!btn(panel, 'Cancel'));

  // drag on page 1
  const page = document.querySelector('[data-page="0"]') || document.querySelector('[data-page="1"]');
  const r = page.getBoundingClientRect();
  const x0 = r.left + r.width * 0.15, y0 = r.top + r.height * 0.80, x1 = r.left + r.width * 0.55, y1 = r.top + r.height * 0.90;
  const ev = (type, x, y) => new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, pointerId: 1, button: 0, buttons: 1, isPrimary: true });
  page.dispatchEvent(ev('pointerdown', x0, y0));
  page.dispatchEvent(ev('pointermove', (x0 + x1) / 2, (y0 + y1) / 2));
  page.dispatchEvent(ev('pointermove', x1, y1));
  page.dispatchEvent(ev('pointerup', x1, y1));
  await sleep(6000);

  const { docStore } = await import('http://localhost:1420/src/lib/stores/document.svelte.ts');
  const sigs = docStore.signatures;
  log.push('signatures: ' + JSON.stringify(sigs.map((s) => ({ signer: s.signer, status: s.status, page: s.page, reason: s.reason, rect: s.rect?.map(Math.round) }))));
  log.push('modified: ' + docStore.doc.modified + ' canUndo: ' + docStore.doc.can_undo);
  log.push('pill: ' + ([...document.querySelectorAll('button')].find((b) => /^Signed/.test(b.textContent.trim()))?.textContent.trim() ?? 'none'));
  log.push('tab shows: ' + (panel.querySelector('[role=tab][aria-selected=true]')?.textContent.trim()));
  return log.join('\n');
})()
