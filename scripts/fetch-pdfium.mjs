// Downloads the prebuilt PDFium shared library for the current (or requested)
// target into src-tauri/pdfium/<target>/ so pdfium-render can load it at runtime.
// Usage: node scripts/fetch-pdfium.mjs [target]
//   target: win-x64 | win-arm64 | mac-univ | linux-x64 | linux-arm64 (default: host)
import { mkdirSync, existsSync, createWriteStream, readdirSync, copyFileSync, rmSync } from "node:fs";
import { pipeline } from "node:stream/promises";
import { execSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { arch, platform } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const RELEASE = process.env.PDFIUM_RELEASE ?? "chromium/7543";

function hostTarget() {
  // On Windows-on-ARM, Node may be an x64 build under emulation; trust the
  // Rust toolchain host instead when available.
  let rustHost = "";
  try {
    rustHost = execSync("rustc -vV", { encoding: "utf8" }).match(/host: (\S+)/)?.[1] ?? "";
  } catch {}
  const isArm = rustHost.startsWith("aarch64") || arch() === "arm64";
  if (platform() === "win32") return isArm ? "win-arm64" : "win-x64";
  if (platform() === "darwin") return "mac-univ";
  return isArm ? "linux-arm64" : "linux-x64";
}

if (process.argv[2] === "--print-host") {
  console.log(hostTarget());
  process.exit(0);
}
const target = process.argv[2] ?? hostTarget();
const outDir = join(root, "src-tauri", "pdfium", target);
const libName = target.startsWith("win") ? "pdfium.dll" : target.startsWith("mac") ? "libpdfium.dylib" : "libpdfium.so";
if (existsSync(join(outDir, libName)) && !process.env.PDFIUM_FORCE) {
  console.log(`pdfium already present for ${target}: ${join(outDir, libName)}`);
  process.exit(0);
}

const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/${RELEASE}/pdfium-${target}.tgz`;
console.log(`Downloading ${url}`);
mkdirSync(outDir, { recursive: true });
const tgz = join(outDir, "pdfium.tgz");
const res = await fetch(url, { redirect: "follow" });
if (!res.ok) throw new Error(`download failed: ${res.status} ${res.statusText}`);
await pipeline(res.body, createWriteStream(tgz));
// Run from outDir with a relative archive path so GNU tar (git-bash/MSYS)
// does not mistake the "C:" drive prefix for a remote host.
execSync(`tar -xzf pdfium.tgz`, { stdio: "inherit", cwd: outDir });
rmSync(tgz);

// Flatten: the archive puts the library under bin/ (win) or lib/ (mac/linux).
for (const sub of ["bin", "lib"]) {
  const d = join(outDir, sub);
  if (!existsSync(d)) continue;
  for (const f of readdirSync(d)) {
    if (f === libName) copyFileSync(join(d, f), join(outDir, f));
  }
}
console.log(`pdfium ready: ${join(outDir, libName)}`);
