// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import DesktopIntegrationDialog from "./DesktopIntegrationDialog.svelte";
import { docStore } from "$lib/stores/document.svelte";
import { api, type DesktopIntegrationStatus, type MacosInstallStatus, type WindowsInstallStatus } from "$lib/api";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";

vi.mock("@tauri-apps/plugin-store", () => ({ LazyStore: class { async close() {} } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => false) }));
const destroy = vi.fn(async () => {});
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn(() => ({ destroy })) }));
vi.mock("$lib/api", async (original) => ({
  ...await original<typeof import("$lib/api")>(),
  api: {
    desktopIntegrationStatus: vi.fn(),
    windowsInstallStatus: vi.fn(),
    macosInstallStatus: vi.fn(),
    openWindowsInstalledApps: vi.fn(async () => {}),
    deleteLocalAppData: vi.fn(async () => ({ path: "C:\\Users\\alice\\AppData\\Roaming\\org.sheafpdf.sheaf", removed: true })),
    uninstallAppImage: vi.fn(async () => {}),
    installDesktopIntegration: vi.fn(),
    removeDesktopIntegration: vi.fn(),
  },
}));

const linuxUnsupported: DesktopIntegrationStatus = {
  supported: false,
  is_appimage: false,
  integrated: false,
  appimage_path: null,
  is_default_pdf: false,
};
function windowsStatus(patch: Partial<WindowsInstallStatus> = {}): WindowsInstallStatus {
  return {
    supported: true,
    context: "installed",
    install_location: "C:\\Users\\alice\\AppData\\Local\\Sheaf",
    installed_version: "0.2.3",
    app_data_dir: "C:\\Users\\alice\\AppData\\Roaming\\org.sheafpdf.sheaf",
    app_data_present: true,
    ...patch,
  };
}
function macosUnsupported(): MacosInstallStatus {
  return {
    supported: false,
    app_bundle_path: null,
    is_dev_checkout: false,
    is_running_from_disk_image: false,
    can_move_to_trash: false,
    app_data_path: null,
  };
}

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;
const onClose = vi.fn();

async function settle() {
  for (let i = 0; i < 12; i++) {
    await Promise.resolve();
    flushSync();
  }
}
async function open(status: WindowsInstallStatus | null, desktop = linuxUnsupported) {
  vi.mocked(api.desktopIntegrationStatus).mockResolvedValue(desktop);
  vi.mocked(api.windowsInstallStatus).mockResolvedValue(status ?? windowsStatus({ supported: false }));
  vi.mocked(api.macosInstallStatus).mockResolvedValue(macosUnsupported());
  component = mount(DesktopIntegrationDialog, { target, props: { onClose } });
  await settle();
}
function button(label: string): HTMLButtonElement {
  const found = [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === label);
  if (!found) throw new Error(`no button labelled ${label}`);
  return found;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ask).mockResolvedValue(false);
  target = document.createElement("div");
  document.body.append(target);
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
});

test("installed build: uninstall hands off to Windows Installed apps and keeps data", async () => {
  await open(windowsStatus());
  expect(target.textContent).toContain("Sheaf 0.2.3 is installed");
  expect(target.textContent).toContain("C:\\Users\\alice\\AppData\\Local\\Sheaf");
  expect(target.textContent).toContain("Your preferences, recent files, OCR models, and signing identities are kept");
  const uninstall = button("Uninstall Sheaf…");
  expect(uninstall.disabled).toBe(false);
  uninstall.click();
  await settle();
  expect(api.openWindowsInstalledApps).toHaveBeenCalledTimes(1);
  expect(target.textContent).toContain("Find Sheaf in the list and choose Uninstall");
  // The default path never removes data or closes the app.
  expect(api.deleteLocalAppData).not.toHaveBeenCalled();
  expect(destroy).not.toHaveBeenCalled();
  expect(ask).not.toHaveBeenCalled();
});

