# SIGF recipe format and app contract

This document describes what the SIGF desktop app reads and does: how it finds games, how it installs a mashup from
its recipe (`mashup.json`), the sigf.ai catalog API it talks to, and how multiplayer lobbies and invite links work.
Section numbers are stable: code comments point at them (`docs/RECIPE-FORMAT.md` section 4, 9.2, ...). Sections 6 to 8
are intentionally not part of this document.

Three parties read the same recipe: the SIGF publisher writes it when a mashup is released, the sigf.ai catalog checks
and serves it, and the app in this repository installs it.

## 1. Principles

1. **The player's copy is the base.** We never ship game files. A mashup that needs a game the player does not own
   shows what is missing and where to get it. A mashup that needs a file only the player may have (a ROM they dumped, a
   library built from decompiled code) gets it on the player's PC (`own_copies`, `player_build`, section 4): nothing
   unshareable is ever shipped by SIGF.
2. **Isolated by default.** A mod installs into an app-managed profile (a Minecraft instance, a `-file` argument, a
   BepInEx doorstop profile), not into the game folder. When a mod must go into the game folder, the app takes a
   snapshot of every file it touches first. "Restore vanilla" is always one click.
3. **Fingerprinted versions.** A mod may declare the game builds it runs on (`games[].builds`), so the app can warn
   before installing on a build it was not made for.
4. **Never into official online modes.** Games with online anti-cheat launch their modded profile offline, or not at
   all. Mashup multiplayer (section 9) runs on the mashup's own lobby or server, never on the publisher's.
5. **Source-respecting.** Outside mods download from their source. We rehost only when the license allows it.
6. **Trust.** Every download is pinned by a sha256 in the recipe and comes only from an allowlist: release files of the
   mod's own SIGFAI repository or of its one pinned upstream release (plus Modrinth's CDN for files listed inside an
   mrpack), checked by the catalog and again by the app itself (section 4, "Download rule in the app"). Every write
   outside the app's own folder is snapshot-backed and undone by Restore.

## 2. Game detection (`src-tauri/src/scan/`)

| Store | Source | Fields |
|---|---|---|
| Steam | `HKCU\Software\Valve\Steam\SteamPath` → `steamapps/libraryfolders.vdf` → `appmanifest_<id>.acf` | appid, name, installdir, buildid, size |
| Epic | `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` | AppName, DisplayName, InstallLocation, AppVersionString |
| Ubisoft Connect | `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>\InstallDir` | id, dir |
| GOG Galaxy | `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` | gameID, gameName, path, ver |
| Minecraft | `%APPDATA%\.minecraft`, Prism (`%APPDATA%\PrismLauncher`), Modrinth App (`%APPDATA%\ModrinthApp`) | launcher, instances |

Each detected game maps to a **canonical game id** (`gta5`, `skyrim`, `minecraft`, ...) through the app's built-in game
list, so the same game bought on two stores matches the same mods. The scan only reads; it never writes.

Launch goes through the store, so the store's DRM and overlay stay intact:
`steam://rungameid/<appid>`, `com.epicgames.launcher://apps/<AppName>?action=launch&silent=true`,
`uplay://launch/<id>/0`, `goggalaxy://openGameView/<id>`. A modded launch that needs extra arguments or a loader uses
the store's launch with arguments (Steam `-applaunch <id> <args>`) or starts the loader exe directly (`launch[].exe`,
section 4).

## 3. Minecraft: use the existing launchers, don't rebuild one

Prism Launcher and the Modrinth App already handle Microsoft accounts, Java, loaders and instances, so the app does not
launch Minecraft itself and never sees a Minecraft account. A Minecraft mod ships as a **`.mrpack`** (Modrinth modpack
format: open, documented, imported by Prism, the Modrinth App and other launchers).

1. Prism found → the app unpacks the `.mrpack` itself into `<Prism data>/instances/<slug>/` (`instance.cfg`,
   `mmc-pack.json`, files checked against the index hashes, `overrides/`), then `prismlauncher.exe --launch <slug>`.
   Prism's `--import` opens a dialog, so it is only the fallback.
2. Else Modrinth App found → open the `.mrpack` with it.
3. Else → the app asks the player to install Prism Launcher (GPL-3.0, a separate program). The player signs in once in
   Prism.

Passthrough crossovers (two games running together, for example GTA V × Minecraft) use the same Prism instance plus
the host game's side.

## 4. The recipe: `mashup.json`

