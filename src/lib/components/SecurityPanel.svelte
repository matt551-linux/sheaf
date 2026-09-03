<script lang="ts">
  // M5: sign and protect. A side panel (not a modal) so the page stays
  // interactive: "Place signature" arms a drag on the page.
  import { docStore } from "$lib/stores/document.svelte";
  import { api, errorMessage, type Identity, type SecuritySpec, type SignatureInfo } from "$lib/api";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onClose: () => void;
    initialTab?: "sign" | "signatures" | "protect";
  }
  let { onClose, initialTab = "sign" }: Props = $props();

  type Tab = "sign" | "signatures" | "protect";
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

  // ---- identities ----
  let identities = $state<Identity[]>([]);
  let identityId = $state("");
  let identityPassword = $state("");
  let showNewIdentity = $state(false);
  let newName = $state("");
  let newOrg = $state("");
  let newPassword = $state("");
  let importFilePassword = $state("");

  async function loadIdentities() {
    identities = await api.listIdentities();
    if (!identities.find((i) => i.id === identityId)) identityId = identities[0]?.id ?? "";
  }
  $effect(() => {
    void loadIdentities().catch((e) => (error = errorMessage(e)));
  });

  const cn = (dn: string) => dn.split(",").map((s) => s.trim()).find((p) => p.startsWith("CN="))?.slice(3) ?? dn;

  const createIdentity = () =>
    run(async () => {
      if (!newName.trim()) throw new Error("Enter a name for the new identity.");
      const idn = await api.createIdentity(newName.trim(), newOrg.trim() || null, newPassword);
      await loadIdentities();
      identityId = idn.id;
      identityPassword = newPassword;
      showNewIdentity = false;
      newName = newOrg = newPassword = "";
      docStore.showToast(`Created identity ${cn(idn.subject)}`);
    });

  const importIdentity = () =>
    run(async () => {
      const picked = await open({
        multiple: false,
        filters: [{ name: "Digital ID (PKCS#12)", extensions: ["p12", "pfx"] }],
      });
      if (!picked || Array.isArray(picked)) return;
      const idn = await api.importIdentity(picked, importFilePassword, newPassword);
      await loadIdentities();
      identityId = idn.id;
      identityPassword = newPassword;
      showNewIdentity = false;
      importFilePassword = newPassword = "";
      docStore.showToast(`Imported ${cn(idn.subject)}`);
    });

  const deleteIdentity = () =>
    run(async () => {
      if (!identityId) return;
      await api.deleteIdentity(identityId);
      await loadIdentities();
    });

  // ---- signing ----
  let reason = $state("");
  let location = $state("");
  let signName = $state("");
  let lock = $state(false);
  let visible = $state(true);
  let placing = $state(false);

  function startPlacing() {
    if (!doc) return;
    if (!identityId) {
      error = "Choose or create a digital identity first.";
      return;
    }
    error = null;
    placing = true;
    docStore.tool = "select";
    docStore.placingSignature = (page, r) => {
      placing = false;
      void signAt(page, [r.x, r.y, r.x + r.w, r.y + r.h]);
    };
  }
  function cancelPlacing() {
    placing = false;
    docStore.placingSignature = null;
  }
  const signInvisible = () => signAt(docStore.currentPage, [0, 0, 0, 0]);

  const signAt = (page: number, rect: [number, number, number, number]) =>
    run(async () => {
      if (!doc) return;
      const info = await api.signDocument(doc.id, {
        identity_id: identityId,
        password: identityPassword,
        page,
        rect,
        reason: reason || null,
        location: location || null,
        name: signName || null,
        lock,
      });
      docStore.applyStructure(info);
      await docStore.refreshSignatures(info.id);
      tab = "signatures";
      docStore.showToast("Signed. Save to keep the signature.");
    });

  // ---- signatures ----
  const sigs = $derived(docStore.signatures);
  const badge = (s: SignatureInfo) =>
    s.status === "valid"
      ? ["Valid", "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-100"]
      : s.status === "modified"
        ? ["Modified after signing", "bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-100"]
        : s.status === "invalid"
          ? ["Invalid", "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-100"]
          : ["Unknown", "bg-neutral-200 text-neutral-700 dark:bg-neutral-700 dark:text-neutral-200"];
  function fmtDate(s: string | null): string {
    if (!s) return "";
    const m = /^D:(\d{4})(\d{2})(\d{2})(\d{2})?(\d{2})?(\d{2})?/.exec(s);
    return m ? `${m[1]}-${m[2]}-${m[3]} ${m[4] ?? "00"}:${m[5] ?? "00"} UTC` : s;
  }

  // ---- protect ----
  let userPassword = $state("");
  let ownerPassword = $state("");
  let sec = $state<Omit<SecuritySpec, "user_password" | "owner_password">>({
    allow_print: true,
    allow_print_high_quality: true,
    allow_modify: true,
    allow_copy: true,
    allow_annotate: true,
    allow_fill_forms: true,
    allow_assemble: true,
    allow_accessibility: true,
  });
  const permRows: { key: keyof typeof sec; label: string }[] = [
    { key: "allow_print", label: "Printing" },
    { key: "allow_print_high_quality", label: "High-quality printing" },
    { key: "allow_modify", label: "Changing the document" },
    { key: "allow_copy", label: "Copying text and images" },
    { key: "allow_annotate", label: "Commenting" },
    { key: "allow_fill_forms", label: "Filling forms" },
    { key: "allow_assemble", label: "Page assembly (insert, rotate, delete)" },
    { key: "allow_accessibility", label: "Text access for screen readers" },
  ];
  const protect = () =>
    run(async () => {
      if (!doc) return;
      if (!userPassword && !ownerPassword) throw new Error("Enter an open password, a permissions password, or both.");
      const info = await api.protectDocument(doc.id, { user_password: userPassword, owner_password: ownerPassword, ...sec });
      docStore.applyStructure(info);
      await docStore.refreshSignatures(info.id);
      userPassword = ownerPassword = "";
      docStore.showToast("Security applied. Save to keep it.");
    });
  const unprotect = () =>
    run(async () => {
      if (!doc) return;
      const info = await api.unprotectDocument(doc.id);
      docStore.applyStructure(info);
      await docStore.refreshSignatures(info.id);
      docStore.showToast("Security removed. Save to keep it.");
    });

  const btn =
    "inline-flex h-8 items-center justify-center whitespace-nowrap rounded px-2.5 text-sm hover:bg-neutral-200 disabled:opacity-40 disabled:hover:bg-transparent dark:hover:bg-neutral-700";
  const primary = "inline-flex h-8 items-center justify-center whitespace-nowrap rounded bg-blue-600 px-3 text-sm text-white hover:bg-blue-700 disabled:opacity-40";
  const field = "h-8 w-full rounded border border-neutral-300 bg-white px-2 text-sm dark:border-neutral-600 dark:bg-neutral-800";
  const tabBtn = (t: Tab) =>
    `flex-1 border-b-2 px-2 py-1.5 text-sm ${tab === t ? "border-blue-600 font-semibold" : "border-transparent text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200"}`;
