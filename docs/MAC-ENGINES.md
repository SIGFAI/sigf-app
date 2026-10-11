# Play on Mac: engines and core API

Companion to [MAC-PLAY.md](./MAC-PLAY.md) (contract) and [MAC-PLAY-RESEARCH.md](./MAC-PLAY-RESEARCH.md). Hashes were
taken by downloading each file on 2026-10-08; for GitHub assets they match the release API's `digest`.

## 1. Engine list (`MacEngine[]`, served by sigf.ai in `GET /api/app/mac/games`)

| id | kind | url | size | sha256 |
|---|---|---|---|---|
| `wine-sikarugir-10.0_8` | wine | https://github.com/Sikarugir-App/Engines/releases/download/v1.0/WS12WineSikarugir10.0_8.tar.xz | 166321980 | `2a6bcf2a3bf13cddf825599b6bc1e3659cbf71e1282c6d77141a92f6ac8d4c68` |
| `sikarugir-template-1.0.21` | runtime | https://github.com/Sikarugir-App/Wrapper/releases/download/v1.0/Template-1.0.21.tar.xz | 95883080 | `bbe996e4e4375318485953d0c7818b7b4b0a4dc1f13303bcc584f99f7602f78d` |
| `dxmt-0.80` | renderer | https://github.com/3Shain/dxmt/releases/download/v0.80/dxmt-v0.80-builtin.tar.gz | 18681669 | `8f260e36b5739e68f3bad613381441385c4dc7b85b78ba8de653d5a6a264529d` |

```ts
{ id: 'wine-sikarugir-10.0_8', kind: 'wine', size: 166321980, needs: 'sikarugir-template-1.0.21',
  url: 'https://github.com/Sikarugir-App/Engines/releases/download/v1.0/WS12WineSikarugir10.0_8.tar.xz',
  sha256: '2a6bcf2a3bf13cddf825599b6bc1e3659cbf71e1282c6d77141a92f6ac8d4c68' },
{ id: 'sikarugir-template-1.0.21', kind: 'runtime', size: 95883080,
  url: 'https://github.com/Sikarugir-App/Wrapper/releases/download/v1.0/Template-1.0.21.tar.xz',
  sha256: 'bbe996e4e4375318485953d0c7818b7b4b0a4dc1f13303bcc584f99f7602f78d' },
{ id: 'dxmt-0.80', kind: 'renderer', size: 18681669,
  url: 'https://github.com/3Shain/dxmt/releases/download/v0.80/dxmt-v0.80-builtin.tar.gz',
  sha256: '8f260e36b5739e68f3bad613381441385c4dc7b85b78ba8de653d5a6a264529d' },
```

Native-port engines (VCMI, GeneralsX) come from the recipes agent's research; the core takes them as data (`.dmg`,
`.zip`, `.tar.xz`, `.tar.gz` on GitHub releases).

### Native-port engines (batch 2, 2026-10-11)

Upstream release assets only: the app downloads each one from the project's own GitHub release (pinned sha256) on
the player's Mac; SIGF never rehosts or modifies them. Arch / minimum macOS from Info.plist and the binary's
`LC_BUILD_VERSION`; URLs, sizes and hashes in `GET /api/app/mac/games` (the fixture `src-tauri/tests/fixtures/mac-games.json`).

