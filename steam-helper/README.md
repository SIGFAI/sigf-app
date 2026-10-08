# sigf-steam

The SIGF app's Steam helper (docs/WORKSHOP.md section 3): subscribes to, unsubscribes from and reads the state of Steam
Workshop items through the player's running Steam client, as the game's app id. The app starts it once per call
(`sigf-steam <subscribe|unsubscribe|state> <appid> [ids...]`) and reads one JSON object per stdout line.

It is its own crate, outside the app's Cargo build, so `SIGF.exe` never links `steam_api64.dll`: only this helper loads
it, for the few seconds a call lasts.

## Build

```
node app/steam-helper/build.mjs
```

builds `target/release/sigf-steam(.exe)` and copies the Steamworks SDK redistributable (`steam_api64.dll` on Windows,
shipped inside the [`steamworks-sys`](https://crates.io/crates/steamworks-sys) crate and copied to its build output)
next to it. Plain `cargo build --release` builds the exe but leaves the library in
`target/release/build/steamworks-sys-*/out/`.

## Bundling (Windows)

`app/src-tauri/tauri.windows.conf.json` (merged into tauri.conf.json on Windows only):

- `bundle.resources` puts `sigf-steam.exe` and `steam_api64.dll` at the install root, next to `SIGF.exe`;
- `build.beforeBuildCommand` runs `build.mjs` before the UI build, so `npm run tauri build` builds the helper first.

Order on Windows: helper first, then the app. `tauri build` does that by itself; for `tauri dev` or a plain
`cargo build`/`cargo check` of `src-tauri` on Windows, run `build.mjs` once first, since tauri-build refuses a missing
resource. A dev build also finds the helper in `app/steam-helper/target/release` (then `debug`, which has no DLL next to it unless copied).

macOS and Linux builds do not ship the helper (only the Windows bundle has it); the app answers
`helper_missing` there.

## License

AGPL-3.0-only, like the app. `steam_api64.dll` is Valve's, under the Steamworks SDK Access Agreement (see
`app/THIRD_PARTY_NOTICES.md`).