</script>

<aside class="flex w-80 shrink-0 flex-col border-l border-neutral-300 bg-neutral-50 text-neutral-800 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100" aria-label="Sign and protect" data-panel="security">
  <div class="flex h-11 shrink-0 items-center border-b border-neutral-300 px-2 dark:border-neutral-700">
    <span class="flex-1 text-sm font-semibold">Sign &amp; protect</span>
    <button class={btn} onclick={() => (cancelPlacing(), onClose())} aria-label="Close">✕</button>
  </div>
  <div class="flex shrink-0 border-b border-neutral-200 dark:border-neutral-800" role="tablist">
    <button class={tabBtn("sign")} role="tab" aria-selected={tab === "sign"} onclick={() => (tab = "sign")}>Sign</button>
    <button class={tabBtn("signatures")} role="tab" aria-selected={tab === "signatures"} onclick={() => (tab = "signatures")}>Signatures{sigs.length ? ` (${sigs.length})` : ""}</button>
    <button class={tabBtn("protect")} role="tab" aria-selected={tab === "protect"} onclick={() => (tab = "protect")}>Protect</button>
  </div>

  {#if error}
    <div class="m-2 rounded bg-red-100 px-2 py-1 text-xs text-red-800 dark:bg-red-900 dark:text-red-100" role="alert">{error}</div>
  {/if}

  <div class="flex-1 overflow-y-auto p-3 text-sm">
    {#if tab === "sign"}
      <label class="block text-xs text-neutral-500" for="sig-identity">Digital identity</label>
      <div class="mt-1 flex gap-1">
        <select id="sig-identity" class={field} bind:value={identityId} disabled={busy || !identities.length}>
          {#if !identities.length}<option value="">No identities yet</option>{/if}
          {#each identities as i}<option value={i.id}>{cn(i.subject)}{i.self_signed ? " (self-signed)" : ""}</option>{/each}
        </select>
        <button class={btn} title="Remove this identity" disabled={!identityId || busy} onclick={deleteIdentity}>🗑</button>
      </div>
      <input class="{field} mt-1" type="password" placeholder="Identity password (if set)" bind:value={identityPassword} autocomplete="off" />
      <button class="{btn} mt-1 w-full justify-start px-0 text-blue-700 hover:bg-transparent hover:underline dark:text-blue-300" onclick={() => (showNewIdentity = !showNewIdentity)}>
        {showNewIdentity ? "Hide" : "New or import identity…"}
      </button>
      {#if showNewIdentity}
        <div class="mt-1 space-y-1 rounded border border-neutral-200 p-2 dark:border-neutral-700">
          <input class={field} placeholder="Your name" bind:value={newName} />
          <input class={field} placeholder="Organization (optional)" bind:value={newOrg} />
          <input class={field} type="password" placeholder="Protect identity with a password (optional)" bind:value={newPassword} autocomplete="new-password" />
          <button class="{primary} w-full" disabled={busy} onclick={createIdentity}>Create self-signed identity</button>
          <div class="pt-1 text-xs text-neutral-500">Or import a .p12 / .pfx from a certificate authority:</div>
          <input class={field} type="password" placeholder="Password of the .p12 file" bind:value={importFilePassword} autocomplete="off" />
          <button class="{btn} w-full border border-neutral-300 dark:border-neutral-600" disabled={busy} onclick={importIdentity}>Import digital ID…</button>
        </div>
      {/if}

      <div class="mt-3 space-y-1">
        <input class={field} placeholder="Reason (optional)" bind:value={reason} />
        <input class={field} placeholder="Location (optional)" bind:value={location} />
        <input class={field} placeholder="Display name (defaults to certificate name)" bind:value={signName} />
        <label class="flex items-center gap-2 text-xs"><input type="checkbox" bind:checked={visible} /> Visible signature box</label>
        <label class="flex items-center gap-2 text-xs"><input type="checkbox" bind:checked={lock} /> Lock document after signing (no further changes allowed)</label>
      </div>

      <div class="mt-3">
        {#if placing}
          <div class="rounded bg-blue-50 p-2 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">Drag a rectangle on the page where the signature should appear.</div>
          <button class="{btn} mt-1 w-full border border-neutral-300 dark:border-neutral-600" onclick={cancelPlacing}>Cancel</button>
        {:else if visible}
          <button class="{primary} w-full" disabled={busy || !doc || !identityId} onclick={startPlacing}>Place signature on page…</button>
        {:else}
          <button class="{primary} w-full" disabled={busy || !doc || !identityId} onclick={signInvisible}>Sign document</button>
        {/if}
      </div>
      <p class="mt-3 text-xs text-neutral-500">Self-signed identities prove the document has not changed since signing, but do not vouch for who you are. For that, import an ID issued by a certificate authority.</p>

    {:else if tab === "signatures"}
      {#if !sigs.length}
        <p class="text-neutral-500">This document has no digital signatures.</p>
      {:else}
        <ul class="space-y-2">
          {#each sigs as s}
            {@const [label, cls] = badge(s)}
            <li class="rounded border border-neutral-200 p-2 dark:border-neutral-700">
              <div class="flex items-center gap-2">
                <span class="font-semibold">{s.signer}</span>
                <span class="ml-auto rounded px-1.5 py-0.5 text-xs {cls}">{label}</span>
              </div>
              <div class="mt-1 text-xs text-neutral-500">
                {#if s.signed_at}<div>Signed {fmtDate(s.signed_at)}</div>{/if}
                {#if s.reason}<div>Reason: {s.reason}</div>{/if}
                {#if s.location}<div>Location: {s.location}</div>{/if}
                {#if s.page != null}<div>Page {s.page + 1}</div>{/if}
                {#if s.issuer}<div class="truncate" title={s.issuer}>Issuer: {cn(s.issuer)}</div>{/if}
                {#if s.locks_document}<div>Locks the document against changes</div>{/if}
                {#each s.messages as m}<div class="mt-1 text-amber-700 dark:text-amber-300">{m}</div>{/each}
              </div>
            </li>
          {/each}
        </ul>
        <button class="{btn} mt-2 w-full border border-neutral-300 dark:border-neutral-600" disabled={busy} onclick={() => docStore.refreshSignatures()}>Re-check</button>
      {/if}

    {:else}
      {#if doc?.encrypted}
        <div class="mb-3 rounded bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">
          This document is encrypted. Applying new settings replaces the current security; removing it makes the file open freely.
        </div>
        <button class="{btn} mb-3 w-full border border-neutral-300 dark:border-neutral-600" disabled={busy} onclick={unprotect}>Remove security</button>
      {/if}
      {#if sigs.length}
        <div class="mb-3 rounded bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">
          Encrypting rewrites the file and will invalidate its {sigs.length} existing signature{sigs.length > 1 ? "s" : ""}.
        </div>
      {/if}
      <label class="block text-xs text-neutral-500" for="sec-user">Password to open</label>
      <input id="sec-user" class="{field} mt-1" type="password" placeholder="Leave empty so anyone can open" bind:value={userPassword} autocomplete="new-password" />
      <label class="mt-2 block text-xs text-neutral-500" for="sec-owner">Password to change permissions</label>
      <input id="sec-owner" class="{field} mt-1" type="password" placeholder="Leave empty to reuse the open password" bind:value={ownerPassword} autocomplete="new-password" />
      <div class="mt-3 text-xs text-neutral-500">Allow</div>
      <ul class="mt-1 space-y-1">
        {#each permRows as p}
          <li><label class="flex items-center gap-2 text-xs"><input type="checkbox" checked={sec[p.key]} onchange={(e) => (sec = { ...sec, [p.key]: (e.currentTarget as HTMLInputElement).checked })} /> {p.label}</label></li>
        {/each}
      </ul>
      <button class="{primary} mt-3 w-full" disabled={busy || !doc} onclick={protect}>Apply AES-256 encryption</button>
      <p class="mt-3 text-xs text-neutral-500">Permissions are enforced by well-behaved readers; only the open password provides real confidentiality.</p>
    {/if}
  </div>
</aside>