| id | project | license | version | arch, min macOS | games |
|---|---|---|---|---|---|
| `openmw-0.51.0` | OpenMW | GPL-3.0 | 0.51.0 | arm64, 14.8 | morrowind |
| `openrct2-0.5.6` | OpenRCT2 | GPL-3.0 | 0.5.6 | universal, 11.0 (unsigned) | rct2, rctclassic |
| `devilutionx-1.5.5` | DevilutionX | Sustainable Use License (source-available) | 1.5.5 | universal, 11.0 | diablo |
| `fallout1-ce-1.1.0` | Fallout Community Edition | Sustainable Use License (source-available) | 1.1.0 | universal, 11.0 | fallout1 |
| `fallout2-ce-1.3.0` | Fallout 2 Community Edition | Sustainable Use License (source-available) | 1.3.0 | universal, 11.0 | fallout2 |
| `uzdoom-5.0.3` | UZDoom | GPL-3.0 | 5.0.3 | arm64, 12.0 (unsigned) | doom, heretichexen |
| `raze-1.11.0` | Raze | GPL-2.0+ with Build-licensed parts | 1.11.0 | universal, 11.0 | duke3d, blood, shadowwarrior, powerslave |
| `openra-20250330` | OpenRA | GPL-3.0 | release-20250330 | universal, 11.0 | redalert, tiberiandawn |
| `trx-1.11.1` | TRX | GPL-3.0 | 1.11.1 | universal, 12.0 | tombraider1, tombraider2 |
| `julius-1.8.0` | Julius | AGPL-3.0 | 1.8.0 | universal, 11.0 | caesar3 |
| `fs2open-26.0.1` | FreeSpace 2 Open | Volition source licence (non-commercial) | 26.0.1 | arm64, 11.0 | freespace2 |
| `opengothic-0.92` | OpenGothic | MIT | 0.92 | arm64, 15.0 | gothic, gothic2 |
| `corsixth-0.70.1` | CorsixTH | MIT | 0.70.1 | universal, 11.0 | themehospital |
| `descent3-1.5.0` | Descent 3 (open source) | GPL-3.0 | 1.5.0 | arm64, 14.0 | descent3 |
| `openloco-26.09` | OpenLoco | MIT | 26.09 | arm64, 15.0 | locomotion |
| `ja2-stracciatella-0.22.1` | JA2 Stracciatella | SFI-SCLA (non-commercial) | 0.22.1 | arm64, 15.0 | ja2 |
| `dethrace-0.10.1` | dethrace | GPL-3.0 plus a non-commercial clause in its README | 0.10.1 | universal, 15.0 | carmageddon |

License caveats: the Sustainable Use License (DevilutionX, fallout1-ce, fallout2-ce) is source-available, not open
source; FreeSpace 2 Open, JA2 Stracciatella and dethrace forbid commercial use. SIGF only points the app at the
upstream download, never redistributes them, and charges nothing for Play on Mac.