Written by the SIGF publisher when a mashup is released (release assets on the mod's SIGFAI repository, then
`mashup.json` committed to it), checked and served by the catalog API, installed by the app's engine
(`src-tauri/src/install/`). `src-tauri/tests/fixtures/` holds synthetic recipes in the publisher's layout, written by
`src-tauri/tests/make-fixtures.mjs`, that the engine's tests (`src-tauri/tests/recipes.rs`) install and restore.
Unknown fields are ignored by the app.

```json
{
  "id": "sigf/gta5-blocky",                 // "sigf/<repo name>"; the app's registry key
  "version": "1.0.0",                       // also the release tag v1.0.0
  "name": "Blocky Los Santos",
  "tagline": "...",
  "how_to_play": [ "...", "..." ],            // optional: 2-5 in-game lines (goal, controls, where to go)
  "kind": "passthrough",                    // "mod" (no guest) | "mashup" (guest re-made inside the host) | "passthrough" (both games run, linked)
  "games": [
    { "game": "gta5", "role": "host", "engine": "...", "apps": { "steam": "271590" } },
    { "game": "minecraft", "role": "guest", "label": "Minecraft", "mc": "26.3", "loader": "fabric@0.19.5" }
  ],
  "requires": [ { "id": "fabric-loader" }, { "id": "scripthookv" }, { "id": "reshade" } ],
  "install": [
    { "game": "gta5", "strategy": "game-dir-snapshot", "files": [
      { "src": "gta5-blocky-gta5.zip", "dst": "{game}", "unpack": true,
        "contents": [ { "path": "MCPassthrough.asi", "sha256": "..." }, { "path": "reshade-shaders/Shaders/MCPassthrough.fx", "sha256": "..." } ],
        "url": "https://github.com/SIGFAI/gta5-blocky/releases/download/v1.0.0/gta5-blocky-gta5.zip", "sha256": "...", "size": 293 } ] },
    { "game": "minecraft", "strategy": "mrpack",
      "pack": { "src": "gta5-blocky.mrpack", "url": "https://github.com/SIGFAI/gta5-blocky/releases/download/v1.0.0/gta5-blocky.mrpack", "sha256": "...", "size": 540 } }
  ],
  "launch": [
    { "game": "minecraft", "wait": "port:25599" },
    { "game": "gta5", "args": [] }
  ],
  "files": [ { "name": "gta5-blocky.mrpack", "url": "...", "sha256": "...", "size": 540 }, { "name": "gta5-blocky-gta5.zip", "url": "...", "sha256": "...", "size": 293 } ],
  "source": { "repo": "https://github.com/SIGFAI/gta5-blocky", "license": "MIT" },
  "media": { "cover": "https://...", "clip": "https://..." },
  "built_by": { "agent": "SIGF" },
  "server": { "game": "minecraft", "mc": "26.3", "loader": "fabric@0.19.5", "ram_gb": 2, "max_players": 10, "pack": "gta5-blocky.mrpack" },
  "idea_by": "alice", "kit": { "id": "crossover-gta5-minecraft", "status": null }, "built_at": "2026-10-05T00:00:00.000Z",
  "notes": [ "..." ]                         // optional: what the player must know
}
```

Fields, one format each:

