# Play on Mac

Mac players run their Windows games from the game page. Background, licenses and choices: docs/MAC-PLAY-RESEARCH.md.
First batch: Skyrim SE and The Witcher 3 (Wine + DXMT), Heroes of Might and Magic III (VCMI, native), Generals Zero
Hour (native GPL port), Age of Empires II DE (native Mac build, launch only). No D3DMetal in this batch.

## 1. Mac recipes (served by sigf.ai as `GET /api/app/mac/games`)

```ts
type MacEngine = { id: string; url: string; sha256: string; size: number; kind: 'wine' | 'renderer' | 'native' };
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
    engine: string;                   // MacEngine id (kind native): VCMI, the Generals port
    data: { from: 'steam-win' | 'gog-installer' | 'folder'; appid?: string; files: string[] };   // what to copy
    launch: string[];                 // command inside the engine bundle
  };
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
- `mac_remove(game, what: 'prefix'|'engine'|'all')`.
- The scan learns Windows games installed inside prefixes (`prefixes/*/drive_c/Program Files (x86)/Steam/steamapps`), so the game page and mods see them; mod installs target that folder.

## 3. UI

The game page on macOS shows "Play on Mac" for games with a Mac recipe: a guided setup (download engine → prepare →
Steam login → install the game → Play), with sizes, honest status tags from `tested`, and Remove. Windows-only games
without a recipe say so.

## 4. Testing

`.github/workflows/app-macplay.yml` on macos arm64 runners: engine download + sha256, prefix creation, Windows Steam
silent install, `wine cmd /c ver`, renderer DLLs in place. Real play is tested by a person on a Mac; results go into
`tested`.