The OpenGothic and Descent 3 zips store their binaries without the execute bit: before a launch the core adds it to
`launch[0]` (resolved inside the engine folder) when it has none. JA2 Stracciatella's asset name holds a `+`, which
GitHub serves as `%2B`; the URL check accepts it (only encoded `.`, `/`, `\` and NUL are refused).

Recommended Wine recipe values (Skyrim SE, The Witcher 3 DX11): `engine: 'wine-sikarugir-10.0_8'`,
`renderer: 'dxmt-0.80'`, `dllOverrides: 'dxgi,d3d11,d3d10core,winemetal=b;nvapi64,nvngx='` (DXMT is installed as
builtin DLLs, so `b`, never `n,b`), `env: { WINEMSYNC: '1', ROSETTA_ADVERTISE_AVX: '1' }`.

### Why these

- **DXMT needs a Wine whose `winemac.so` exposes its Metal hooks.** Its guide asks for "a FOSS CrossOver Wine 24+
  built from the sources", or a Wine patched to export the API in `include/winemacdrv.h`
  ([guide](https://github.com/3Shain/dxmt/wiki/DXMT-Installation-Guide-for-Geeks),
  [runtime spec](https://github.com/3Shain/dxmt/wiki/Device-System-Runtime-Specifications): Wine 8+, macOS 14+).
  Plain WineHQ builds (Gcenx `macOS_Wine_builds`, 11.18) hide those symbols, so they are not used.
- **Sikarugir `WS12WineSikarugir10.0_8`** (2026-10-01) is Wine 10.0 built by Gcenx from the CrossOver source tree
  (the same family GameToMac ships). Inspected: `wswine.bundle/bin/wine` + `wineserver` (x86_64, signed), a wow64
  layout (`lib/wine/x86_64-unix`, `x86_64-windows`, `i386-windows`, no `i386-unix`), `winemac.so` exports
  `_macdrv_functions` with `macdrv_view_create_metal_view` / `macdrv_view_get_metal_layer` (what DXMT 0.80's
  `winemetal.so` looks up), Wine Mono 9.4.0 bundled in `share/wine/mono` (so `wineboot` installs it with no download).
  No symlinks in the archive.
- **Why not Wine 11.** The Sikarugir Wine 11 builds (`WS12WineSikarugir11.0`, `11.0_1`, `WS11WineSikarugir11.0_1`)
  fail on the CI Mac (macOS 15.7, arm64 VM): the first process starts, but every child process (wineboot.exe) exits 1
  right after loading `apisetschema.dll`, so no prefix is ever made (`err:environ:run_wineboot failed to start
  wineboot 1`; a dtrace of exec/spawn shows the child exec'd fine). On the same runner, with the same runtime,
  `WS12WineSikarugir10.0_8` and `WS12WineCX24.0.7_7` boot a prefix and answer `cmd /c ver`, and so does upstream
  WineHQ 11.18 (which lacks the DXMT hooks). Moving to 11.x waits for a build that passes `app-macplay`.
- **The engine is not self-contained.** `wineserver` and several `lib/wine/x86_64-unix/*.so` load `@rpath/libinotify.0.dylib`,
  `libintl.8.dylib`, GStreamer, FFmpeg and GLib libraries, and the binaries carry no `LC_RPATH`: Sikarugir's launcher
  puts the Template's `Contents/Frameworks` on `DYLD_FALLBACK_LIBRARY_PATH` (seen in its binary). The first CI run
  failed on exactly this (`dyld: Library not loaded: @rpath/libinotify.0.dylib`). So the wine engine `needs` the
  **Sikarugir Template 1.0.21** (2026-10-01, built with the engine), kind `runtime`: the core unpacks it with
  `--exclude '*/Frameworks/renderer'` and then deletes any `renderer`, `d3dmetal`, `apple_gptk` or `D3DMetal.framework`
  folder, so **Apple's D3DMetal (`Frameworks/renderer/d3dmetal`) is never written to disk**, and sets
  `DYLD_FALLBACK_LIBRARY_PATH=<runtime>/Template-1.0.21.app/Contents/Frameworks:/usr/local/lib:/usr/lib` for every
  Wine process. Its relative symlinks (dylib versions, fontconfig) stay inside the folder.
- **DXMT v0.80** (2026-04-23) is the latest release and the last one under MIT (later ones move to LGPL). The
  `-builtin` archive holds `x86_64-unix/winemetal.so`, `x86_64-windows/{d3d11,dxgi,d3d10core,winemetal,nvapi64,nvngx}.dll`
  and `i386-windows/{d3d11,dxgi,d3d10core,winemetal}.dll`.
- Both are on rolling or third-party release pages: the sha256 pin makes a replaced asset fail closed (`sha` error).
  Moving them to a SIGFAI release (with the LGPL source offer) is the long-term home.

## 2. Files the core pins itself (not in recipes)

The webview cannot change these; they live in `src-tauri/src/macplay/engine.rs`.

| File | URL | size | sha256 |
|---|---|---|---|
| Windows Steam installer | https://cdn.cloudflare.steamstatic.com/client/installer/SteamSetup.exe (fallback `https://steamcdn-a.akamaihd.net/client/installer/SteamSetup.exe`; unchanged since 2024-05-20) | 2380800 | `7d3654531c32d941b8cae81c4137fc542172bfa9635f169cb392f245a0a12bcb` |
| Wine Mono 11.2.1 (verb `mono`, only when the engine bundles none) | https://github.com/wine-mono/wine-mono/releases/download/wine-mono-11.2.1/wine-mono-11.2.1-x86.msi | 83334144 | `e38810c017ccb64d450bbd6095d41c0b50e3fd967c306f6d2c64c3a48945cc92` |
| VC++ 2015-2022 x64 14.44.35211 (verb `vcrun2022`) | https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe | 25635768 | `cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b` |
| VC++ 2015-2022 x86 14.44.35211 | https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/0C09F2611660441084CE0DF425C51C11E147E6447963C3690F97E0B25C55ED64/VC_redist.x86.exe | 13953392 | `0c09f2611660441084ce0df425c51c11e147e6447963c3690f97e0b25c55ed64` |

Download hosts (start and redirects): `github.com`, `objects.githubusercontent.com`,
`release-assets.githubusercontent.com`, `cdn.cloudflare.steamstatic.com`, `steamcdn-a.akamaihd.net`,
`download.visualstudio.microsoft.com`. Recipe engine URLs: GitHub release assets only
(`github.com/<owner>/<repo>/releases/download/<tag>/<file>` or the two asset hosts), https, no query.

## 3. How the core installs them

- Runtime folder: `engines/<runtime>/` (shared by every engine that needs it), its library folder recorded in
  `.sigf-engine.json` (`libDir`).
- Engine folder: `engines/<engine>+<renderer>/` (DXMT overwrites the engine's own d3d11/dxgi, so each pairing gets
  its own copy), `engines/<engine>/` without a renderer. Unpacked with `/usr/bin/tar` (bsdtar: .tar.xz, .tar.gz,
  .zip; refuses `..` and absolute names) or `hdiutil attach -nobrowse -readonly` + `ditto` + `detach` for a .dmg;
  links that leave the folder are deleted; `xattr -dr com.apple.quarantine`; `.sigf-engine.json` written last.
- DXMT, the "builtin" install of its guide: `winemetal.so` → `<wine>/lib/wine/x86_64-unix/` (ad-hoc signed with
  `codesign --force --sign -`, it ships unsigned), its DLLs → `<wine>/lib/wine/x86_64-windows/` and `i386-windows/`,
  then `winemetal.dll` → the prefix's `system32` and `syswow64`.
- Native engines: every `.app` whose signature does not verify gets an ad-hoc `codesign --force --deep --sign -`
  (GeneralsX ships unsigned).
- Prefix (`prefixes/<game>`, no `WINEARCH`: the engine is a wow64-only build and Wine 10 refuses `WINEARCH=wow64`): `wine wineboot -u`, verbs (`mono`: MSI via `msiexec /i … /qn` only if
  the engine has no `share/wine/mono`; `vcrun2022`: both runtimes `/install /quiet /norestart`), `winemetal.dll`,
  then `SteamSetup.exe /S` → `drive_c/Program Files (x86)/Steam/steam.exe`. Each step is recorded in
  `prefixes/<game>/.sigf-prefix.json` and skipped next time; a prefix made by another engine folder is recreated.
- Every process is started without a shell; inherited `WINE*` variables are removed; output goes to
  `logs/<game>.log` (restarted past 20 MB). Env: `WINEPREFIX`, `WINESERVER`, `WINELOADER`,
  `WINEDLLOVERRIDES` (the recipe's, empty during setup), `DYLD_FALLBACK_LIBRARY_PATH` (the runtime's libraries,
  when the engine `needs` one), `WINEDEBUG=-all` unless the recipe sets it, then the recipe's env (`{prefix}`
  expanded).