- `how_to_play` (optional): 2 to 5 short in-game lines, plain English, no markdown: the goal, the controls, where to
  go ("You spawn wearing the jetpack.", "Hold Jump in the air to fly and shoot.", "Follow the lab hall east, collect
  coins, dodge zappers."). Never install steps (mod folders, jars, loaders, versions, launch options): the app installs
  and launches the mod. The catalog accepts at most 5 strings of at most 160 chars and shows them on the card as
  `howToPlay` (cleaned like `notes`). SIGF writes it per mod after reading the mod's README and source.
- `notes` (optional): real caveats the player must know before playing (requirements the app cannot install, known
  limits). Never a copy of a README's manual install steps.
- `install[].jvm_args` on a Minecraft side built with the SIGF Minecraft kit: `["-Dsigf.player=1"]` (player mode: the
  player joins in survival, no stream camera, no stream demo).
- `kind`: `mod` | `mashup` | `passthrough`. `games[].game` is a canonical id (`[a-z0-9-]`: `gta5`, `minecraft`,
  `tf2`, ...); exactly one `role: "host"`. `apps` is store -> the game's id in that store (Steam app id); `builds`
  (store -> game build ids the mod is known to run on) is optional.
- `requires[]`: `{ id, version?, source?, page?, license?, note?, optional? }`. Without `source` it is a prerequisite
  shown to the player, not downloaded. With `source: { url, sha256 }` the app fetches and verifies it like an install
  file; the catalog only accepts a source that is a release asset of the mod's own repo. A prerequisite whose license
  forbids rehosting (SKSE64, F4SE, the Address Libraries on Nexus) never gets a `source`: `page` is its official
  download page, `license` why it is linked and not shipped, `note` which file to take, `optional: true` a
  recommendation. These four are shown to the player, nothing else reads them.
- `files[]`: every release asset, `{ name, url, sha256, size }`. An install file is fetched from its own `url`, else
  from the `files[]` entry whose `name` is its `src`.
- `install[]`: one step per game (never two for the same game), `{ game, strategy, files }`, or
  `{ game, strategy: "mrpack", pack: { src, url, sha256, size } }`. Strategy names: `args`, `profile`,
  `game-dir-snapshot`, `mrpack` (see the table). The publisher may also copy `loader`, `runtime` (`fivem` for GTA V)
  and `overlay` keys onto the step; the app reads them as information only.
- `install[].jvm_args` (`mrpack` only, optional, at most 16): extra JVM arguments for the Prism instance, written into
  its `instance.cfg` as `OverrideJavaArgs=true` + `JvmArgs="<args joined by spaces>"` (Prism's per-instance override).
  Whitelisted, the same rule in the app (`install/mrpack.rs` `jvm_arg_ok`) and the catalog:
  `-D<key>=<value>` with a key of `[A-Za-z][A-Za-z0-9_.-]*` not starting with `java.`, `javax.`, `jdk.`, `sun.`,
  `com.sun.`, `log4j`, `fabric.`, `org.lwjgl.`, `jna.`, `polyglot.`, `mixin.` (case-insensitive) and a value of
  `[A-Za-z0-9_.+-]*` (no path, URL, space or quote), or `-Xmx` / `-Xms` / `-Xss` with up to 6 digits and an optional
  `k`/`m`/`g`. Nothing else (`-javaagent`, `-XX:`, `-cp`, ...). Example: `["-Dfusion.startHidden=true"]`, so a
  Minecraft window stays hidden while the other game draws everything.
- `install[].files[]`: `{ src, dst, unpack, contents?, root?, url, sha256, size }`.
  - `src`: the asset name. `sha256` pins the downloaded file (always checked, cache included).
  - `dst`: starts with a root placeholder (below), then plain relative segments (no `..`, `:`, `{`, `}`, drive or
    leading `/`). Missing = `{app}/<src>`.
  - `unpack: false`: the file is copied to `dst`, which names the file (`{app}/mod.pk3`, `{game}/cstrike/x.amxx`).
  - `unpack: true`: the file is a zip extracted into the folder `dst` (`{game}` alone = the game folder itself).
    `contents` lists every entry `{ path, sha256 }`; the app refuses an archive that differs from it.
  - `root` (optional, `unpack: true` only): a folder inside the zip, plain relative segments like `dst`'s
    (`Fusion/red4ext`). Only the entries under `<root>/` are extracted, that prefix stripped, into `dst`; the others
    are not written but still checked against `contents`, which keeps listing every entry with its full path. At least
    one entry must be under it. For upstream zips wrapped in a top folder, installed as released.
  - Within one step, a later file wins over an earlier one on the same path (a kit's runtime layer is listed after the
    mod so the kit's `mapspawn.nut` stays).
- `launch[]`: `{ game, args?, wait?, exe? }`, in start order (a passthrough's Minecraft side first). `args` may use the
  root placeholders; the app returns them resolved to absolute paths (`-file {app}/mod.pk3` ->
  `-file %LOCALAPPDATA%\SIGF\profiles\sigf-doom-x\doom\mod.pk3`) and refuses any other `{placeholder}`. `wait`:
  `port:<n>` (1-65535), the next game starts once `127.0.0.1:<n>` accepts a connection (polled up to 120 s, then Play
  fails with a clear error).
  `exe` (optional): the program in `{game}` to start instead of the store's launch, for games whose mod loads only
  through a script extender (`skse64_loader.exe`, `f4se_loader.exe`; `{game}/` prefix optional, plain relative
  segments, must end in `.exe`). At install the app checks it stays inside the scanned game folder (refused like a
  `dst`) and records it with the folder in `installed.json` (`games[].exe = { path, dir, hint }`, plus `wait`); the
  exe may be missing then. At Play it is resolved again inside that folder (junctions included) and must exist, else
  Play fails with `<exe> not found: install <ID> from <page>`, from the `requires` entry whose `id` starts the exe's
  file name (`skse64` for `skse64_loader.exe`) and its `page`. For a Steam game the app first makes sure Steam runs
  (`steam.exe` in the process list; else `steam://open/main` and up to 20 s for it), then starts the exe with the
  resolved `args` (plus join args) and the game folder as working directory. Joining a lobby launches the same way.
- `server` (optional): the recipe can run on a free hosted server (section 9.5). `{ game: "minecraft", mc,
  loader: "fabric@<x>", ram_gb (1-8, default 2), max_players (2-10), pack, load_on_server? }`. `pack` is the file name
  (`src`) of the recipe's own Minecraft `mrpack` step, `mc` / `loader` must equal the Minecraft game's. The server
  installs that pack server-side: the index files whose `env.server` is not `"unsupported"`, then `overrides/` and
  `server-overrides/` (Modrinth's format; `client-overrides/` stays on the clients). `load_on_server`: Fabric mod ids
  whose `fabric.mod.json` says `"environment": "client"` but that carry a server half; the server sets their
  environment to `*` before it starts (the clients keep the upstream jar unchanged). The catalog refuses a wrong block.

Root placeholders (destinations and launch args):

| Placeholder | Folder | Undo |
|---|---|---|
| `{app}` | this mashup's own folder for that game: `<SIGF_HOME>/profiles/<slug of id>/<game>` (`SIGF_HOME` defaults to `%LOCALAPPDATA%\SIGF`) | deleted on restore |
| `{game}` | the game's install folder from the scan (`gameDirs[<game>]`) | snapshot |
| `{docs}` | the player's Documents folder | snapshot |
| `{fivem}` | the player's FiveM server data folder, the one holding `resources/` (`gameDirs["fivem"]`) | snapshot |

Every write outside `{app}` goes through a snapshot of that folder, whatever the strategy: the originals it overwrites
are copied first, and restore puts them back, deletes the files it added and removes the folders it created (only when
empty). One step writes into at most one of `{game}`, `{docs}`, `{fivem}`. A missing folder fails before any download
with `missingGameDir` (`game` = the game id, or `docs` / `fivem`).

Install strategies, from safest to least safe (what the strategy promises; the placeholders decide where files go):

| Strategy | Used by | Undo |
|---|---|---|
| `args` | Doom `-file {app}/mod.pk3`, Quake / Portal 2 `-game sigf_<slug>` | delete the `{app}` folder |
| `mrpack` | Minecraft: a Modrinth pack (`modrinth.index.json` + `overrides/`), written as a Prism instance `<slug of id>` | delete the instance |
| `profile` | Source `tf/custom` / `garrysmod/addons` folders, BepInEx, Skyrim / Fallout 4 `Data` overlay, GTA V FiveM resource in `{fivem}/resources/sigf_<slug>` | delete the `{app}` folder, restore the snapshot |
| `game-dir-snapshot` | ScriptHookV / ReShade plugins (passthrough hosts), CS 1.6 AMX Mod X, Cyberpunk, Elden Ring, Terraria `{docs}` | restore the snapshot (hashes checked) |

GTA V mods are FiveM resources: `{ "game": "gta5", "strategy": "profile", "runtime": "fivem", "files": [ { "dst":
"{fivem}/resources/sigf_<slug>", "unpack": true, ... } ] }` with `requires: [{ "id": "fivem" }]`; the player adds
`ensure sigf_<slug>` to the server's `server.cfg`. The GTA V side of a passthrough crossover is a ScriptHookV ASI into
`{game}` instead.

### Upstream fusions (community mods hosted on SIGFAI)

A fusion someone else already made can be offered in the app, credited to its authors, when its license allows
redistribution. The publisher takes the upstream release pinned by tag, commit and sha256 of every binary, and builds
two assets: `<id>-<game>.zip` (the script-extender plugin and its files, unpacked into `{game}/Data`,
`game-dir-snapshot`) and `<id>.mrpack` (the upstream Fabric jar and its Modrinth dependencies as downloads, plus the
upstream LICENSE under `overrides/licenses/`). The assets are release assets of `SIGFAI/<id>`, so the download rule
below does not change. The recipe differs in:

```json
"source": { "repo": "https://github.com/<upstream owner>/<name>", "license": "MIT AND GPL-3.0-or-later", "upstream_license": "MIT",
            "tag": "v0.1.2", "commit": "<40 hex>", "hosted": "https://github.com/SIGFAI/<id>",
            "linked": [ { "name": "<library>", "repo": "...", "commit": "...", "license": "..." } ] },
"built_by": { "author": "<upstream author>", "authors": [ "<upstream author>" ], "packaged_by": "SIGF" }, "idea_by": "<upstream author>"
```

Catalog rule: `source.repo` must equal the repo the recipe was read from, **or** `source.hosted` equals that SIGFAI
repo, `source.repo` is another `https://github.com/<owner>/<name>`, `source.commit` is 40 hex and `built_by.author` is a
name. Every `url` is still a release asset of the SIGFAI repo. The catalog card credits `built_by.author` and links the
upstream repo.

**Upstream fetch** (no license to rehost): `source.fetch: "upstream"` plus `source.tag` (the pinned upstream release).
Then, besides the SIGFAI repo's own assets, a download may be a file of exactly that release:
`<source.repo>/releases/download/<source.tag>/<file>` (one path segment), pinned by `sha256` like any asset. The app
downloads the author's file on the player's demand; SIGFAI hosts only the recipe and its own files. Upstream files are
never repacked in this mode: they install as released, with `contents` listing every zip entry.

### Bring your own copy and player builds (BYO-ROM)

Some mashups run on data only the player may have: a cartridge ROM they dumped (Super Mario 64 for libsm64), or a file
built from a game's decompiled code. SIGF ships none of it. Two optional recipe fields move that work to the
player's PC; the SIGF catalog and the app (`src-tauri/src/install/check.rs`) hold them to the same rule, and the card lists them (`ownCopies`, `playerBuild`) so the app says so before Get.

```json
"own_copies": [ {
  "game": "sm64", "label": "Super Mario 64 (USA)", "names": ["super mario 64", "mario 64", "sm64"],
  "rom": { "as": "baserom.us.z64", "sha1": ["9bef1128717f958171a4afac3ed78ee2bb4e86ce"], "extensions": [".z64", ".v64", ".n64"],
           "size": 8388608, "format": "n64" },
  "step": "minecraft", "to": "{instance}/.minecraft/config/mario64" } ],
"player_build": [ {
  "id": "sm64-dll", "label": "Mario's library (sm64.dll)", "step": "minecraft", "minutes": 5,
  "toolchain": ["w64devkit-2.10.0", "python-3.12.10"],
  "script": { "name": "build-sm64-dll.sh", "url": "https://github.com/SIGFAI/<repo>/releases/download/v<version>/build-sm64-dll.sh", "sha256": "...", "size": 2400 },
  "inputs": [
    { "name": "libsm64", "url": "https://github.com/libsm64/libsm64/archive/<40 hex>.zip", "sha256": "...", "size": 616378, "unpack": true, "root": "libsm64-<40 hex>" },
    { "name": "geo.inc.c", "url": "https://raw.githubusercontent.com/n64decomp/sm64/<40 hex>/actors/mario/geo.inc.c", "sha256": "...", "size": 82801 } ],
  "outputs": [ { "name": "sm64.dll", "to": "{instance}/.minecraft/config/mario64" } ] } ]
```

- `own_copies` (1 to 3): a file of a game the player owns. `game` is one of `games[]` (the guest, which the card then
  does not list as a store game to own: `needs` stays the installed games). `label` is what the player is asked for.
  `rom.as` the plain file name it is saved as; `rom.sha1` 1 to 16 accepted SHA-1s (lowercase hex); `rom.extensions` 1
  to 8 (`.z64`); `rom.size` the exact size in bytes (optional); `rom.format: "n64"` (needs `size`, a multiple of 4, at
  most 256 MiB): a byte-swapped `.v64` or little-endian `.n64` dump is normalized to big-endian `.z64` (told apart by
  the header word `80 37 12 40`) before it is hashed and written. `names` (up to 8) are search hints. `step` is an
  install step's game; `to` is `{instance}/...` (that step's Prism instance folder, `mrpack` steps only) or `{app}/...`
  (its own SIGF folder), plain segments after it: never `{game}`, `{docs}` or `{fivem}`.
  - Finding it (`src-tauri/src/install/byo.rs`): when the player clicks Get, the core looks in Downloads, Desktop and Documents
    (OneDrive ones too), `%USERPROFILE%\ROMs`, `C:\ROMs` and similar, 4 levels deep, never into `AppData`, system or
    hidden folders, no symlinks, 15 s at most: files with one of the extensions (and the size), loose or inside a
    `.zip` (up to 1 GiB, entries up to 256 MiB), names matching `names` first. The first one whose SHA-1 matches is used.
    Else the app shows "Uses your own copy of <label>. SIGF never ships or downloads it." and the player picks the file in
    a native dialog the core opens (the webview never names a path). A wrong dump is a clear error (`ownCopyMismatch`:
    "This file is not the <label> dump this mashup needs (SHA-1 ...)"), with the candidates that did not match named.
  - The engine checks every copy before the first download (`ownCopyMissing` / `ownCopyMismatch`), then, once the steps
    are installed, copies it (checked again while copied) to `<to>/<rom.as>` and records it in `installed.json`
    (`placed`). Restore deletes it with the folder it is in. The copy never enters the download cache and is never sent
    anywhere: no code path opens a network connection for it.
- `player_build` (1 or 2): files SIGF must not distribute, built once on the player's PC. There is no command in the
  recipe: `script` is a `.sh` release asset of the mashup's own SIGFAI repo (at most 1 MiB); `inputs` (up to 16, each at
  most 512 MiB) are commit-pinned GitHub sources only, the archive of a commit
  (`https://github.com/<owner>/<repo>/archive/<40 hex>.zip`, redirected to `codeload.github.com`) or one file of it
  (`https://raw.githubusercontent.com/<owner>/<repo>/<40 hex>/<path>`), or release assets of the mashup's own repo;
  every file has its `sha256` and `size`. `unpack: true` extracts a zip input into `$SIGF_IN/<name>/`, `root` keeps only
  that folder of it (GitHub archives wrap everything in `<repo>-<commit>/`). `toolchain` names ids of the app's own
  pinned table (`src-tauri/src/install/tools.rs` `TOOLS`), at least one with a shell:

  | Id | Download (sha256 pinned in the app) | Unpacked to |
  |---|---|---|
  | `w64devkit-2.10.0` | `https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe` (67,127,496 B, GitHub digest `18d0a4c7...`), a self-extracting 7-Zip archive run with `-y -o<dir>` | `<SIGF_HOME>/tools/w64devkit-2.10.0/` (MinGW-w64 GCC, make, BusyBox `sh`, `patch`, `unzip`) |
  | `python-3.12.10` | `https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip` (11,133,606 B, MD5 matches python.org's) | `<SIGF_HOME>/tools/python-3.12.10/` |

  `outputs` (1 to 8): plain file names the script leaves in `$SIGF_OUT`, each copied to `to` (same rule as an own copy's)
  and recorded in `placed`. `minutes` (1 to 60) is shown on the card; the run is stopped after 4x that (10 to 60 min).
  - Running it (`src-tauri/src/install/build.rs`): before anything is installed, the engine fetches the toolchain (once, kept in
    `<SIGF_HOME>/tools/`), the script and the inputs (into `<SIGF_HOME>/build/<slug>/`, never the shared cache), all
    through `FetchOpts::for_build`: build downloads may also start on `raw.githubusercontent.com` and `www.python.org`
    and follow redirects to `codeload.github.com`; mod downloads never can. Then it runs `sh <script>` hidden, with a
    clean environment: `SIGF_IN`, `SIGF_OUT`, `SIGF_WORK` (the working directory), `PATH` (the toolchain, then Windows'
    own folders), `HOME`, `TEMP`, and the Windows basics (`SystemRoot`, `OS`, ...). The build needs no network: the
    script only uses what the app fetched. A failed or missing output stops the install before anything is written
    (`buildFailed`, the log kept as `<SIGF_HOME>/logs/<slug>-<id>-build.log`). The build folder is deleted afterwards,
    sources included.
- Example: `library/mario64-in-minecraft/` (Zckyy's Fabric mod, libsm64 built on the player's PC, the player's SM64 ROM).

**Download rule in the app** (`src-tauri/src/install/check.rs`): the app enforces the catalog's rule itself, so it
does not depend on sigf.ai or its own UI serving honest recipes. Before the first byte of an install, every recipe
download (install files, `requires[].source`, the mrpack `pack`) must be
`https://github.com/SIGFAI/<name>/releases/download/<tag>/<file>` with `<name>` from the id `sigf/<name>` (and equal to
`source.hosted` when given), or, with `source.fetch: "upstream"`, `<source.repo>/releases/download/<source.tag>/<file>`.
Files listed inside an mrpack may also come from `https://cdn.modrinth.com/data/`; a mirror on any other host is
skipped, never contacted. Downloads are https only, follow redirects only to GitHub's release storage
(`objects.githubusercontent.com`, `release-assets.githubusercontent.com`) or Modrinth's CDN, are capped at the recipe's
declared `size` (2 GiB at most) with an idle timeout, and are still checked against their sha256. URLs with `.`/`..`
segments, `%2e`/`%2f`/`%5c`, a user, port, query or fragment are refused. `file://` and local paths only in dev mode
(`SIGF_DEV_LOCAL_RECIPES=1`: the example CLI, the tests, and a debug build); a release build never reads them. The
`install` and `join_lobby` commands re-run the whole rule (`check_recipe`: id, version, games, steps, destinations,
launch, sizes) on the recipe text, and accept a `{game}` folder only when the core's own scan found it.
`cargo run --example install -- validate [--packs] <mashup.json>...` runs the same rule on recipe files (with
`--packs`, on each mrpack's index too). Player builds (section above) add their own downloads: the pinned toolchain
URLs of `install/tools.rs`, the script from the mashup's own releases, inputs that are commit-pinned GitHub sources;
`own_copies` add none.

`source.license` is the license of what is shipped, not only of the upstream code: a script-extender plugin statically
linked with a GPL-3.0 library is a GPL work, so the SIGFAI repo carries the Corresponding Source (a mirror of the
upstream tree at `source.commit` with its submodules), and the plugin's zip has a `SOURCE.txt` naming the trees. When
SIGF rebuilds a binary from the pinned sources, `source.linked[]` gives every library at a 40-hex commit and
`source.rebuilt` the shipped DLL's `sha256` and toolchain.

The app fetches recipes only from `https://sigf.ai/` (the catalog's `recipeUrl` is relative to it:
`/api/app/recipe/<id>@<version>`), and follows a redirect only when it stays on sigf.ai.

## 5. Catalog API (sigf.ai, `/api/app/*`)

- `GET /api/app/catalog?games=gta5,minecraft,skyrim` → cards: id, name, kind, games, cover, clip, built_by, downloads,
  rating.
- `GET /api/app/recipe/:id@:version` → the `mashup.json`.
- `GET /api/app/games` → canonical game ids, store ids per store, art.
- `GET /api/studio/agents` → the SIGF studios shown in the app.

## 9. Multiplayer

Play a mashup with friends: an invite link, or a public lobby anyone with the same games can join. The lobby is a
rendezvous, not a game server: it pins the version, carries where to connect, and counts players. The game traffic
goes between the players' own games (or a hosted world), never through sigf.ai and never through a publisher's online
service (principle 4).

App code: `src-tauri/src/join.rs` (link parsing, join args), `src-tauri/src/hosted.rs` (hosted servers),
`src/lib/lobbies.ts` and `src/views/Lobbies.tsx`.

### 9.1 Lobby

| Field | Meaning |
|---|---|
| `id` | 12 characters of `[a-km-z2-9]` (no `l`, `0`, `1`), 60 random bits. Knowing it = being invited. |
| `mashup` | `{ id, version, name }`: `sigf/<name>@<x.y.z>`, **pinned** at creation (see 9.3). |
| `games` | the games a joiner must own, host first. A public list is filtered by the player's library (`?games=`). |
| `host` | the host player's display name (1-32 letters, digits, spaces, `_ . - '`). No account. |
| `mode` | `invite` (only the link holder sees it) or `public` (listed). |
| `maxPlayers` | 2..100, default 8; a free hosted server caps it at 10. |
| `players` | sent by the host's app with each heartbeat (the host counts as 1). |
| `targets` | per game, where to connect: `{ game, address: "host:port", join: "prism" \| "connect" \| "mod" }`. |
| `server` | `{ provider: "local-host" \| "controller", state: "ready" \| "starting" \| "failed" }` (9.5). |
| `state` | `waiting` (no target yet / server starting), `open`, `full`; closed lobbies answer `410`. |
| `expiresAt` | last heartbeat + 90 s, never more than 24 h after creation. |

The host's app gets a `secret` once, at creation, and signs every heartbeat (every 30 s) and the close with it
(`Authorization: Bearer`). Missing heartbeats for 90 s close the lobby: a crashed host never leaves a ghost lobby.

### 9.2 Links

- `sigf://join/<lobby id>`: the app registers the `sigf` scheme (`tauri-plugin-deep-link`, NSIS writes the registry
  keys). A click while the app runs focuses it and hands it the link (`tauri-plugin-single-instance`, deep-link
  feature); a cold start reads it from the command line. Anything else under `sigf://` is ignored.
- `https://sigf.ai/join/<lobby id>`: what gets pasted in chat apps (custom schemes are not clickable everywhere). The
  page shows the mashup and the host's name, tries `sigf://join/<id>`, and links the installer for players without the
  app. The app accepts this form too (pasted into it).

### 9.3 Version pinning

A lobby can only be created on the catalog's current version of the mashup (`409 unknown_version` otherwise), and the
API keeps that recipe with the lobby (`GET /api/app/lobbies/<id>/recipe`). Joining:
1. reads the lobby (`410` closed -> "this lobby has ended");
2. checks the library owns every game the lobby needs (else: "needs X" with the store link, as on a card);
3. if the installed version of that mashup is not exactly the lobby's: installs the lobby's pinned recipe through the
   normal engine (same verification, snapshot and Restore vanilla; a different version installed is restored first);
4. launches with the join arguments (9.4).
The UI shows each step: `Checking lobby` -> `Installing 62%` -> `Launching` -> `Joined`.

### 9.4 What "join" means per install strategy

| `join` | When | Launch |
|---|---|---|
| `prism` | the game's install step is `mrpack` (Minecraft) | `prismlauncher --launch <instance> --server <host:port>` (Prism's `-l` + `-s`: starts the instance and connects) |
| `connect` | Source / GoldSrc / Quake engines (`tf2`, `gmod`, `portal2`, `cs16`, `css`, `hl2dm`, `l4d2`, `quake`) | the recipe's launch args + `+connect <host:port>` (via `steam://run/<appid>//<args>/`) |
| `mod` | everything else | normal launch; the app writes `{app}/sigf-join.json` = `{ "lobby", "address", "host" }` for the mod's own networking to read (the lobby only carries the address) |

A passthrough starts its Minecraft side first, as for a solo play. The address goes onto a command line, so both the
API and the app accept only `host:port` (DNS name, IPv4 or `[IPv6]`, port 1-65535): never a leading `-`/`+`, a space,
a quote or a placeholder.

### 9.5 Who serves the world

- **`local-host` (the default).** The host's own game is the server. Minecraft: the host starts the world from the app
  (Play), then opens it with `/publish true survival 25565` (or Open to LAN; any port works). The app fills the address
  with the host's LAN address and port 25565; for friends outside the LAN the host pastes a reachable address (a
  forwarded port, or a tunnel). Source games: a listen server on 27015. The app sends the targets with the create and
  every heartbeat.
- **`controller` (free hosted servers, when sigf.ai offers them).** A dedicated Minecraft server run by SIGF for a
  recipe with a `server` block (section 4). Free with caps: a limited number of servers at once (when all are busy the
  API answers `429` and the app shows "All free servers are busy, try again in a few minutes", with the queue place
  when it has one), 8-hour sessions, 10 players, one server per lobby. The app picks the nearest available region. The
  world is kept 7 days after the session; the host downloads it (a zip) with the lobby's secret during and after the
  session, even once the lobby is gone. The host opens a lobby as above, then `POST /api/app/lobbies/<id>/server`.
  Once running, the server's address becomes the lobby's Minecraft target: friends join through the invite link
  exactly as with `local-host`, and the host cannot move it. Stopping the server (or closing the lobby) puts the lobby
  back to `local-host`. The app keeps each hosted lobby's secret in `<SIGF_HOME>/hosted.json` (Rust `hosted_*`
  commands) so "Download world" still works for 7 days, after a restart too.

### 9.6 Privacy and abuse

- The public list carries names and counts only, never an address; invite lobbies are never listed.
- The address goes to whoever holds the lobby id (the invite link, or a click on Join in a public lobby) and is never
  displayed in the app's UI or on the `/join` page.
- The API stores a hash of the host secret and of the creator's IP (3 open lobbies per IP), nothing else about anyone.
- Rate limits: 120 requests/min per IP on `/api/app/*`, 6 lobby creations/min per IP; bodies at most 8 KB.
