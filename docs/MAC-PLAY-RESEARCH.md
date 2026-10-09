# Play on Mac: research for SIGF (2026-10-08)

Question: how could SIGF (Tauri, AGPL-3.0, already builds for macOS) let Mac players run their own Windows games, the way
GameToMac does? Status: research only, nothing built. Every claim links to a source. "Bundle inspection" means the
public, login-free GameToMac DMG (`GameToMac-0.1.56-alpha-build96.dmg`, linked from
[gametomac.com](https://www.gametomac.com) via [updates.gametomac.com/download](https://updates.gametomac.com/download)),
listed with `7z l` and `strings`. Nothing was run, bought or signed up for.

## TL;DR

- GameToMac is not a new technology. It is **Wine 11 (CrossOver-derived) + Apple D3DMetal + DXMT + small per-game
  patches**, with Windows Steam / Battle.net / Rockstar Launcher installed inside one Wine prefix per game. It downloads
  D3DMetal at runtime from the Sikarugir project's GitHub release instead of shipping it.
- Every building block except D3DMetal is open source (LGPL/zlib/Apache/MIT) and can be bundled or downloaded by an
  AGPL app. D3DMetal is Apple-proprietary: its license allows redistribution **only for non-commercial purposes**.
  Whisky, Heroic, Mythic and Sikarugir all redistribute or download it anyway.
- The clock is Rosetta 2. Apple keeps general Rosetta through macOS 27; from macOS 28 (fall 2027) only a games
  subset remains. CodeWeavers is already shipping an ARM64 + FEX CrossOver preview, which does not support D3DMetal yet.
- Recommendation: a "Play on Mac" runner in SIGF that downloads signed, pinned engines (Wine, DXMT, DXVK/MoltenVK,
  D3DMetal only as an opt-in download) into Application Support. Each game gets a prefix and a per-game mac recipe in
  the existing recipe format. First batch: **Skyrim SE, The Witcher 3 (DX11), Elden Ring (offline), plus Heroes 3 via
  VCMI and Generals ZH via the GPL source port** (both native, no Wine). AoE II DE is native on Mac since 2026-05-28,
  so it needs no runner.

## 1. What GameToMac uses (bundle inspection)

| Finding | Evidence |
|---|---|
| Product: curated 20-game launcher, Apple Silicon, macOS 26+ (26.5 for D2R/D4/PoE2/Hogwarts), Rosetta required, 7-day trial then "Pro" subscription, "not affiliated with ... CodeWeavers" | [gametomac.com](https://www.gametomac.com) |
| Swift app (`MacOS/GameLibrary`), Sparkle updates, alpha 0.1.56 (build 96, 2026-10-08), first alpha around 2026-09-07; DMG 772 MB, 1.55 GB unpacked, 5,334 files | [appcast.xml](https://updates.gametomac.com/appcast.xml), [DMG](https://updates.gametomac.com/GameToMac-0.1.56-alpha-build96.dmg), [chatgate.ai](https://chatgate.ai/post/gametomac) |
| Bundled engine `Resources/Engine` = Wine 11 (`engine-manifest.json` version `wine11-cache-…`, 2,252 files), with CodeWeavers' MoltenVK fork `libMVK_CW.dylib` and Wine user `crossover`, so a CrossOver-source-based Wine 11 build | DMG; the same dylib set is in the [Sikarugir Template](https://github.com/Sikarugir-App/Wrapper/releases/tag/v1.0) |
| **D3DMetal is not in the DMG.** The binary downloads `https://github.com/Sikarugir-App/Wrapper/releases/download/v1.0/Template-1.0.15.tar.xz` and uses its `renderer/d3dmetal/external/libd3dshared.dylib` (env `CX_APPLEGPTK_LIBD3DSHARED_PATH`). Hogwarts' provenance file says "Pinned Template-1.0.15 D3DMetal dependency". The binary also contains a `d3dmetal-4-0b2` profile, which suggests a GPTK 4 beta test | `strings` on the binary; [Template-1.0.15](https://github.com/Sikarugir-App/Wrapper/releases/download/v1.0/Template-1.0.15.tar.xz) contains `renderer/d3dmetal`, `dxmt`, `dxvk`, `d9vk`, `cnc_ddraw` |
| Ships Apple's GPTK license (`Licenses/Apple-GPTK-License.rtf`, EA18380) next to Wine LGPL, DXMT, cnc-ddraw, SDL2, MinHook, Mixpanel licenses; ships LGPL corresponding source (`Resources/Sources/*`: DXMT patches, Wine d3d patches) | DMG |
| Per-game translation layer (from `games.json`): **D3DMetal** for AoE3, Diablo IV, Elden Ring, Hogwarts, PoE2, Witcher 3, AoM Retold; **DXMT** for CS2, Overwatch, Skyrim; Wine's own d3d11 rebuilt for the Rockstar launcher (RDR2, GTA V Enh., SA DE); **cnc-ddraw** for Heroes 3 and RA2 | DMG `games.json`, `*/rockstar-renderer.json`, `*/runtime-provenance.json` |
| Installs stores inside the prefix: `SteamSetup.exe` (run with `-cef-disable-gpu`, `-no-cef-sandbox`), Battle.net, Rockstar Games Launcher, Epic, VC++ redist, Wine Mono 11.2.0, CnCNet YR package 9.3.3, GeneralsOnline | download URLs in the binary |
| Recipe knobs: `WINEDLLOVERRIDES` (for example `dxgi,d3d11,d3d10core,winemetal=b;nvapi64,nvngx=`), `WINEMSYNC`/`WINEESYNC`, `ROSETTA_ADVERTISE_AVX`, `D3DM_ENABLE_METALFX`, `D3DM_VENDOR_ID`, `DXMT_CONFIG_FILE`, shader/pipeline cache paths, an `x87sidecar` helper (MIT) | `strings` on the binary, `Licenses/Sidecar-MIT.txt` |

Read: GameToMac = Sikarugir/CrossOver-style Wine 11 + D3DMetal fetched from a third-party GitHub release + DXMT,
plus a few hand-made shims per game, sold as a subscription. Selling it sits badly with the GPTK non-commercial clause
(section 5).

## 2. Building blocks: status and license

| Component | What it does | Current status (2026-10) | License | Can AGPL SIGF bundle it / download it at runtime? |
|---|---|---|---|---|
| Wine (WineHQ) | Win32 API on macOS | 11.0 (Jan 2026, WoW64 complete, single loader) ([linuxiac](https://linuxiac.com/wine-11-0-brings-fully-supported-wow64-mode/)); dev 11.18 macOS builds 2026-09-25 ([Gcenx/macOS_Wine_builds](https://github.com/Gcenx/macOS_Wine_builds/releases)) | LGPL-2.1 | Yes, both (ship notices + source offer) |
| CrossOver FOSS source | CodeWeavers' Wine with the Mac game fixes and D3DMetal glue | CrossOver 26.0 (2026-02-10) = Wine 11.0, D3DMetal 3.0, DXMT 0.72, Wine Mono 10.4.1, vkd3d 1.18; 26.3.0 (2026-07-21) latest stable ([changelog](https://www.codeweavers.com/crossover/changelog)); source tarball `crossover-sources-26.3.0.tar.gz` ([source](https://www.codeweavers.com/crossover/source)); CrossOver 27 drops Intel ([CodeWeavers](https://www.codeweavers.com/blog/mjohnson/2026/6/11/whats-in-and-whats-out-for-crossover-27)) | LGPL/GPL/X11 parts | Yes. Build from the source drop (what Sikarugir's `WS12WineCX*` / `WineSikarugir11.0` engines do: [Sikarugir Engines](https://github.com/Sikarugir-App/Engines/releases/tag/v1.0)). The name "CrossOver" is a trademark: don't use it |
| Wine Mono / Gecko | .NET / MSHTML for prefixes | wine-mono 11.2.1 (2026-09-04) ([releases](https://github.com/madewokherd/wine-mono/releases)) | Mono: MIT/LGPL mix; Gecko: MPL ([WineHQ Mono](https://gitlab.winehq.org/wine/wine/-/wikis/Wine-Mono), [Gecko](https://gitlab.winehq.org/wine/wine/-/wikis/Gecko)) | Yes (download the MSI on first prefix) |
| **D3DMetal** (Apple GPTK) | D3D11/D3D12 → Metal, Apple Silicon only | GPTK 3.0 (D3DMetal 3.0) current; Gcenx repack 3.0-3 2026-03-03 ([Gcenx/game-porting-toolkit](https://github.com/Gcenx/game-porting-toolkit/releases)); GPTK 4 beta at WWDC26 (Metal 4, agent skills) ([AppleInsider](https://appleinsider.com/articles/26/06/08/game-porting-toolkit-4-ushers-in-support-for-agentic-coding), [WWDC26 session 357](https://developer-rno.apple.com/videos/play/wwdc2026/357)) | **Proprietary Apple EULA EA18380**: use to "develop, test, or evaluate video games"; "distribute the Apple Software solely for non-commercial purposes"; "the Framework in its entirety ... may be distributed separately"; no reverse engineering, no service bureau (license text in the Sikarugir Template and the GameToMac DMG; Gcenx links the [License.pdf](https://github.com/user-attachments/files/23971305/License.pdf)). A developer asked Apple to clarify redistribution, with no public answer ([Apple forums](https://developer.apple.com/forums/thread/841547)) | **Grey.** Free, non-commercial redistribution is what the text allows. Precedents: Whisky bundled it in WhiskyWine, Heroic downloads Gcenx's GPTK at runtime ([Heroic constants.ts](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/wine/manager/downloader/constants.ts)), Sikarugir ships it in its Template, Mythic built its engine on it. **Do not bundle it in the AGPL app** (proprietary binary). Offer it as an opt-in runtime download with the Apple license shown, never from a paid flow |
| **DXMT** | D3D10/11 → Metal (no Vulkan hop) | v0.80 (2026-04-23), active (pushed 2026-10-06); "Copyright (c) 2023-2026 Feifan He for CodeWeavers"; in CrossOver, Heroic 2.19+ ([alternativeto](https://alternativeto.net/news/2026/1/heroic-games-launcher-2-19-brings-zoom-games-integration-dxmt-support-on-macos-and-more)), Whisky fork | LGPL-2.1 ([3Shain/dxmt](https://github.com/3Shain/dxmt), `COPYING.LIB`) | Yes, both |
| DXVK + MoltenVK | D3D8-11 → Vulkan → Metal | DXVK 3.1.1 upstream needs Vulkan features MoltenVK lacks; the Mac path is the frozen [DXVK-macOS 1.10.3](https://github.com/Gcenx/DXVK-macOS/releases) (zlib); MoltenVK 1.4.2 (2026-07) ([KhronosGroup/MoltenVK](https://github.com/KhronosGroup/MoltenVK/releases)) | zlib / Apache-2.0 | Yes. Use it for D3D8/9 titles; DXMT is better for D3D11 |
| vkd3d-proton | D3D12 → Vulkan | v3.0.1 (2026-05) ([repo](https://github.com/HansKristian-Work/vkd3d-proton/releases)); not a practical Mac path (MoltenVK lacks required features); on Mac, D3D12 means D3DMetal (or CrossOver's own vkd3d for light titles) | LGPL-2.1 | Legally yes, technically no |
| Rosetta 2 | x86_64 → arm64 for Wine itself | General-purpose through macOS 27; from macOS 28 (fall 2027) only "a subset ... for older unmaintained gaming titles" ([MacRumors](https://www.macrumors.com/2026/02/16/macos-tahoe-26-4-rosetta-2-warnings/), [MacTrast](https://www.mactrast.com/2025/06/apple-phasing-out-rosetta-2-with-release-of-macos-28-as-intel-support-sunsets/amp/)); Wine's coverage is unanswered ([Apple forums](https://developer.apple.com/forums/thread/830692)). CodeWeavers: ARM64 CrossOver Preview with FEX (2026-07-31), macOS 26.5+, DXMT yes, **D3DMetal not yet**, targeted for CrossOver 28 ([ithinkdiff](https://www.ithinkdiff.com/?p=343215), [BornCity](https://borncity.com/news/crossover-fuer-mac-arm64-version-ohne-rosetta-2-verfuegbar/), [CodeWeavers PortJump post](https://www.codeweavers.com/blog/orudge/2026/6/19/portjump-update-upcoming-changes-to-macos-support-for-intel-based-applications)) | OS | n/a. **Design for an engine swap to ARM64 Wine + FEX in 2027** |
| Steam (Windows) in Wine | Store client + Steamworks DRM | Works but fragile: CEF black window; Heroic removed its one-click Steam on Mac ([alternativeto](https://alternativeto.net/news/2026/1/heroic-games-launcher-2-19-brings-zoom-games-integration-dxmt-support-on-macos-and-more)) and says it needs Wine-Staging + DXMT ([Heroic wiki](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/wiki/Installing-Steam-for-Windows-on-MacOS)); `-cef-disable-gpu` workaround ([steam-on-m1-wine](https://github.com/notpop/steam-on-m1-wine/)); CrossOver 25.1 fixed downloads ([changelog](https://www.codeweavers.com/crossover/changelog)) | Valve EULA | Download `SteamSetup.exe` from Valve at runtime; never rehost |
| Whisky | SwiftUI Wine wrapper | Original archived 2025-05 ([Whisky-App/Whisky](https://github.com/Whisky-App/Whisky)); active fork frankea/Whisky app-v3.7.0 (2026-08-30, D3DMetal Metal 4 + MetalFX, experimental DXMT) ([fork](https://github.com/frankea/Whisky)) | GPL-3.0 | Code reusable (GPL-3 → AGPL-3 is compatible); good reference for prefix management |
| Heroic | Epic/GOG/Amazon launcher, Mac via Wine/GPTK/CrossOver | v2.22.3 (2026-09-16) ([repo](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher)); downloads WineHQ builds and Gcenx GPTK from GitHub at runtime | GPL-3.0 | Reference implementation of the runtime download model |
| Mythic | Mac launcher on GPTK | v0.6.0 (2025-12-27); engine repo archived and moved to MythicApp/wine ([Mythic](https://github.com/MythicApp/Mythic), [Engine](https://github.com/MythicApp/Engine)) | GPL-3.0 | Reference only |
| Sikarugir (ex-Kegworks/Wineskin) | Wrapper builder + engines + Template with renderers | Active (2026-10-05); Template 1.0.21; engines up to `WS12WineSikarugir11.0_1` ([Wrapper](https://github.com/Sikarugir-App/Wrapper/releases/tag/v1.0), [Engines](https://github.com/Sikarugir-App/Engines/releases/tag/v1.0)) | no SPDX on the repo; Wine parts LGPL; D3DMetal under the Apple EULA | Its engines are usable (GameToMac does this); pin by sha256 |
| Porting Kit | Free closed wrapper store | Closed source ([portingkit.com](https://www.portingkit.com)) | proprietary | No |

## 3. Per-game feasibility (the 20 GameToMac titles)

API = what GameToMac picked (bundle) or the known API. "Odds" = odds for a SIGF runner, judged by blockers.

| Game | Store / launcher | API → Mac layer | Anti-cheat / DRM | Known status | Odds |
|---|---|---|---|---|---|
| GTA V Enhanced | Steam + Rockstar Launcher | D3D12 → D3DMetal | BattlEye: story only, BE off in launcher ([CodeWeavers forum](https://www.codeweavers.com/compatibility/crossover/forum/grand-theft-auto-v?msg=309459), [Steam](https://steamcommunity.com/app/3240220/discussions/0/592890434797191204)) | CrossOver beta, untested ([CW](https://codeweavers.com/compatibility/crossover/grand-theft-auto-v-enhanced)); GameToMac story only | Medium; huge download, launcher breakage |
| RDR2 | Steam + Rockstar Launcher | Vulkan/D3D12 → D3DMetal | none for story | CrossOver 25 support ([9to5Mac](https://9to5mac.com/2025/03/11/crossover-25-red-dead-redemption-2-macos/)) | Medium |
| The Witcher 3 | Steam/GOG | DX11 (classic) or DX12 → DXMT / D3DMetal | none | GameToMac uses D3DMetal + FidelityFX proxy shim | **High** (DX11 mode) |
| Elden Ring | Steam | D3D12 → D3DMetal | EAC → offline only; CrossOver launches with EAC disabled ([CW tips](https://www.codeweavers.com/compatibility/crossover/tips/elden-ring/easy-anti-cheat-issue), [AppleGamingWiki](https://applegamingwiki.com/wiki/Elden_Ring)) | Works offline | **High (offline)** |
| Diablo IV | Steam/Battle.net | D3D12 → D3DMetal | Blizzard user-mode; online-only | CrossOver fixes through 26.3 ([changelog](https://www.codeweavers.com/crossover/changelog)) | Medium; game updates break it |
| Hogwarts Legacy | Steam | D3D12 → D3DMetal | Denuvo ([Steam](https://store.steampowered.com/app/990080)) | GameToMac requires macOS 26.5 | Medium |
| Counter-Strike 2 | Steam | DX11 → DXMT | VAC (user-mode); Mac-under-Wine VAC status unconfirmed | CrossOver 23.6 support ([CW blog](https://www.codeweavers.com/blog/mjohnson/2023/10/18/crossover-236-another-strike-against-platform-limitations)); no native Mac since 2023 ([csdb.gg](https://csdb.gg/guides/cs2-on-mac/)) | Medium; ban risk on us |
| Age of Empires IV | Steam | DX11/12 → Wine/D3DMetal | online OK per GameToMac FAQ | CrossOver 26.0 fix | Medium-high |
| Skyrim SE | Steam | DX11 → DXMT | none | CrossOver support since 2021; GameToMac `skyrim-dxmt` | **High**, and the strongest SIGF fit (mods) |
| Diablo II: Resurrected | Battle.net only | D3D12 → D3DMetal | Blizzard; online | CrossOver DX12 support since 23.0 | Medium |
| Path of Exile 2 | Steam/standalone | DX12/Vulkan/DX11 → D3DMetal | server-side | CrossOver 25 fixes; GGG lists a Mac version for 1.0 on 2026-12-11 ([rpgamer](https://rpgamer.com/2026/08/path-of-exile-2-fully-releasing-in-december/)) | Prefer native when it ships |
| Overwatch | Steam/Battle.net | DX11 → DXMT | Blizzard anti-cheat, online-only, unreliable ([evetech](https://evezone.evetech.co.za/quick-bytes/can-you-play-overwatch-2-on-mac-in-2026)) | GameToMac alpha, stutter | Low-medium |
| AoM Retold | Steam | D3D12 → D3DMetal | — | CrossOver 25 fix | Medium |
| Company of Heroes 3 | Steam | DX11/12 → Wine (UCRT fix) | online OK per GameToMac | CrossOver 26.0 fix | Medium |
| GTA SA DE | Steam + Rockstar Launcher | UE4 DX11 | — | GameToMac custom | Medium |
| AoE II DE | Steam | **Native Mac** (Feral, 2026-05-28, free for owners, no cross-play) ([ageofempires.com](https://www.ageofempires.com/news/age-of-empires-ii-definitive-edition-available-now-on-mac/), [Feral](https://www.feralinteractive.com/en/news/real-time-royalty-age-of-empire-ii-definitive-edition-is-out-now-on-macos-/)) | — | Steam lists `mac: true` | **Native**: launch the Mac build, no Wine |
| AoE III DE | Steam | DX11 → D3DMetal | — | CrossOver 24.0 fix | Medium-high |
| Heroes 3 | Steam/GOG | DirectDraw → cnc-ddraw | — | **VCMI native arm64**, needs the player's data ([vcmi.eu](https://vcmi.eu/players/Installation_macOS/), [brew](https://formulae.brew.sh/cask/vcmi)) | **High (VCMI, GPL)** |
| Red Alert 2 (CnCNet) | Steam/EA | DDraw → cnc-ddraw | — | CnCNet client needs .NET; fragile on Wine ([CnCNet forum](https://forums.cncnet.org/topic/7240-playing-yuris-revenge-online-on-mac-os-x-wineskin-or-vm/)) | Medium |
| Generals ZH (GeneralsOnline) | Steam/EA | DX8 → DXVK/MoltenVK | — | EA released the source under GPL-3; native arm64 port via GeneralsX ([Gigazine](https://gigazine.net/gsc_news/en/20260706-command-and-conquer-zero-hour-apple/), [korben](https://korben.info/en/command-conquer-generals-ios-macos-ai-port.html)) | **High (native port)** |

Anti-cheat rule: kernel EAC/BattlEye online, Vanguard and Ricochet don't run under Wine on Mac. It is the publisher's
switch, not a fixable bug ([CodeWeavers blog index, 2026-08-31 post](https://www.codeweavers.com/about/blogs),
[macgamingdb](https://macgamingdb.app/blog/how-to-play-windows-games-on-mac)). This matches SIGF principle 4 (never
into official online modes, [RECIPE-FORMAT.md](./RECIPE-FORMAT.md)).

D3D12 on Mac today is effectively **only D3DMetal** (CrossOver's vkd3d covers light titles). DXMT covers D3D10/11 and is
the open, ARM64-ready path.

## 4. Recommended architecture for SIGF

```
SIGF.app (AGPL, small)            ~/Library/Application Support/ai.sigf.app/macplay/
  macplay runner (Rust)   ─────▶   engines/wine-11.x-<sha>/          (download, pinned sha256)
  recipe: platforms.mac            renderers/dxmt-0.80/  dxvk-macos-1.10.3/  cnc-ddraw/
                                   renderers/d3dmetal-3.0/           (opt-in, Apple EULA screen)
                                   prefixes/<game-id>/  (WINEPREFIX, one per game)
                                   caches/<game-id>/    (DXMT/Metal shader caches)
```

1. **Engines are downloaded, not bundled.** Keep the AGPL app small and free of proprietary binaries. Use the existing
   SIGF download rule: allowlisted GitHub release URL + sha256 in the recipe. Sources: Wine built by us from WineHQ or
   the CrossOver source drop, published as a SIGFAI release (LGPL: publish the source too). DXMT (LGPL) and DXVK-macOS
   (zlib) can be rehosted. D3DMetal: download from the upstream Gcenx GPTK or Sikarugir release URL only after the
   player accepts Apple's license in-app; never rehost on SIGFAI, never tie it to anything paid.
2. **One prefix per game** (`WINEPREFIX=…/prefixes/<id>`, `WINEARCH=wow64`), so a broken game never touches another.
   Create it with `wineboot -u`, install Wine Mono from its GitHub MSI, and add `vcredist` from Microsoft's URL when
   the recipe asks. This is GameToMac's model, seen in its bundle.
3. **Store inside the prefix.** Download `SteamSetup.exe` from Valve's CDN and run Steam with `-cef-disable-gpu
   -no-cef-sandbox` (GameToMac's flags). The player logs in and installs the Windows build there. Battle.net and the
   Rockstar Launcher come from their official installer URLs. Existing files: let the player point at a folder
   (external SSD or a copy from a PC). Steam adopts it as a library folder and verifies it. The SIGF scan learns
   `prefixes/*/drive_c/Program Files (x86)/Steam/steamapps`.
4. **Per-game mac recipe**: a `platforms.mac` block in `mashup.json` / the game list:
   ```json
   "mac": {
     "engine": "wine-11.0-cx26@sha256:…", "renderer": "dxmt-0.80" ,
     "store": "steam-win", "appid": 489830, "exe": "SkyrimSE.exe",
     "env": { "WINEMSYNC": "1", "ROSETTA_ADVERTISE_AVX": "1", "DXMT_CONFIG_FILE": "<recipe>/dxmt.conf" },
     "dll_overrides": "dxgi,d3d11,d3d10core,winemetal=b;nvapi64,nvngx=",
     "args": [], "verbs": ["vcrun2022"], "min_macos": "26.0", "online": "offline-only"
   }
   ```
   Mods then install into the prefix exactly as on Windows, so SKSE, REDmod and similar loaders run unchanged. This is
   SIGF's edge over GameToMac.
5. **Launch**: `wine start /unix <exe>` or `steam.exe -applaunch <id>` inside the prefix. Supervise `wineserver -w`.
   "Stop" runs `wineserver -k`. Logs go to the existing report flow.
6. **Signing / notarization**: the Mac build is ad-hoc signed today (`"signingIdentity": "-"` in
   `src-tauri/tauri.conf.json`), so users hit Gatekeeper's "Open Anyway". For a real launch, get a Developer ID
   (USD 99/yr) and notarize SIGF.app itself. Engines fetched by the app over HTTPS don't get the quarantine attribute,
   so Gatekeeper doesn't assess them. They still need valid (ad-hoc is fine) signatures and the Wine entitlements;
   GameToMac ships a `wineserver-entitlements.plist` (bundle).
7. **Disk**: compressed sizes are Wine 11.18 ≈ 191 MB ([release assets](https://github.com/Gcenx/macOS_Wine_builds/releases)),
   GPTK 3.0-3 ≈ 239 MB ([assets](https://github.com/Gcenx/game-porting-toolkit/releases)), Sikarugir Template ≈ 87 MB
   (measured); GameToMac totals 1.55 GB installed (measured). Add the prefix plus Windows Steam (est. 1–2 GB) and the
   full Windows game. Mac Steam's library can't be reused (different depots), so the game is downloaded again. Offer
   external-drive prefixes.
8. **Rosetta exit (2027)**: keep `engine` a recipe field so a later `wine-arm64-fex` engine replaces the x86_64 one
   without touching recipes. Prefer DXMT where it works (already ARM64 in the CrossOver preview), and treat D3DMetal
   titles as "at risk after macOS 27" until Apple/CodeWeavers ship ARM64 D3DMetal.

**First batch (best odds, no online anti-cheat, fits a mod app):**

| # | Game | Path | Why |
|---|---|---|---|
| 1 | Skyrim SE | Wine + DXMT | DX11, no DRM/AC, huge mod scene, proven in CrossOver and GameToMac |
| 2 | The Witcher 3 (DX11 mode) | Wine + DXMT (D3DMetal optional) | No AC, mods (REDmod) |
| 3 | Heroes 3 | **VCMI native** + player's data | GPL, arm64, no Wine at all |
| 4 | Generals Zero Hour | **GeneralsX-family native port** (GPL-3 EA source) | No Wine; GeneralsOnline later |
| 5 | Elden Ring (offline) | Wine + D3DMetal (opt-in) | Proves the D3D12 path; EAC off → offline mods only |

Plus AoE II DE: just detect and launch the native Mac build.

## 5. Risks

| Risk | Detail | Mitigation |
|---|---|---|
| Apple GPTK EULA | Licensed for evaluating/testing games; redistribution "solely for non-commercial purposes"; no service bureau (EULA EA18380, see section 2). SIGF has a commercial side (stream/token), which could be argued as commercial | Never bundle or rehost D3DMetal; player-initiated download from upstream with the license shown; keep the D3DMetal path optional and DXMT-first; never in a paid tier |
| Game EULAs / anti-cheat | Online play under Wine can trigger bans (example: NetEase banned Mac/Deck players until 2124, later reversed ([Slashdot/Ars](https://games.slashdot.org/story/25/01/03/1929250/marvel-game-developer-reverses-century-long-bans-on-linux-mac-users))) | Recipes declare `online: offline-only`; no CS2/Overwatch/D4/D2R in the first batch |
| Steam SSA | Running the Windows client under a compatibility layer is normal (Valve ships [Proton](https://github.com/ValveSoftware/Proton) itself); SIGF only downloads Valve's official installer, never Steam files | Use official URLs only |
| Trademarks | GameToMac's footer disclaims affiliation with CodeWeavers, Apple, etc. ([site](https://www.gametomac.com)) | No "CrossOver"/"GPTK" branding in SIGF UI beyond attribution |
| Support burden | Store/game updates break recipes ([CodeWeavers, 2026-08-24](https://www.codeweavers.com/about/blogs)); Steam-in-Wine is the weakest link ([Heroic](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/wiki/Installing-Steam-for-Windows-on-MacOS)) | Pin `validatedSHA256`/buildid per recipe (GameToMac does this in `games.json`), mark "verified for build X", use the report flow |
| Performance | Translation overhead; GPTK 4 beta shows gains (GTA V +66% on M4 Pro) ([Macworld](https://www.macworld.com/article/3189951/apples-latest-game-porting-toolkit-beta-changed-how-i-think-about-mac-gaming.html)); M1 RDR2 25–50 fps on low ([9to5Mac](https://9to5mac.com/2025/03/11/crossover-25-red-dead-redemption-2-macos/)) | Per-recipe minimum chip; shader-cache prewarm (GameToMac's `cs2-prewarm`) |
| Rosetta end | macOS 28 (fall 2027) may break x86_64 Wine | Engine-pluggable design; follow CodeWeavers ARM64/FEX |
| Licenses we owe | LGPL Wine/DXMT: ship notices + corresponding source (GameToMac does: `Resources/Sources`) | Publish engine sources next to SIGFAI engine releases; extend `THIRD_PARTY_NOTICES.md` |
