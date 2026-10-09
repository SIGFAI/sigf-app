# Game hub: one page per game, every mod source

The game page is where a player does everything for one game: mods from every source, mashups, servers, libraries
(mod packs that mix sources), and Play. It grows out of the Workshop page (docs/WORKSHOP.md), which stays the Steam
Workshop part of it.

## 1. Sources

| Prefix | Source | API (server side) | Key | Installs through |
|---|---|---|---|---|
| `ws` | Steam Workshop | existing `/api/app/workshop/*` | `STEAM_WEB_API_KEY` (set) | `sigf-steam.exe` (docs/WORKSHOP.md) |
| `ts` | Thunderstore | `https://thunderstore.io/c/<community>/api/v1/package/` (+ v2 package index), experimental package API | none | engine plan |
| `cf` | CurseForge | `https://api.curseforge.com/v1/*` | `CURSEFORGE_API_KEY` (pending) | engine plan; `allowModDistribution: false` → link out only |
| `nx` | Nexus Mods | search: public GraphQL `https://api.nexusmods.com/v2/graphql`; files/downloads: v1 REST with the player's own key from SSO | app slug (pending) | engine plan after an `nxm://` click (free users) or a direct link (Premium) |
| `mio` | mod.io | `https://api.mod.io/v1/*` (`api_key=`) | `MODIO_API_KEY` (pending) | engine plan |
| `gb` | GameBanana | `https://gamebanana.com/apiv11/*` | none | engine plan |
| `mr` | Modrinth | `https://api.modrinth.com/v2/*` | none | engine plan into a Minecraft profile (section 4) |

A source with no key answers `503 { error: "source_unavailable", source }` and the UI shows it as "coming soon".

## 2. Games map (served by sigf.ai as `GET /api/app/mods/games`)

```ts
type ModGame = {
  game: string;                 // canonical id, as app/src/data/games.ts (`lethalcompany`, `skyrimse`, ...)
  name: string;
  steam?: string;               // Steam app id
  sources: {
    ts?: { community: string };                 // thunderstore community slug
    cf?: { gameId: number; classId?: number };  // curseforge game id (+ the "mods" class)
    nx?: { domain: string };                     // nexus game domain (skyrimspecialedition)
    mio?: { gameId: number };
    gb?: { gameId: number };
    mr?: {};                                     // minecraft only
  };
  targets: Target[];            // where files go (section 4), first match wins
};
```