test("delete local data is confirmation-protected and defaults to keeping data", async () => {
  const closePrefs = vi.spyOn(docStore, "closePrefs").mockResolvedValue();
  await open(windowsStatus());
  expect(target.textContent).toContain("C:\\Users\\alice\\AppData\\Roaming\\org.sheafpdf.sheaf");
  button("Delete local Sheaf data…").click();
  await settle();
  expect(ask).toHaveBeenCalledTimes(1);
  const [message, options] = vi.mocked(ask).mock.calls[0];
  expect(message).toContain("Your PDF files are not touched");
  expect(options).toMatchObject({ kind: "warning", okLabel: "Delete data", cancelLabel: "Keep data" });
  expect(closePrefs).not.toHaveBeenCalled();
  expect(api.deleteLocalAppData).not.toHaveBeenCalled();
  expect(destroy).not.toHaveBeenCalled();
});

test("confirming delete local data detaches prefs, deletes, then closes Sheaf", async () => {
  const order: string[] = [];
  const closePrefs = vi.spyOn(docStore, "closePrefs").mockImplementation(async () => void order.push("closePrefs"));
  vi.mocked(api.deleteLocalAppData).mockImplementation(async () => {
    order.push("delete");
    return { path: "x", removed: true };
  });
  destroy.mockImplementation(async () => void order.push("destroy"));
  vi.mocked(ask).mockResolvedValue(true);
  await open(windowsStatus());
  button("Delete local Sheaf data…").click();
  await settle();
  expect(closePrefs).toHaveBeenCalledTimes(1);
  expect(api.deleteLocalAppData).toHaveBeenCalledTimes(1);
  expect(getCurrentWindow).toHaveBeenCalled();
  expect(order).toEqual(["closePrefs", "delete", "destroy"]);
  expect(api.openWindowsInstalledApps).not.toHaveBeenCalled();
});

test("a failed deletion surfaces the error and keeps the app open", async () => {
  vi.spyOn(docStore, "closePrefs").mockResolvedValue();
  vi.mocked(ask).mockResolvedValue(true);
  vi.mocked(api.deleteLocalAppData).mockRejectedValue({ kind: "engine", message: "refusing to delete local data: path is not the Sheaf app-data directory" });
  await open(windowsStatus());
  button("Delete local Sheaf data…").click();
  await settle();
  expect(destroy).not.toHaveBeenCalled();
  expect(target.textContent).toContain("refusing to delete local data");
  expect(button("Delete local Sheaf data…").disabled).toBe(false);
});

test("dev build: both controls are unavailable and explained", async () => {
  await open(windowsStatus({ context: "dev" }));
  expect(target.textContent).toContain("development build");
  expect(button("Uninstall Sheaf…").disabled).toBe(true);
  expect(button("Delete local Sheaf data…").disabled).toBe(true);
  button("Delete local Sheaf data…").click();
  button("Uninstall Sheaf…").click();
  await settle();
  expect(ask).not.toHaveBeenCalled();
  expect(api.openWindowsInstalledApps).not.toHaveBeenCalled();
  expect(api.deleteLocalAppData).not.toHaveBeenCalled();
});

test("portable build: nothing to uninstall, data removal still offered", async () => {
  await open(windowsStatus({ context: "portable", install_location: null, installed_version: null }));
  expect(target.textContent).toContain("not registered with Windows");
  expect(target.textContent).toContain("delete its folder");
  expect(button("Uninstall Sheaf…").disabled).toBe(true);
  expect(button("Delete local Sheaf data…").disabled).toBe(false);
});

test("portable build points at a separately installed copy", async () => {
  await open(windowsStatus({ context: "portable" }));
  expect(target.textContent).toContain("A separate installed copy (0.2.3) is registered at");
  expect(target.textContent).toContain("C:\\Users\\alice\\AppData\\Local\\Sheaf");
  expect(button("Uninstall Sheaf…").disabled).toBe(true);
});

test("Linux AppImage status never consults the Windows install state", async () => {
  await open(null, { supported: true, is_appimage: true, integrated: false, appimage_path: null, is_default_pdf: false });
  expect(api.windowsInstallStatus).not.toHaveBeenCalled();
  expect(target.textContent).toContain("Integrate");
  expect(target.textContent).not.toContain("Delete local Sheaf data");
});

test("other platforms without install support show the Linux-only note", async () => {
  await open(windowsStatus({ supported: false }));
  expect(target.textContent).toContain("Linux AppImage only");
  expect(target.textContent).not.toContain("Delete local Sheaf data");
});