## 4. Core API for the UI (deviations from MAC-PLAY.md marked *new*)

`gameJson` is either `{ "game": MacGame, "engines": MacEngine[] }` or a MacGame with an `engines` field. Engines the
game does not use are ignored; `name`, `notes`, `tested` are not checked.

| Command | Args | Returns |
|---|---|---|
| `mac_status` | `game`, `gameJson?` (else the recipe saved by the last setup; never set up → `route: null`, all `missing`) | `MacStatus` |
| `mac_setup` | `gameJson` | `MacStatus` (emits `macplay://progress`) |
| `mac_store` | `game` | – (starts Windows Steam with `-cef-disable-gpu -no-cef-sandbox` and the recipe env) |
| `mac_play` | `game`, `gameJson?`, `direct?` (true: the exe without Steam) | – |
| `mac_stop` | `game` | – (`wineserver -k` + what Play started) |
| `mac_remove` | `game`, `what: 'prefix'|'engine'|'all'` | – (`engine` refused with `inUse` while another set-up game uses it) |
| `mac_copy_data` *new* | `gameJson` | `MacStatus` (steam-win port: copy the game folder from Windows Steam again) |
| `mac_pick_data` *new* | `game` | `MacStatus` (folder port: native folder dialog, then copy) |

Errors are `{ code, message }`; codes: `unsupported`, `badRecipe`, `download`, `sha`, `io`, `unpack`, `wine`,
`timeout`, `notReady`, `notSetUp`, `dataMissing`, `macosTooOld`, `inUse`, `launch`, `steam_missing`.

