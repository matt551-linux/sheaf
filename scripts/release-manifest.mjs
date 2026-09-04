// Collect signed bundles from one or more tauri build trees into a flat
// dist/ folder and write the updater manifest (latest.json).
//
//   node scripts/release-manifest.mjs <bundlesRoot> <distDir> <tag> <owner/repo>
//
// <bundlesRoot> is searched recursively, so it works for CI artifact
// downloads (bundles/sheaf-win-x64/release/bundle/...) and for local
// src-tauri/target trees alike. Download URLs point at the GitHub release
// for <tag>, which is where the files end up.
import { promises as fs } from "node:fs";
import path from "node:path";

const [root, dist, tag, repo] = process.argv.slice(2);
if (!root || !dist || !tag || !repo) {
  console.error("usage: release-manifest.mjs <bundlesRoot> <distDir> <tag> <owner/repo>");
  process.exit(2);
}
const version = tag.replace(/^v/, "");

async function walk(dir, out = []) {
  for (const e of await fs.readdir(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) await walk(p, out);
    else out.push(p);
  }
  return out;
}

const keep = /\.(msi|exe|dmg|deb|rpm|AppImage|sig)$|\.app\.tar\.gz(\.sig)?$/;
const files = (await walk(root)).filter((f) => keep.test(f) && !/\.pdb$/.test(f));
await fs.mkdir(dist, { recursive: true });
const names = new Map();
for (const f of files) {
  const name = path.basename(f);
  if (names.has(name)) continue; // same bundle reached via two paths
  names.set(name, f);
  await fs.copyFile(f, path.join(dist, name));
}

// Updater platforms -> the asset the updater downloads for that platform.
// Windows prefers NSIS (exe), Linux uses the AppImage, macOS the app tarball.
const pick = (re) => [...names.keys()].find((n) => re.test(n));
const platforms = {
  "windows-x86_64": pick(/_x64-setup\.exe$/) ?? pick(/_x64_en-US\.msi$/),
  "windows-aarch64": pick(/_arm64-setup\.exe$/) ?? pick(/_arm64_en-US\.msi$/),
  "darwin-x86_64": pick(/\.app\.tar\.gz$/),
  "darwin-aarch64": pick(/\.app\.tar\.gz$/),
  "linux-x86_64": pick(/_amd64\.AppImage$/),
  "linux-aarch64": pick(/_aarch64\.AppImage$/),
};
const manifest = { version, notes: `Sheaf ${version}`, pub_date: new Date().toISOString(), platforms: {} };
for (const [platform, asset] of Object.entries(platforms)) {
  if (!asset) {
    console.warn(`no bundle for ${platform}`);
    continue;
  }
  const sigFile = names.get(`${asset}.sig`);
  if (!sigFile) {
    console.warn(`no signature for ${asset}; ${platform} will not auto-update`);
    continue;
  }
  manifest.platforms[platform] = {
    signature: (await fs.readFile(sigFile, "utf8")).trim(),
    url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(asset)}`,
  };
}
await fs.writeFile(path.join(dist, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`dist: ${names.size} files, manifest platforms: ${Object.keys(manifest.platforms).join(", ") || "none"}`);
