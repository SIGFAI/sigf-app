# Steam Workshop in the SIGF app

Players browse, subscribe to and group Steam Workshop mods without opening the Workshop. Steam only.

## 1. Pieces

| Piece | Where | Does |
|---|---|---|
| Workshop proxy | sigf.ai, routes under `/api/app/workshop/*` | Browse/search (Steam Web API `IPublishedFileService/QueryFiles`, needs `STEAM_WEB_API_KEY`), item details and collections (`ISteamRemoteStorage/GetPublishedFileDetails`, `GetCollectionDetails`, no key). Cached, rate-limited, normalized. |
| Steam helper | `steam-helper/` (own crate, binary `sigf-steam.exe`, next to `SIGF.exe` with `steam_api64.dll`) | Talks to the player's running Steam client through the Steamworks SDK as the game's app id: subscribe, unsubscribe, item state. One process per call, exits when done, so Steam shows the game as running only for a few seconds. `SIGF.exe` itself never loads `steam_api64.dll`. |
| Core commands | `src-tauri/src/workshop.rs` + `lib.rs` | Spawn the helper (no console window), check the app id is a scanned Steam game, stream progress as `workshop://progress` events. Store the player's libraries in `<SIGF_HOME>/libraries.json`. Parse `sigf://library/...` links. |
| UI | `src/views/Workshop.tsx`, `src/lib/workshop.ts` | Per-game Workshop page (search, sort, cards, subscribe), item sheet, Libraries (create, add, apply, remove, share, import a Steam collection). |

## 2. Proxy API (JSON)

All ids are decimal strings (published file ids exceed 2^53).

`GET /api/app/workshop/browse?appid=<id>&sort=trend|top|new|updated&q=<text>&cursor=<c>&tag=<tag>`
→ `{ items: WorkshopItem[], next: string | null, total: number }` (30 per page). 503 `{ error: "workshop_search_unavailable" }` when no key.

`GET /api/app/workshop/items?ids=<id>,<id>…` (≤ 100) → `{ items: WorkshopItem[] }`

`GET /api/app/workshop/collection/<id>` → `{ collection: WorkshopItem, items: WorkshopItem[] }` (children resolved, nested collections flattened one level, ≤ 500 items; a plain item id gives `{ collection: item, items: [item] }`).

```ts
type WorkshopItem = {
  id: string; appid: string; title: string; description: string;   // description: plain text, ≤ 600 chars
  preview?: string;          // https image URL from Steam's CDN
  author?: string;           // persona name when Steam returns it
  subs: number; favs: number; votesUp?: number; votesDown?: number; score?: number; // score 0..1
  sizeBytes?: number; updated: number /* unix s */; created: number;
  tags: string[]; kind: 'item' | 'collection';
  children?: number;         // collections: item count
  requires?: string[];       // required items (children of type "required")
  url: string;               // https://steamcommunity.com/sharedfiles/filedetails/?id=<id>
};
```

## 3. Helper protocol

`sigf-steam.exe <command> <appid> [ids…]`, environment `SteamAppId=<appid>`. One JSON object per stdout line:

- `subscribe <appid> <ids…>`: `{"ev":"progress","id":"…","state":"subscribing|downloading|installed|failed","done":<bytes>,"total":<bytes>,"error":"…"}` per change, then `{"ev":"done","ok":true}`. Waits for downloads (timeout 30 min per call, keeps reporting).
- `unsubscribe <appid> <ids…>`: `{"ev":"done","ok":true}` (`ok: false` with `"error":"failed","message":"…"` when Steam refused or did not answer some).
- `state <appid> [ids…]`: no ids = every subscribed item. `{"ev":"state","items":[{"id","subscribed","installed","downloading","needsUpdate","sizeBytes"?}]}`.
- Failure to start (Steam not running, game not owned): `{"ev":"done","ok":false,"error":"steam_not_running|not_owned|init_failed","message":"…"}` with exit code 2. The core reads `init_failed` as `steam_not_running` when no Steam process runs (no process check before a call).

## 4. Core commands (Tauri)

```ts
workshop_subscribe(appid: string, ids: string[]): Promise<void>    // events workshop://progress { appid, id, state, done, total, error? }
workshop_unsubscribe(appid: string, ids: string[]): Promise<void>
workshop_state(appid: string, ids: string[] | null): Promise<ItemState[]>   // ItemState = { id, subscribed, installed, downloading, needsUpdate, sizeBytes? }
libraries_list(): Promise<Library[]>
libraries_save(lib: Library): Promise<Library[]>    // upsert by id
libraries_delete(id: string): Promise<Library[]>
lobby_api('GET', '/api/app/workshop/browse?…' | '/api/app/workshop/items?…' | '/api/app/workshop/collection/<id>', null, null): Promise<{ status, body }>
```

Errors come back as `{ code, message }` with code `steam_not_running | not_owned | not_steam_game | helper_missing | init_failed | failed`; the `libraries_*` commands reject with code `library`. The proxy goes through the core's `lobby_api` (GET only, query limited to `[A-Za-z0-9=&%,*+/._-]`, ≤ 2500 chars).

```ts
type Library = {
  id: string;            // 10 chars [a-km-z2-9], made by the UI
  appid: string; name: string; items: string[];   // ordered published file ids
  applied: boolean;       // last Apply succeeded and no Remove since
  addedByUs: string[];    // items this library subscribed that were not subscribed before (Remove unsubscribes only these)
  source?: { kind: 'collection' | 'link'; id?: string };
  created: number; updated: number;   // unix ms
};
```

## 5. Share links

`sigf://library/{appid}/{items comma-separated>?name=<urlencoded>` (≤ 200 items). Also accepted when pasted: the same after `https://sigf.ai/library/`. A received link opens the Workshop page of that game with an "Import library" sheet; nothing is subscribed without a click. Steam collection links (`steamcommunity.com/sharedfiles/filedetails/?id=…` or `/workshop/filedetails/?id=…`) pasted in the import box resolve through `/collection/<id>`.

## 6. Without a Steam Web API key

Browse/search needs the server's Steam Web API key. Without one it answers 503 and the app shows "Search is coming soon", while items, collections and libraries still work.
