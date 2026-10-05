# SIGF

SIGF is a Windows desktop app that installs game mashups in one click: community mods and SIGF's own mashups that join
two games together. It finds the games you already own, installs the files a mashup needs into the right folders, and
can put every game back the way it was with **Restore vanilla**.

SIGF is open source (AGPL-3.0).

- Download: [Releases](https://github.com/SIGFAI/sigf-app/releases)
- Website: [sigf.ai](https://sigf.ai)
- Report a security problem: [SECURITY.md](SECURITY.md)
- Name and logo: [TRADEMARKS.md](TRADEMARKS.md)
- Code signing: [Code signing policy](CODE-SIGNING-POLICY.md)
- Privacy: [what the app sends, and to whom](docs/PRIVACY.md)

SIGF is not affiliated with any game publisher or store. Game names and art belong to their owners.

## What the app does and doesn't do

**It does:**

- Find your installed games by reading only these files and keys:
  - Steam: `libraryfolders.vdf`, `appmanifest_*.acf`, `appinfo.vdf` and Steam's own art cache;
  - Epic Games: the install manifests and the catalog cache;
  - GOG and Ubisoft: their install registry keys;
  - Prism Launcher, the Modrinth App and `.minecraft`: folder names.
- Check every file it downloads against a SHA-256 hash (SHA-512 or SHA-1 for Minecraft pack entries from Modrinth)
  before it uses it. A mismatch stops the install.
- Download mod files only from the GitHub release of the mashup's own repository (or the one pinned upstream release a
  mashup is built from), and Minecraft pack files only from Modrinth's CDN, over HTTPS.
- Save a copy of every game file before changing it. **Restore vanilla** puts every original back byte for byte,
  deletes the files the mashup added, and stops to ask you if the game changed those files since.
- Keep every install inside its target folder: no `..`, no absolute paths, no escape through junctions or symlinks.
- Install only into the game folders its own scan found.
- Ask before acting on an invite link: it shows the mashup and version, the games it will change, the server address
  and the host's name, and does nothing until you click Join.
- Start games only through `steam://`, Epic, Ubisoft and GOG Galaxy links, Prism Launcher, or a loader `.exe` that sits
  inside the game's own folder.

**It doesn't:**

- collect telemetry, analytics or crash reports, show ads, or check for updates;
- need an account;
- ask for administrator rights, or install any service, driver, scheduled task, startup entry, firewall rule or
  antivirus exclusion;
- run in the background: closing the window ends it;
- read launcher logins, tokens or Minecraft accounts (Minecraft sign-in stays in Prism Launcher);
- give its interface direct file, shell or network access: only a fixed set of commands in the Rust core can act;
- open links other than `https://` pages in your browser.
- ship any game art.

**Limits you should know:**

- **Mods are programs.** Mashups contain native code (DLL and ASI plugins, BepInEx, Fabric mods) that runs inside the
  game with your rights. SIGF checks that every file is exactly what was published. It does not prove that a mod is
  harmless. See [how the catalog is checked](https://sigf.ai/trust).
- **Restore cannot undo everything:** files the game or the mod creates while you play (configs, logs, saves), a
  Restore you force after the game was updated, or a Minecraft instance created through Prism's import fallback.
- **Uninstalling SIGF does not restore your games.** Use Restore vanilla on each installed mashup first.

## Privacy: exactly what is sent

SIGF has no account, no ads, no telemetry, no analytics and no crash reports. It never reads your store logins,
passwords or tokens, and it has no machine id or install id. Every request you did not ask for directly can be turned
off, and the app asks you about them before it makes any of them. The full policy, the source of truth for this
section, is [docs/PRIVACY.md](docs/PRIVACY.md) (also at [sigf.ai/privacy](https://sigf.ai/privacy)).

| When | To | What | Can you turn it off? |
|---|---|---|---|
| App start | `sigf.ai` | A request for the mashup catalog. No identifier, no cookie. It is the only request made before you answer the privacy screen. | No |
| App start, after the privacy screen | `sigf.ai` | The list of mashups being built and the free hosted server regions. No identifier. | No |
| Pictures on screen | Steam, Epic Games and Modrinth image servers | Image requests for game, mashup and creator pictures, so these servers can tell which games are on your screen. | **Yes**: "Game pictures" (off: plain colored tiles; pictures already in Steam's own cache on your PC still show, read from disk) |
| A game has no picture (some Ubisoft, GOG and Epic games) | Steam store search | **The game's name**, to find its picture. The answer is cached on your PC. | **Yes**: "Find missing pictures on Steam" (also off when "Game pictures" is off) |
| Lobbies tab or a mashup's "Play with friends" panel open, every 10 seconds | `sigf.ai` | The ids of the games you own, among the games SIGF has mashups for, so you see only lobbies you can join. | **Yes**: "Lobbies for the games I own" (off: the app gets every public lobby and picks yours on your PC) |
| Installing a mashup or joining a lobby | `sigf.ai`, then GitHub release servers and Modrinth's CDN | The mashup's recipe, then plain file downloads. Like any download, these servers see your IP address. | You asked for the install |
| Opening an invite link | `sigf.ai` | The lobby id, to show you the lobby before you confirm the join. | You asked to join |
| Hosting a lobby | `sigf.ai` | Your display name, the lobby mode, the player limit, the join address, and the player count (every 30 seconds). **Your PC's local network (LAN) address goes into the join address only when you click "Use my LAN address"**, or by itself if you chose "Fill in for me" (the default is "Ask each time"). Anyone with the invite link can see the address; a public lobby list never shows it. sigf.ai keeps a salted hash of your IP address with the lobby to limit abuse. | Do not host |
| "Host on SIGF (free)", if you choose it (when sigf.ai offers it) | `sigf.ai` | The lobby and the region you pick. Your world runs on SIGF's server for up to 8 hours and is kept for 7 days so you can download it. | Do not use it |
| Hosting a Minecraft lobby | Your own Minecraft server | A status check (Minecraft Server List Ping) every 30 seconds to count players. | Do not host |
| First install on a PC without WebView2 | Microsoft | The installer downloads Microsoft's WebView2 runtime. | No |

**Your choices.** The installer's first page shows the privacy text with a link to the full policy, and at the end of
the installation it asks "Allow SIGF's optional online requests?" (No turns them all off). On its first start, before
anything but the catalog request goes out, the app shows each choice: "Game pictures", "Find missing pictures on
Steam", "Lobbies for the games I own" and "My local network address when I host" (Ask each time or Fill in for me),
with the installer's answer filled in; nothing else happens until you click Continue. You can change them any time under **Privacy**, at the bottom of the app's left
bar. The choices are saved in `%LOCALAPPDATA%\SIGF\privacy.json`, and the app's Rust core enforces them for its own
requests.

**What sigf.ai keeps.** For a lobby: the mashup and version, your display name, the lobby settings, the join address, a
hash of the host's secret and an HMAC-SHA256 of your IP address under a secret server-side salt, used only to limit how
many lobbies and free servers one address runs at once. A lobby closes 90 seconds after its last heartbeat (at most 24
hours after it opened) and is deleted a day after it closes. For a free hosted server: the lobby id, the mashup, the
region, the server's state and times, the same IP hash, and your world, deleted 7 days after the session. Rate-limit counts per IP address live in memory for a
minute and are not stored. SIGF keeps no record of catalog, recipe or image requests; sigf.ai runs behind Cloudflare,
which carries every request to the site and keeps its own logs under Cloudflare's privacy policy.

**What stays on your PC:** the list of your games and their folders, downloads, backups of original game files, the
list of installed mashups, the picture search cache, your privacy choices, the secrets of your hosted lobbies' worlds
(for 7 days) and your host display name, in
`%LOCALAPPDATA%\SIGF` and the app's WebView2 profile. The program itself is installed in
`%LOCALAPPDATA%\Programs\SIGF`; uninstalling it keeps `%LOCALAPPDATA%\SIGF`.

**Privacy requests**, or anything about your own data: open a private report through
[GitHub Security Advisories](https://github.com/SIGFAI/sigf-app/security/advisories/new). Only the maintainers can read
it. General questions that are not sensitive: [issues](https://github.com/SIGFAI/sigf-app/issues).

## Verify a release

Every release is built by GitHub Actions from a tagged commit of this repository. Each release has a `SHA256SUMS` file
and a signed build provenance attestation.

```powershell
# 1. The file was built by this repository's release workflow
gh attestation verify SIGF_0.1.0_x64-setup.exe --repo SIGFAI/sigf-app

# 2. The hash matches SHA256SUMS
Get-FileHash -Algorithm SHA256 .\SIGF_0.1.0_x64-setup.exe
```

Replace `0.1.0` with the version you downloaded. Step 1 needs the [GitHub CLI](https://cli.github.com/). It proves where
and how the file was built, not that the code is bug-free.

Releases are not code-signed yet: Windows SmartScreen may warn before the first run. Signing through SignPath
Foundation is planned; see the [code signing policy](CODE-SIGNING-POLICY.md).

## Build from source

Windows 10 or 11, x64.

Prerequisites:

- [Rust](https://rustup.rs/) with the MSVC toolchain (`x86_64-pc-windows-msvc`), version in `rust-toolchain.toml`
- [Visual Studio 2022 Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with **Desktop development
  with C++** (MSVC and the Windows SDK)
- [Node.js](https://nodejs.org/) 22 with npm
- Microsoft Edge WebView2 Runtime (included in Windows 11)

The NSIS tools the installer needs are downloaded by the Tauri CLI on the first build.

```powershell
git clone https://github.com/SIGFAI/sigf-app
cd sigf-app
npm ci

# Run in development (UI with hot reload + Rust core)
npm run tauri dev

# Tests (the Rust build expects dist/ to exist, so build the UI first)
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml

# Release build: app exe and NSIS installer
npm run tauri build
```

The installer is written to `src-tauri/target/release/bundle/nsis/SIGF_<version>_x64-setup.exe` and the app to
`src-tauri/target/release/sigf-app.exe`.

The app is built with [Tauri 2](https://tauri.app/) (Rust core, React + TypeScript UI built with Vite).

| Path | What |
|---|---|
| `src/` | The UI (React) |
| `src-tauri/src/scan/` | Finds installed games (read-only) |
| `src-tauri/src/install/` | The recipe engine: download, hash check, install, snapshot, Restore |
| `src-tauri/src/join.rs`, `launch.rs` | Invite links, lobbies and game launch |
| `src-tauri/capabilities/` | What the UI is allowed to call |
| `docs/RECIPE-FORMAT.md` | The `mashup.json` recipe format, the catalog API and multiplayer lobbies |
| `docs/PRIVACY.md` | What the app sends, and to whom |
| `src-tauri/tests/` | Engine tests; `make-fixtures.mjs` writes the synthetic test fixtures |

## Contributing

Issues and pull requests are welcome. By contributing, you agree that your contribution is licensed under the
AGPL-3.0. Every change is reviewed by a maintainer before it is merged.

## License

Copyright (C) 2026 SIGF contributors.

This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General
Public License version 3 only (`AGPL-3.0-only`), as published by the Free Software Foundation. See [LICENSE](LICENSE).

Bundled fonts (IBM Plex Sans, IBM Plex Mono, Michroma) are under the SIL Open Font License 1.1; third-party notices are
in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The SIGF name and logo are covered by [TRADEMARKS.md](TRADEMARKS.md).