Covers at least the 60 most-modded PC games (every game present on 2 or more sources, plus every game in the SIGF
catalog). Each entry cites where its install rule comes from (the source's own install docs, r2modman/Thunderstore
ecosystem schema, Vortex game extension, the game's own docs) in a comment. Nexus / GameBanana / CurseForge targets
follow Vortex's game extensions (Nexus-Mods/vortex-games, Vortex's per-game `stopPatterns`, the Nexus-Mods/game-*
repositories, ChemBoy1's extensions), pinned to a commit in the file header: the folders and file tests they use,
turned into layout rules (section 4). Games whose mods need a tool to activate or merge them (a mod manager, a load
order file, a deploy step) or live outside the game folder keep no targets.

## 3. Proxy API (sigf.ai, JSON, all GET)

- `GET /api/app/mods/games` → `ModGame[]` (without the comments).
- `GET /api/app/mods/search?game=<canon>&source=<prefix>&q=&sort=popular|updated|new&cursor=` → `{ items: ModItem[], next, total? }`. One source per call; the app calls the sources in parallel and merges.
- `GET /api/app/mods/item?ref=<ref>` → `ModItem` with `description` (≤ 2000 chars plain text), `files?: ModFile[]`.
- `GET /api/app/mods/plan?ref=<ref>&game=<canon>[&file=<fileId>][&mc=<version>&loader=<loader>]` → `InstallPlan`
  (section 4), dependencies resolved. `mc` + `loader`: the Minecraft profile (section 4), also taken by `/search`.
- `GET /api/app/mods/mc-versions` → `{ latest, versions: { id, loaders }[] }`: Minecraft releases newest first (Modrinth's
  `/v2/tag/game_version`), each with the loaders Prism's metadata has for it (cached 10 min, 6 h upstream).

Refs: `ts:<namespace>-<name>`, `cf:<projectId>`, `nx:<domain>:<modId>`, `mio:<gameId>:<modId>`, `gb:<itemtype>:<id>`,
`mr:<projectId>`, `ws:<publishedFileId>`. Charset `[A-Za-z0-9_.:-]`, ≤ 120.

```ts
type ModItem = {
  ref: string; source: 'ts'|'cf'|'nx'|'mio'|'gb'|'mr'|'ws';
  game: string; title: string; summary: string; author?: string;
  icon?: string;               // https, the source's CDN
  downloads?: number; likes?: number; updated?: number; created?: number;  // unix s
  version?: string; sizeBytes?: number; tags: string[];
  url: string;                 // the item's page on its source
  nsfw?: boolean;              // hidden unless the player opts in
  installable: boolean;        // false: link out only (CurseForge distribution off, Nexus without a key, unknown target)
  why?: string;                // when not installable: 'distribution_off' | 'needs_nexus_login' | 'no_target' | ...
};
```

Caching like the Workshop proxy (search 10 min, items 30 min), per-IP rate limits, upstream timeouts 8 s.

## 4. Install plans

The server turns a ref into a plan; the core checks and installs it with the engine's snapshot/restore
(`src-tauri/src/install/`), registry id `mod/<ref>`.

```ts
type InstallPlan = {
  ref: string; game: string; name: string; version: string;
  files: PlanFile[];           // in order; dependencies first
  deps: { ref: string; name: string; version: string }[];   // what the files include besides the item
  link?: { url: string; why: string };   // instead of files when the mod can't be installed by the app
  needs?: 'nxm';               // Nexus free user: the app waits for the nxm:// click from the mod's page
};
type PlanFile = {
  url: string;                 // https, a host on the source's allowlist (section 5)
  size?: number;
  hash?: { sha256?: string; sha512?: string; sha1?: string; md5?: string };   // every hash the source gives
  name: string;                // file name
  unpack: boolean;
  dst: string;                 // '{game}/...' only; never '..', never absolute
  root?: string;               // strip this folder inside the archive
  of: string;                  // the ref this file belongs to (the item or a dependency)
  detect?: Detect[];           // layout rules, unpacked archives only: they replace dst / root (below)
};
type Target = {
  source?: string;             // '*' default
  match?: string;              // e.g. a Thunderstore package name ('BepInEx-BepInExPack') or a file extension
  dst: string;                 // '{game}/BepInEx/plugins/{ns}-{name}' , '{game}/Data', '{game}/Mods', ...
  root?: string;               // e.g. 'BepInExPack' inside the BepInEx pack zip
  unpack?: boolean;
  detect?: Detect[];           // copied into the plan file (dst placeholders filled) when the file is an archive
};
type Detect = {
  ifContains: string | string[];   // one pattern or several (any of them)
  dst?: string;                // '{game}/...': where the archive goes when the rule matches
  up?: number;                 // 0-4: the root is that many folders above where the pattern matched
  refuse?: string;             // instead of dst: refuse the archive ('fomod', ...)
};
```

#### Layout rules (`detect`)

The server only knows a file's name; how a mod archive is laid out (a wrapper folder, `Data/` or not, a `~mods`
path inside) is only known once it is downloaded. So a plan file can carry layout rules the **core** applies to the
archive's listing (zip or 7z, read without unpacking, `install/archive.rs` `list` + `detect`) before it is installed:

- Patterns, case-insensitive, on whole path segments: `*` any file; `*.pak` a file with that ending; `Data/` or
  `archive/pc/mod/` those folders in a row; `manifest.json` or `fomod/ModuleConfig.xml` a file (with the folders
  before it). Each occurrence implies a root folder: the archive's top for `*`, the folder holding the file for
  `*.ext` and file patterns (holding the first segment for `a/b.c`), the folder holding the first folder for folder
  patterns; `up` moves that root up (an occurrence with fewer folders above it does not count).
