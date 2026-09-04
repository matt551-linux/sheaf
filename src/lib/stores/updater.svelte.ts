// Auto-update: check quietly on launch (and on demand), then offer a one-click
// "Update and restart". Never blocks the user; failures are silent unless the
// check was requested explicitly.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none"; version: string }
  | { kind: "available"; update: Update; version: string; notes: string | null; date: string | null }
  | { kind: "downloading"; version: string; received: number; total: number | null }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

class UpdaterStore {
  state = $state<UpdateState>({ kind: "idle" });
  /** Dismissed banner for this version (session only). */
  dismissed = $state<string | null>(null);

  async checkNow(explicit = false): Promise<void> {
    if (this.state.kind === "checking" || this.state.kind === "downloading") return;
    this.state = { kind: "checking" };
    try {
      const update = await check({ timeout: 15_000 });
      if (!update) {
        this.state = { kind: "none", version: "" };
        return;
      }
      this.state = { kind: "available", update, version: update.version, notes: update.body ?? null, date: update.date ?? null };
    } catch (e) {
      // Dev builds and offline machines land here. Only surface when asked.
      this.state = { kind: "error", message: explicit ? String(e) : "" };
    }
  }

  async install(): Promise<void> {
    if (this.state.kind !== "available") return;
    const { update, version } = this.state;
    let received = 0;
    let total: number | null = null;
    this.state = { kind: "downloading", version, received, total };
    try {
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? null;
        else if (ev.event === "Progress") received += ev.data.chunkLength;
        this.state = { kind: "downloading", version, received, total };
      });
      this.state = { kind: "ready", version };
      await relaunch();
    } catch (e) {
      this.state = { kind: "error", message: String(e) };
    }
  }

  dismiss() {
    if (this.state.kind === "available") this.dismissed = this.state.version;
  }
}

export const updater = new UpdaterStore();
