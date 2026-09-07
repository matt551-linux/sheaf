<script lang="ts">
  // Explicit, per-user integration for a portable Linux AppImage, and the
  // matching installation controls on Windows. This is deliberately not
  // automatic: it copies the app and changes desktop state, or removes data.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorMessage, type DesktopIntegrationStatus, type WindowsInstallStatus } from "$lib/api";
  import { docStore } from "$lib/stores/document.svelte";

  interface Props {
    firstRun?: boolean;
    onClose: (dontAskAgain?: boolean) => void;
  }
  let { firstRun = false, onClose }: Props = $props();

  let status = $state<DesktopIntegrationStatus | null>(null);
  let windows = $state<WindowsInstallStatus | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let deleteData = $state(false);

  async function refresh() {
    error = null;
    try {
      const desktop = await api.desktopIntegrationStatus();
      // Windows has no AppImage-style integration; ask for its install state
      // only when the Linux path reports itself unsupported. Assign both at
      // once so the dialog never flashes the "Linux only" text on Windows.
      const install = desktop.supported ? null : await api.windowsInstallStatus();
      status = desktop;
      windows = install;
    } catch (e) {
      error = errorMessage(e);
    }
  }
  onMount(() => void refresh());

  async function integrate() {
    busy = true;
    error = null;
    try {
      status = await api.installDesktopIntegration();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function removeIntegration() {
    const current = status;
    if (current?.is_default_pdf) {
      const ok = await ask(
        "Sheaf is currently the default PDF application. Removing its launcher entry will not select a replacement, so choose another PDF app in your desktop settings afterwards. Continue?",
        { title: "Default PDF application", kind: "warning", okLabel: "Remove integration", cancelLabel: "Cancel" },
      );
      if (!ok) return;
    }
    busy = true;
    error = null;
    try {
      status = await api.removeDesktopIntegration();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function uninstall() {
    const data = deleteData
      ? "This also permanently removes preferences, recents, OCR models, and saved signing identities."
      : "Your preferences, OCR models, and saved signing identities will be kept.";
    const ok = await ask(
      `This removes Sheaf's installed AppImage and launcher integration, then closes Sheaf. ${data} Save any open PDF changes first. Continue?`,
      { title: "Uninstall Sheaf", kind: "warning", okLabel: "Uninstall", cancelLabel: "Cancel" },
    );
    if (!ok) return;
    busy = true;
    error = null;
    try {
      await api.uninstallAppImage(deleteData);
      // The running AppImage remains alive after its installed pathname is
      // unlinked. Exit immediately, after the user explicitly confirmed.
      await getCurrentWindow().destroy();
    } catch (e) {
      error = errorMessage(e);
      busy = false;
    }
  }

  // ----- Windows -----

  /** Hand off to the installer's registered uninstaller; never delete files. */
  async function openInstalledApps() {
    busy = true;
    error = null;
    notice = null;
    try {
      await api.openWindowsInstalledApps();
      notice = "Windows Installed apps is open. Find Sheaf in the list and choose Uninstall. Your preferences and local data stay unless you remove them below.";
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function deleteLocalData() {
    const where = windows?.app_data_dir ? ` (${windows.app_data_dir})` : "";
    const ok = await ask(
      `Permanently delete Sheaf's local data${where}? This removes preferences, recent files, downloaded OCR models, and saved signing identities, then closes Sheaf. Your PDF files are not touched. Save any open PDF changes first.`,
      { title: "Delete local Sheaf data", kind: "warning", okLabel: "Delete data", cancelLabel: "Keep data" },
    );
    if (!ok) return;
    busy = true;
    error = null;
    try {
      // Detach the preferences store first so nothing is flushed back into
      // the directory on exit.
      await docStore.closePrefs();
      await api.deleteLocalAppData();
      await getCurrentWindow().destroy();
    } catch (e) {
      error = errorMessage(e);
      busy = false;
    }
  }

  const action = "rounded px-3 py-1.5 text-sm font-medium disabled:cursor-wait disabled:opacity-50";
  const muted = "text-sm text-neutral-600 dark:text-neutral-300";
  const secondary = `${action} border border-neutral-300 hover:bg-neutral-100 disabled:cursor-not-allowed dark:border-neutral-600 dark:hover:bg-neutral-800`;
  const danger = `${action} bg-red-600 text-white hover:bg-red-700 disabled:cursor-not-allowed`;
</script>

<div class="absolute inset-0 z-40 flex items-center justify-center bg-black/45 p-4" role="dialog" aria-modal="true" aria-label={windows?.supported ? "Installation" : "Desktop integration"}>
  <section class="w-full max-w-lg rounded bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
    <h2 class="text-lg font-semibold">{firstRun ? "Integrate Sheaf with your desktop?" : windows?.supported ? "Installation" : "Desktop integration"}</h2>

    {#if !status && !error}
      <p class="mt-3 {muted}">Checking desktop integration…</p>
    {:else if windows?.supported}
      {#if windows.context === "installed"}
        <p class="mt-3 {muted}">
          Sheaf{windows.installed_version ? ` ${windows.installed_version}` : ""} is installed
          {#if windows.install_location}at <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">{windows.install_location}</code>{/if}
          and registered with Windows.
        </p>
      {:else if windows.context === "dev"}
        <p class="mt-3 rounded bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-100">This is a development build. It shares the same local data directory as an installed Sheaf, so installation and data controls are unavailable here.</p>
      {:else}
        <p class="mt-3 {muted}">This copy of Sheaf is not registered with Windows, so there is nothing to uninstall. To remove a portable or unpacked copy, close Sheaf and delete its folder.</p>
        {#if windows.install_location}
          <p class="mt-2 {muted}">A separate installed copy{windows.installed_version ? ` (${windows.installed_version})` : ""} is registered at <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">{windows.install_location}</code>. Uninstall that one from Windows Installed apps.</p>
        {/if}
      {/if}

      <div class="mt-4 border-t border-neutral-200 pt-4 dark:border-neutral-700">
        <h3 class="font-medium">Uninstall Sheaf…</h3>
        <p class="mt-1 {muted}">Uninstall through Windows Installed apps, using the uninstaller the Sheaf installer registered. This removes the application only. Your preferences, recent files, OCR models, and signing identities are kept, and your PDF files are never touched.</p>
        <button class="{secondary} mt-3" disabled={busy || windows.context !== "installed"} title={windows.context === "installed" ? undefined : "Available only for an installed copy of Sheaf"} onclick={openInstalledApps}>Uninstall Sheaf…</button>
        {#if notice}<p class="mt-2 rounded bg-blue-50 px-3 py-2 text-sm text-blue-900 dark:bg-blue-950 dark:text-blue-100">{notice}</p>{/if}
      </div>

      <div class="mt-4 border-t border-red-200 pt-4 dark:border-red-900">
        <h3 class="font-medium text-red-700 dark:text-red-300">Delete local Sheaf data</h3>
        <p class="mt-1 {muted}">
          Permanently removes preferences, recent files, downloaded OCR models, and saved signing identities
          {#if windows.app_data_dir}from <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">{windows.app_data_dir}</code>{/if}, then closes Sheaf. Uninstalling does not do this. Your PDF files are not touched.
        </p>
        {#if windows.context !== "dev" && !windows.app_data_present}
          <p class="mt-1 {muted}">No local data is present right now.</p>
        {/if}
        <button class="{danger} mt-3" disabled={busy || windows.context === "dev"} title={windows.context === "dev" ? "Unavailable in development builds" : undefined} onclick={deleteLocalData}>Delete local Sheaf data…</button>
      </div>

      <div class="mt-5 flex justify-end"><button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" disabled={busy} onclick={() => onClose()}>Close</button></div>
    {:else if !status?.supported}
      <p class="mt-3 {muted}">Desktop integration is currently available for the Linux AppImage only.</p>
      <div class="mt-5 flex justify-end"><button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" onclick={() => onClose()}>Close</button></div>
    {:else if !status.is_appimage && !status.integrated}
      <p class="mt-3 {muted}">Run Sheaf from an AppImage to install launcher and PDF “Open With” integration. DEB and RPM installs integrate through the package manager.</p>
      <div class="mt-5 flex justify-end"><button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" onclick={() => onClose()}>Close</button></div>
    {:else}
      {#if status.integrated}
        <p class="mt-3 {muted}">Sheaf is installed at a stable per-user path and appears in your app launcher and PDF “Open With” menu.</p>
        {#if status.is_default_pdf}
          <p class="mt-2 rounded bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-100">Sheaf is your current default PDF application. Removal will not choose a replacement automatically.</p>
        {/if}
        <div class="mt-4 border-t border-neutral-200 pt-4 dark:border-neutral-700">
          <h3 class="font-medium">Remove desktop integration</h3>
          <p class="mt-1 {muted}">Keeps the AppImage and your personal data, but removes the launcher, icon, and PDF “Open With” entry.</p>
          <button class="{secondary} mt-3" disabled={busy} onclick={removeIntegration}>Remove integration</button>
        </div>
        <div class="mt-4 border-t border-red-200 pt-4 dark:border-red-900">
          <h3 class="font-medium text-red-700 dark:text-red-300">Uninstall Sheaf</h3>
          <p class="mt-1 {muted}">Removes the installed AppImage and its desktop integration, then closes Sheaf.</p>
          <label class="mt-3 flex items-start gap-2 text-sm">
            <input class="mt-0.5" type="checkbox" bind:checked={deleteData} />
            <span>Also permanently remove preferences, recents, OCR models, and saved signing identities.</span>
          </label>
          <button class="{danger} mt-3" disabled={busy} onclick={uninstall}>Uninstall AppImage…</button>
        </div>
      {:else}
        <p class="mt-3 {muted}">Copy this AppImage to <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">~/.local/opt/sheaf/Sheaf.AppImage</code>, add it to your launcher, and register it as an available PDF application. This never changes your default PDF app.</p>
        <div class="mt-5 flex flex-wrap justify-end gap-2">
          {#if firstRun}
            <button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" disabled={busy} onclick={() => onClose()}>Keep portable</button>
            <button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" disabled={busy} onclick={() => onClose(true)}>Don’t ask again</button>
          {:else}
            <button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" disabled={busy} onclick={() => onClose()}>Close</button>
          {/if}
          <button class="{action} bg-blue-600 text-white hover:bg-blue-700" disabled={busy} onclick={integrate}>Integrate</button>
        </div>
      {/if}
      {#if status.integrated}
        <div class="mt-5 flex justify-end"><button class="{action} hover:bg-neutral-100 dark:hover:bg-neutral-800" disabled={busy} onclick={() => onClose()}>Close</button></div>
      {/if}
    {/if}

    {#if error}<p class="mt-3 rounded bg-red-100 px-3 py-2 text-sm text-red-800 dark:bg-red-950 dark:text-red-100">{error}</p>{/if}
  </section>
</div>
