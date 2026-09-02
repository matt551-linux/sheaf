<script lang="ts">
  // Popup editor for an annotation's note text, author and color.
  import { docStore } from "$lib/stores/document.svelte";
  import type { Annotation } from "$lib/api";
  import { colorToHex, hexToColor } from "$lib/viewer/geometry";

  interface Props {
    annotation: Annotation;
    onClose: () => void;
  }
  let { annotation, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let contents = $state(annotation.contents);
  // svelte-ignore state_referenced_locally
  let author = $state(annotation.author);
  // svelte-ignore state_referenced_locally
  let color = $state(colorToHex(annotation.color));
  let ta: HTMLTextAreaElement;
  $effect(() => {
    ta?.focus();
  });

  async function apply() {
    const patch: Record<string, unknown> = {};
    if (contents !== annotation.contents) patch.contents = contents;
    if (author !== annotation.author) patch.author = author;
    if (color !== colorToHex(annotation.color)) patch.color = hexToColor(color);
    if (Object.keys(patch).length) await docStore.updateAnnotation(annotation.page_index, annotation.index, patch);
    onClose();
  }
  const isText = $derived(annotation.kind === "freetext");
</script>

<div class="absolute inset-0 z-20 flex items-center justify-center bg-black/30" role="presentation" onclick={(e) => e.target === e.currentTarget && onClose()}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    class="w-96 rounded bg-white p-4 shadow-xl dark:bg-neutral-900 dark:text-neutral-100"
    onsubmit={(e) => (e.preventDefault(), apply())}
    onkeydown={(e) => {
      if (e.key === "Escape") onClose();
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) apply();
    }}
  >
    <div class="mb-2 flex items-center gap-2">
      <h2 class="font-semibold">{isText ? "Text box" : "Comment"}</h2>
      <span class="text-xs text-neutral-500">page {annotation.page_index + 1}</span>
      <span class="flex-1"></span>
      <input type="color" class="h-6 w-8 cursor-pointer border-0 bg-transparent p-0" bind:value={color} title="Color" />
    </div>
    <textarea bind:this={ta} class="mb-2 h-32 w-full resize-y rounded border border-neutral-300 p-2 text-sm dark:border-neutral-600 dark:bg-neutral-800" bind:value={contents} placeholder={isText ? "Text to show on the page" : "Add a comment"}></textarea>
    <label class="mb-3 flex items-center gap-2 text-xs text-neutral-600 dark:text-neutral-300">
      Author
      <input class="h-7 flex-1 rounded border border-neutral-300 px-1 dark:border-neutral-600 dark:bg-neutral-800" bind:value={author} />
    </label>
    <div class="flex justify-between">
      <button type="button" class="rounded px-3 py-1 text-sm text-red-700 hover:bg-red-100" onclick={() => (docStore.deleteAnnotation(annotation.page_index, annotation.index), onClose())}>Delete</button>
      <div class="flex gap-2">
        <button type="button" class="rounded px-3 py-1 text-sm hover:bg-neutral-200 dark:hover:bg-neutral-700" onclick={onClose}>Cancel</button>
        <button type="submit" class="rounded bg-blue-600 px-3 py-1 text-sm text-white">Apply</button>
      </div>
    </div>
  </form>
</div>
