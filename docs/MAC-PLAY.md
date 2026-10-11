# Play on Mac

Mac players run their Windows games from the game page. Background, licenses and choices: docs/MAC-PLAY-RESEARCH.md.
First batch: Skyrim SE and The Witcher 3 (Wine + DXMT), Heroes of Might and Magic III (VCMI, native), Generals Zero
Hour (native GPL port), Age of Empires II DE (native Mac build, launch only). No D3DMetal in this batch.
Batch 2 (2026-10-11): 17 open-source native engines (MAC-ENGINES.md) for Morrowind (OpenMW), RollerCoaster Tycoon 2 and
Classic (OpenRCT2), Diablo (DevilutionX), Fallout 1 and 2 (Fallout CE), DOOM + DOOM II and Heretic + Hexen (UZDoom), Duke
Nukem 3D, Blood, Shadow Warrior and PowerSlave (Raze), Red Alert and Tiberian Dawn (OpenRA, freeware data), Tomb Raider
I and II (TRX), Caesar III (Julius), FreeSpace 2 (FSO), Gothic I and II (OpenGothic), Theme Hospital (CorsixTH), Descent
3, Locomotion (OpenLoco), Jagged Alliance 2 (Stracciatella), Carmageddon (dethrace); and Wine for Fallout: New Vegas,
Fallout 3, Oblivion (Wine's own d3d), Fallout 4, Kenshi, Bannerlord, Sekiro, Dark Souls Remastered, Dark Souls III and
Monster Hunter: World (DXMT). Nothing tested on a Mac yet (`tested: []`).

## 1. Mac recipes (served by sigf.ai as `GET /api/app/mac/games`)

```ts
type MacEngine = {
  id: string; url: string; sha256: string; size: number;
  kind: 'wine' | 'renderer' | 'native' | 'runtime';
  name?: string; version?: string;    // display only
  needs?: string;                     // wine engines: the runtime (kind runtime) whose libraries Wine loads
};
// A path a port writes: a root, then a plain relative path (no '..', '.', '\', ':', empty or padded segments, no
// leading '/', <= 260 bytes in all). Under {appsupport}/{prefs} at least one segment, and the first one is never
// ai.sigf.app, SIGF or com.apple.*. Existing links on the way must resolve inside the root.
type MacRootedPath =
  | `{appsupport}/${string}`          // ~/Library/Application Support
  | `{prefs}/${string}`               // ~/Library/Preferences
  | `{engine}` | `{engine}/${string}` // the unpacked engine folder (may point inside a .app bundle)
  | `{data}/${string}`;               // the data folder (config paths only)
type MacGame = {
  game: string;                       // canonical id (app/src/data/games.ts)
  name: string;
  route: 'wine' | 'native-port' | 'native';
  // wine: the Windows game runs in its own prefix
  wine?: {
    engine: string;                   // MacEngine id (kind wine)
    renderer: string;                 // MacEngine id (kind renderer), e.g. dxmt
    store: 'steam-win';               // Windows Steam installed in the prefix
    appid: string;                    // Steam app id of the Windows build
    exe: string;                      // relative to the game folder, for a direct launch
    env: Record<string, string>;      // keys [A-Z0-9_]+, values without newlines
    dllOverrides: string;             // WINEDLLOVERRIDES
    args: string[];
    verbs?: string[];                 // 'vcrun2022' | 'mono' (installed on first prefix setup)
    online: 'ok' | 'offline-only' | 'none';
  };
  // native-port: an open-source engine for Mac + the game's data files from the player's copy
  port?: {
    engine: string;                   // MacEngine id (kind native): VCMI, GeneralsX, OpenMW, DevilutionX...
    data: {
      // 'none': the engine needs no player data (OpenRA downloads freeware content itself): always ready, no copy
      // step, and none of the fields below
      from: 'steam-win' | 'gog-installer' | 'folder' | 'none';
      appid?: string;                 // steam-win: the Windows build's Steam app id
      files?: string[];               // 1..64 paths in the game folder, each segment matched ignoring case
                                      // (DIABDAT.MPQ = diabdat.mpq), copied with the folder's own case; ['.'] = all
      optional?: string[];            // <= 64, same rules, copied when present (hellfire.mpq)
      markers?: string[];             // <= 8, under ~/Library/Application Support: ready (the engine imported it)
      into?: MacRootedPath;           // where the data goes: {appsupport}/, {prefs}/ or {engine}[/...];
                                      // default ports/<game>/data. {data} expands to it
    };
    launch: string[];                 // [0]: a file inside the engine folder (OpenMW.app/Contents/MacOS/openmw), the
                                      // rest its arguments
    env?: Record<string, string>;     // same key rules as wine.env
    via?: string;                     // steam-win data without a wine block: the wine engine hosting Windows Steam
    config?: {                        // <= 8 files the engine reads
      path: MacRootedPath;            // a file (not a bare root, no trailing '/'); no {data}/ with from 'none'
      text: string;                   // <= 16 KiB, no NUL
      keep?: boolean;                 // true: written only when missing (player settings kept); else each launch
    }[];
  };
  // launch args, env values and config text expand {data}, {engine}, {appsupport} and {prefs}, and nothing else
  // ({home} does not exist).
  // native: a Mac build exists; the app only detects and launches it
  native?: { steamAppId: string };
  minMacos: string;                   // '26.0'
  notes: string[];
  tested: { by: string; on: string; result: 'works' | 'menu' | 'broken' }[];   // honest status, shown as tags
};
```

Engines are pinned (`url` https on an allowlisted host: github.com release assets, objects.githubusercontent.com,
SIGFAI releases; Valve's `SteamSetup.exe` CDN; the wine-mono GitHub release) and verified by sha256.

## 2. Core (`src-tauri/src/macplay/`, macOS only; the commands answer `unsupported` elsewhere)

Layout under `~/Library/Application Support/ai.sigf.app/macplay/`: `engines/<id>/`, `prefixes/<game>/`,
`ports/<game>/`, `logs/<game>.log`.

Commands (errors `{ code, message }`):
- `mac_status(game)` → `{ route, engine: 'missing'|'ready', prefix: 'missing'|'ready', store: 'missing'|'installed'|'running', game: 'missing'|'installed', data?: 'missing'|'ready', running: boolean, disk: { engineBytes, prefixBytes } }`
- `mac_setup(gameJson)`: downloads the engine + renderer, creates the prefix (`wineboot -u` with a wow64-only engine; no `WINEARCH`, see MAC-ENGINES.md), installs Wine Mono / verbs, installs the renderer DLLs, installs Windows Steam (`SteamSetup.exe /S`). Progress `macplay://progress { game, step, done, total }`.
- `mac_store(game)`: starts Windows Steam in the prefix (`-cef-disable-gpu -no-cef-sandbox`) so the player logs in and installs the game.
- `mac_play(game)`: `steam.exe -applaunch <appid>` (or the exe directly) with the recipe env/overrides; native-port: launch the engine; native: `steam://rungameid/<id>`.
- `mac_stop(game)`: `wineserver -k` for that prefix.
- `mac_copy_data(gameJson)` (steam-win data again from Windows Steam), `mac_pick_data(game)` (folder data, picked in a
  native dialog; refused for `from: 'none'`).
- `mac_remove(game, what: 'prefix'|'engine'|'all')`.

Native-port data and config (`port.rs`):
- `from: 'none'`: `data` reads `ready` and `game` `installed` from the start; nothing is copied or picked.
- Default folder `ports/<game>/data`: filled through `data.part`, `.sigf-data.json` written last inside it, renamed into
  place. A `data.into` folder that is missing or empty is filled the same way through `<folder>.sigf-part`; one that
  already holds other files (an engine's settings folder) gets each file written as `<file>.sigf-part` and renamed
  over its place, the other files left alone. A `data.into` copy records itself in `ports/<game>/.sigf-data.json`:
  the folder and every file it wrote (`copied`). The data is ready when that record names the folder the recipe
  resolves to now and the first copied file is there (a changed `into` or a new engine version means a new copy).
- `config` files are written after each data copy and before each launch (parent folders created; an existing link
  or folder in the file's place is refused).
- `mac_remove` `prefix`/`all`: the files the record lists (each checked again: plain paths inside the folder), the
  folders they leave empty, the `into` folder when empty, the config files replaced each launch under `~/Library`
  (`keep` ones are the player's and stay); then `ports/<game>` as before. `engine` removes an `{engine}` data folder
  with the engine.
- The scan learns Windows games installed inside prefixes (`prefixes/*/drive_c/Program Files (x86)/Steam/steamapps`), so the game page and mods see them; mod installs target that folder.

## 3. UI

The game page on macOS shows "Play on Mac" for games with a Mac recipe: a guided setup (download engine → prepare →
Steam login → install the game → Play), with sizes, honest status tags from `tested`, and Remove. Windows-only games
without a recipe say so.

## 4. Testing

`.github/workflows/app-macplay.yml` on macos arm64 runners: engine download + sha256, prefix creation, Windows Steam
silent install, `wine cmd /c ver`, renderer DLLs in place. Real play is tested by a person on a Mac; results go into
`tested`.
