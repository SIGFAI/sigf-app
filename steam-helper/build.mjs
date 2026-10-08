// Builds sigf-steam in release and puts steam_api64.dll next to it, where the Windows bundle takes both from
// (src-tauri/tauri.windows.conf.json bundle.resources). Run from anywhere: `node app/steam-helper/build.mjs`.
// The Windows `tauri build` runs it first (its beforeBuildCommand).
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
execFileSync('cargo', ['build', '--release', '--locked'], { cwd: root, stdio: 'inherit' });

const release = join(root, 'target', 'release');
const lib = process.platform === 'win32' ? 'steam_api64.dll' : process.platform === 'darwin' ? 'libsteam_api.dylib' : 'libsteam_api.so';
// steamworks-sys copies the SDK's redistributable into its build output; take the newest one.
const build = join(release, 'build');
const found = readdirSync(build)
  .filter((d) => d.startsWith('steamworks-sys-'))
  .map((d) => join(build, d, 'out', lib))
  .filter((p) => existsSync(p))
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
if (!found) {
  console.error(`${lib} not found under ${build}`);
  process.exit(1);
}
copyFileSync(found, join(release, lib));
console.log(`sigf-steam built, ${lib} copied to ${release}`);