`macplay://progress { game, step, done, total }`: `step` is `runtime` / `engine` / `renderer` / `mono` / `vcrun2022` / `steam`
with `done`/`total` in bytes while downloading, then `unpack`, `prefix`, `mono`, `vcrun2022`, `renderer`, `steam`,
`data` with `0/0` while that step runs, and `ready` at the end.

`MacStatus` fields a route lacks read as done: `native` → `engine`/`prefix` `ready`; a port without Windows Steam →
`prefix` `ready`, `store` `installed`, `game` = whether its data is ready.

### Recipe additions (*new*, checked by the core)

- `MacEngine.kind: 'runtime'` and `MacEngine.needs?: string` (on a wine engine only, naming a runtime engine of the
  list): the runtime is downloaded with the engine and its libraries put on `DYLD_FALLBACK_LIBRARY_PATH`.

- `port.via?: string`: a wine engine id; a port with `data.from: 'steam-win'` needs `via` or a full `wine` block
  (the Windows Steam that downloads its data; `mac_setup` makes the prefix and Steam, `mac_store` opens it).
- `port.env?: Record<string,string>`: same rule as `wine.env`; values may use `{data}` (= `ports/<game>/data`) and
  `{engine}` (GeneralsX: `CNC_GENERALS_ZH_PATH: '{data}'`). `port.launch` args take the same placeholders.
- `port.data.files: ['.']` copies the whole game folder.
- `port.data.markers?: string[]`: paths relative to `~/Library/Application Support`; any present = data ready (VCMI:
  `['vcmi/Data/H3bitmap.lod']`).
- `data.from: 'gog-installer'`: setup downloads the engine only; Play starts `port.launch` even without data (VCMI's
  launcher imports the installer itself).
- `wine.renderer` may be `''` (Wine's own d3d).
- Env keys: `[A-Z][A-Z0-9_]*`, at most 32, values without control characters; `WINEPREFIX`, `WINESERVER`,
  `WINELOADER`, `WINEDLLPATH`, `WINEARCH`, `WINEDLLOVERRIDES`, `PATH`, `HOME`, `TMPDIR`, `USER`, `SHELL`, `DYLD_*`,
  `LD_*` refused. `dllOverrides`: `[A-Za-z0-9_.,;=-]`. `exe`, `files`, `launch[0]`, `markers`: relative `/` paths, no
  `.`/`..`/`\`/`:`. Unknown fields inside `wine`, `port`, `data`, `native` and engine entries are refused.

The scan (macOS) lists Windows Steam games inside prefixes as store `steam` with `prefix: '<game>'` and no `launch`
URI; their `installDir` is accepted for mod installs like any scanned folder.