- Rules are tried in order; the first with an occurrence decides. Among its occurrences the shallowest roots win (a
  rule with several patterns works like Vortex's `stopPatterns`: the shallowest plugin / data folder sets the root).
  One root: the archive goes to the rule's `dst` with that root stripped (`root`), files outside it left out.
  Several different roots at that depth (`Option A/`, `Option B/`): refused, the player picks on the mod's page.
  `refuse`: refused with that reason (`fomod`: an installer with options). No rule matches: refused.
- Refusals are `unsupported_archive` with a message naming why (FOMOD installer, several variants, layout not one of
  the game's), which the UI shows as "install from its page" (`link_only`).
- The core checks every rule like a `dst`: at most 40 rules of at most 40 patterns, patterns plain (no `..`, `:`,
  `\`, wildcard inside), `dst` under `{game}`, `up` ≤ 4, `refuse` `[a-z_]{1,32}`; rules on a file that is not
  unpacked are a `bad_plan`. The root it finds is checked like a plan `root` (plain segments Windows can write).
- A plan file with rules keeps a `dst` (where a bare, not archived file of that target goes) for older apps.

Example (an Unreal Engine game; `{game}/Pal/...` for Palworld):

```json
"detect": [
  { "ifContains": "fomod/ModuleConfig.xml", "refuse": "fomod" },
  { "ifContains": "LogicMods/", "dst": "{game}/Pal/Content/Paks" },
  { "ifContains": "*.pak", "dst": "{game}/Pal/Content/Paks/~mods" }
]
```

`Cool Mod v2/Cool_P.pak` + `Cool Mod v2/Cool_P.utoc` + `readme.txt` installs `Cool_P.pak` and `Cool_P.utoc` into
`Pal/Content/Paks/~mods`; `Red/a.pak` + `Blue/a.pak` is refused (variants); `MyLuaMod/Scripts/main.lua` is refused
(a UE4SS script mod: no rule).

Archives: zip and 7z are unpacked (the same rules for both: entry names made plain relative paths, `..` / absolute /
drive / `:` names abort the install before anything is written, at most 100 000 entries and 8 GiB unpacked; 7z
headers at most 64 MiB raw or unpacked, with their coders' dictionaries at most 1 GiB; encrypted 7z refused. Known
limit: the data blocks' dictionaries are not bounded, `sevenz-rust2` keeps coder properties private, so a crafted 7z
can make the decoder ask for up to 4 GiB of memory and fail). RAR is
not: the only full decoder is UnRAR, whose license is not open source, and no permissively licensed pure-Rust decoder
exists, so a `.rar` file (by name in the plan, or by its first bytes once downloaded) is `unsupported_archive`.

Rules:
- A game with no `targets` gets `link` plans only (the app never guesses a folder).
- Thunderstore: dependencies resolved recursively from the package index (latest versions), BepInExPack /
  MelonLoader installed into `{game}`, plugins into `{game}/BepInEx/plugins/<ns>-<name>` (r2modman layout).
- Minecraft (`mr`, `cf` with game `minecraft`): see below.
- Nexus: free users get `needs: 'nxm'` and the app opens the mod's files page; the `nxm://` click brings key + expiry
  to the core, which asks Nexus for the download link with the player's key. Premium: direct link. FOMOD installers
  are refused by the layout rules (`fomod`) once the archive is downloaded (not handled in this version).
- Nexus / GameBanana / CurseForge (games other than Minecraft): one catch-all target per source with layout rules
  (`detect`, above), plus extension targets for bare files (`.pak` → `~mods`, `.archive` → `archive/pc/mod`).

### Minecraft: profiles

Minecraft mods install into a **SIGF-managed Prism instance per (Minecraft version, loader)**: "SIGF 1.21.1 Fabric",
folder `<Prism data>/instances/sigf-<version>-<loader>` (loaders `fabric`, `neoforge`, `forge`, `quilt`). The game
page picks the profile (latest release with Fabric by default, remembered on the PC; versions and loaders from
`/mc-versions`), and every Minecraft search and plan carries it as `mc` + `loader`:

- Search: Modrinth facets `versions:<mc>` and `categories:<loader>` (Quilt also lists Fabric mods); CurseForge
  `gameVersion` + `modLoaderType` (1 Forge, 4 Fabric, 5 Quilt, 6 NeoForge).
- Plan: the newest compatible version (Modrinth `/project/:id/version?game_versions=[..]&loaders=[..]`, CurseForge
  files filtered the same way; releases before betas before alphas), required dependencies recursively under the same
  constraints, Modrinth's sha512 + sha1 / CurseForge's sha1 + md5. `dst` by kind, plain files only: mods
  `{instance}/mods`, resource packs `{instance}/resourcepacks`, shaders `{instance}/shaderpacks`; modpacks, datapacks
  and anything else are links. No compatible version: a link (why `no_version`). The plan carries
  `instance: { mc, loader, loaderVersion }` (Prism's recommended loader build for that version, from
  `meta.prismlauncher.org`); a loader with no build for the version is `400 mc_loader_unavailable`.
- Without `mc` + `loader` (apps before profiles) plans stay links (why `use_minecraft_flow`).

The core (`src-tauri/src/mcprofile.rs`) writes the instance on the first install into it (`instance.cfg`,
`mmc-pack.json` with Minecraft + the loader, the `.sigf-instance` marker `profile:<version>-<loader>`), in the Prism
data folder the scan finds, never from a path the webview gives; a folder of that name without our marker, or one
that resolves elsewhere through a link, is refused. `{instance}` is the instance's game folder (`.minecraft`, or
`minecraft` as Prism picks it), the engine's `{game}` for that plan, so snapshot and restore work as for any game: an
uninstall restores what the mod wrote, the instance stays. No Prism: `needs_launcher`, the app opens Prism's download
page. Play on the page: `prismlauncher --launch sigf-<version>-<loader>` (`mc_profile_play`). A mod installed in one
profile shows as not installed in another; installing it there moves it (registry id `mod/<ref>` is one install).

## 5. Core (`src-tauri/src/mods.rs`)

- `mods_install(planJson, gameDirs)`: checks the plan (hosts per source allowlist, `dst` inside `{game}`, sizes ≤ 4
  GB, at least one hash verified when the source gives one, layout rules), downloads, applies each archive's layout
  rules to its listing (section 4), installs with snapshot, emits `install://progress`. Archives: zip and 7z
  (`install/archive.rs`, told apart by their first bytes; `sevenz-rust2`, Apache-2.0, decoders only), RAR refused.
  A plan with `instance` (Minecraft, `mr`/`cf` only) goes into that profile's instance (section 4); `gameDirs` is
  not used for it.
  Uninstall = the existing `restore("mod/<ref>")`.
- Download hosts (the final host after redirects is checked too): `ts` thunderstore.io, gcdn/ccdn.thunderstore.io;
  `cf` edge.forgecdn.net, mediafilez.forgecdn.net; `nx` *.nexus-cdn.com; `mio` api.mod.io, *.modapi.io, *.modcdn.io;
  `gb` *.gamebanana.com; `mr` cdn.modrinth.com.
- `nxm://<domain>/mods/<mod>/files/<file>?key=&expires=&user_id=`: deep link registered next to `sigf://`; routed
  to the UI as `{ kind: 'nxm', ... }` through the same pending-links path. The core keeps the player's Nexus key
  (from SSO) in `<SIGF_HOME>/nexus.json`; commands `nexus_status`, `nexus_login` (SSO websocket
  `wss://sso.nexusmods.com`, app slug from build config, inactive until the slug exists), `nexus_logout`,
  `nexus_download_links(domain, mod, file, key?, expires?)`.
- "Use my Nexus API key" (no SSO needed): `nexus_set_key(key)` takes the player's personal key from
  `https://www.nexusmods.com/users/myaccount?tab=api` (trimmed, 16 to 512 chars of `[A-Za-z0-9+/=_-]`), checks it with
  `GET https://api.nexusmods.com/v1/users/validate.json` (headers `apikey`, `Application-Name: SIGF`,
  `Application-Version`) and keeps it in `nexus.json` exactly like an SSO key (never sent back to the webview, never
  logged). Answers `{ connected, name, premium }`, or `{ code: 'nexus_key_invalid' }`. Nexus items with a target are
  then installable: Premium through direct links, free accounts through the `nxm://` click.

## 6. Libraries across sources

`Library.items` entries become refs. A bare number is still a Steam Workshop id (existing libraries keep working).
`Library` gains `game?: string` (canonical id) for games that aren't on Steam. Apply installs each item through its
source (Workshop subscribe, engine plans for the rest); Remove undoes only what the library added.

## 7. UI: the game page

Tabs: **Mods** (every source merged, a source filter, dedup by title + author, source badge on each card),
**Mashups** (catalog filtered to the game), **Servers** (hosted servers + lobbies for the game), **Libraries**,
**Workshop** (Steam games only; the current Browse/Subscribed).
The Mods tab shows what installs first: an "Installable" chip, on by default, keeps the cards the app can install
(the proxy's `installable`, and for Nexus also the player's key); the others wait under "Show N more on the web" as
secondary cards (a small link to their page instead of a primary button). A source whose loaded mods are all
link-only says why on its chip ("log in to install", "no install rule yet"...); the Nexus one opens the account sheet.
The count shown is the installable count. Header: Play (vanilla) and the installed mod count
with Restore all. Every game in Library opens its page; the rail entry "Workshop" becomes "Games".

## 8. Human steps

- CurseForge: third-party API key application (form reviewed by Overwolf), key → `CURSEFORGE_API_KEY`.
- Nexus Mods: application slug + connection token for SSO, from a Nexus Community Manager.
- mod.io: API key from mod.io account settings (API access), key → `MODIO_API_KEY`.
