<script lang="ts">
  import { docStore } from "$lib/stores/document.svelte";

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();
  const d = $derived(docStore.doc);

  function fmtDate(s: string | null): string {
    if (!s) return "";
    const m = /^D:(\d{4})(\d{2})(\d{2})(\d{2})?(\d{2})?(\d{2})?/.exec(s);
    if (!m) return s;
    return `${m[1]}-${m[2]}-${m[3]}${m[4] ? ` ${m[4]}:${m[5] ?? "00"}:${m[6] ?? "00"}` : ""}`;
  }
  function fmtSize(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(2)} MB`;
  }
  // PDF permission bits (Table 22 in ISO 32000-1).
  const perms = $derived(
    d
      ? [
          ["Printing", (d.permissions & 4) !== 0],
          ["Modify contents", (d.permissions & 8) !== 0],
          ["Copy text", (d.permissions & 16) !== 0],
          ["Annotate / fill forms", (d.permissions & 32) !== 0],
          ["Assemble pages", (d.permissions & 1024) !== 0],
          ["High-quality print", (d.permissions & 2048) !== 0],
        ]
      : [],
  );
  const rows = $derived(
    d
      ? [
          ["File", d.path],
          ["Title", d.title ?? ""],
          ["Author", d.author ?? ""],
          ["Subject", d.subject ?? ""],
          ["Keywords", d.keywords ?? ""],
          ["Creator", d.creator ?? ""],
          ["Producer", d.producer ?? ""],
          ["Created", fmtDate(d.creation_date)],
          ["Modified", fmtDate(d.mod_date)],
          ["PDF version", d.pdf_version],
          ["Pages", String(d.page_count)],
          ["Page size", `${(d.pages[0].width / 72).toFixed(2)} x ${(d.pages[0].height / 72).toFixed(2)} in`],
          ["File size", fmtSize(d.file_size)],
          ["Encrypted", d.encrypted ? "Yes" : "No"],
          ["Attachments", String(d.attachments.length)],
        ]
      : [],
  );
</script>

{#if d}
  <div class="absolute inset-0 z-20 flex items-center justify-center bg-black/30" role="presentation" onclick={(e) => e.target === e.currentTarget && onClose()}>
    <div class="w-[32rem] max-w-full rounded bg-white p-4 shadow-xl dark:bg-neutral-900 dark:text-neutral-100" role="dialog" aria-label="Document properties" tabindex="-1" onkeydown={(e) => e.key === "Escape" && onClose()}>
      <h2 class="mb-3 font-semibold">Document properties</h2>
      <table class="w-full text-sm">
        <tbody>
          {#each rows as [k, v]}
            <tr class="border-b border-neutral-100 dark:border-neutral-800">
              <td class="py-1 pr-3 align-top text-neutral-500">{k}</td>
              <td class="break-all py-1">{v}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if d.encrypted}
        <h3 class="mt-3 mb-1 text-sm font-semibold">Permissions</h3>
        <ul class="grid grid-cols-2 gap-x-4 text-xs">
          {#each perms as [k, ok]}
            <li>{ok ? "✓" : "✗"} {k}</li>
          {/each}
        </ul>
      {/if}
      <div class="mt-4 flex justify-end">
        <button class="rounded bg-blue-600 px-3 py-1 text-sm text-white" onclick={onClose}>Close</button>
      </div>
    </div>
  </div>
{/if}
