# SIGF desktop app: privacy policy

Last updated: 2026-10-05. This file is the source of truth for the sigf.ai privacy page (`https://sigf.ai/privacy`), the
installer's privacy text (`src-tauri/windows/privacy.txt`, quoted at the end and checked against this file by
`cargo test`) and the app's Privacy screen (`src/views/Privacy.tsx`). Change them together. Paths are relative to the
app's folder.

## In short

SIGF has no account, no ads, no telemetry, no analytics and no crash reports. It never reads your store logins,
passwords or tokens, and it has no machine id or install id. It sends information to other systems only to show you
the catalog, pictures and lobbies, and to install and play what you choose. Every request you did not ask for directly
can be turned off, and the app asks you about them before it makes any of them.

## What is sent, to whom, and why

| What | To | When | Why | Can you turn it off? |
|---|---|---|---|---|
| A request for the mashup catalog. No identifier, no cookie | sigf.ai | App start | To list the mashups | No: it is the app's content. It is the only request made before you answer the privacy screen |
| Requests for the list of mashups being built (launchpad) and for the free hosted server regions. No identifier | sigf.ai | App start, after the privacy screen | To show builds in progress and whether "Host on SIGF" is available | No |
| Image requests for game, mashup and creator pictures. These servers can tell which games are on your screen | Steam (`*.steamstatic.com`), Epic Games (`cdn1.epicgames.com`, `cdn2.unrealengine.com`) and Modrinth (`cdn.modrinth.com`) image servers | While pictures are on screen | To show pictures | **Yes**: "Game pictures". Off: plain colored tiles. Pictures already in Steam's own cache on your PC still show (they are read from disk) |
| The name of a game that has no picture | Steam store search (`store.steampowered.com`, then `api.steampowered.com` with that game's Steam app id) | A tile with no picture (some Ubisoft, GOG and Epic games) | To find its picture. The answer is cached on your PC | **Yes**: "Find missing pictures on Steam". Also off when "Game pictures" is off |
| The ids of the games you own, among the games SIGF has mashups for | sigf.ai | While the Lobbies tab or a mashup's "Play with friends" panel is open, every 10 seconds | So sigf.ai lists only lobbies you can join | **Yes**: "Lobbies for the games I own". Off: the app gets every public lobby and picks yours on your PC |
| A request for a mashup's recipe | sigf.ai | When you click Get or join a lobby | To know what to download | You asked for the install |
| File downloads | GitHub release servers, Modrinth's CDN | When you install a mashup or join a lobby | To install it | You asked for the install. Like any download, these servers see your IP address |
| A lobby id | sigf.ai | When you open an invite link | To show you the lobby before you confirm the join | You asked to join |
| Your display name, the lobby mode, the player limit, the join address, and the player count (every 30 seconds) | sigf.ai | Only when you host a lobby | So friends can find and join you | Do not host. **The join address is filled in with your PC's local network (LAN) address only when you click "Use my LAN address"**, or by itself if you chose "Fill in for me" under Privacy. Anyone with the invite link can see the address; a public lobby list never shows it |
| The lobby and the region you pick | sigf.ai | Only when you choose "Host on SIGF (free)" | To start a Minecraft server for your lobby | Do not use it |
| A status check (Minecraft Server List Ping) | Your own Minecraft server | Every 30 seconds while you host a Minecraft lobby | To count players | Do not host |
| The WebView2 runtime download | Microsoft | During installation, only on a PC without WebView2 | The app's window needs it | No |

Pages you open from the app (store pages, GitHub pages, the launchpad, Prism Launcher's download page) open in your
web browser, under that site's own privacy policy.

## How to turn things off

- **In the installer**: the first page shows the short text quoted at the end, with a link to this policy. At the end of the installation, SIGF
  asks "Allow SIGF's optional online requests?": No turns all of them off.
- **On first start**: before anything but the catalog request goes out, SIGF shows each choice ("Game pictures",
  "Find missing pictures on Steam", "Lobbies for the games I own", and "My local network address when I host": Ask
  each time or Fill in for me), with the installer's answer filled in. Nothing else happens until you click Continue.
- **Any time**: Privacy, at the bottom of the app's left bar.

The choices are saved in `%LOCALAPPDATA%\SIGF\privacy.json` (or `<SIGF_HOME>\privacy.json`). The app's core enforces
them for its own requests: it refuses the Steam search and a lobby list carrying your games when they are off, and
refuses everything but the catalog request before the first answer. Pictures load in the app's window, which loads
only the pictures your choices allow.

## Never collected

- No account, login, email address or password. SIGF never reads Steam, Epic, GOG, Ubisoft, Prism Launcher or Minecraft
  login files or tokens.
- No telemetry, analytics, usage statistics, crash reports, advertising or tracking ids.
- No machine id, install id or hardware fingerprint.
- No list of your games, folders or files leaves your PC, except the owned-game ids above when that choice is on.

## What sigf.ai keeps

- **Lobbies**: the mashup and version, your display name, the lobby settings, the join address, a hash of the host's
  secret, and a hash of your IP address. The IP hash is an HMAC-SHA256 under a secret salt kept on the server
  (`LOBBY_IP_SALT`), so it cannot be turned back into your address by trying every address; it is used only to limit
  how many lobbies and free servers one address can run at once. A lobby closes 90 seconds after its last heartbeat (at
  most 24 hours after it opened) and is deleted one day after it closes.
- **Free hosted servers**: the lobby id, the mashup, the region, the server's state and times, the same IP hash, and
  your world, kept for 7 days after the session so you can download it, then deleted.
- **Rate limits**: sigf.ai counts requests per IP address in memory, in one-minute windows, to stop abuse. These counts
  are not stored.
- **Catalog, recipe and image requests**: SIGF keeps no record of them. sigf.ai runs behind Cloudflare, which carries
  every request to the site and keeps its own logs of them under
  [Cloudflare's privacy policy](https://www.cloudflare.com/privacypolicy/).

What stays on your PC only: the list of your games and their folders, downloads, backups of original game files, the
list of installed mashups, the picture search cache, your privacy choices, the secrets of your hosted lobbies' worlds
(for 7 days), and your host display name, all under `%LOCALAPPDATA%\SIGF` and the app's WebView2 profile.
The program itself is installed in `%LOCALAPPDATA%\Programs\SIGF`. Uninstalling it keeps `%LOCALAPPDATA%\SIGF`
(it holds the backups of your original game files); delete that folder yourself, once no mashup is installed, to
remove this data.

## Other services

These services have their own privacy policies: [Steam](https://store.steampowered.com/privacy_agreement/),
[Epic Games](https://www.epicgames.com/site/privacypolicy),
[GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement),
[Modrinth](https://modrinth.com/legal/privacy), [Microsoft](https://privacy.microsoft.com/privacystatement),
[Cloudflare](https://www.cloudflare.com/privacypolicy/).

## Contact

Privacy requests, or anything about your own data: open a private report through GitHub Security Advisories at
https://github.com/SIGFAI/sigf-app/security/advisories/new. Only the maintainers can read it. General questions that
are not sensitive: https://github.com/SIGFAI/sigf-app/issues.

## Installer text

Shown on the installer's first page (`src-tauri/windows/privacy.txt`). Keep it word for word in step with that
file: `cargo test` compares them.

```text
SIGF and your privacy

SIGF has no account, no ads, no telemetry, no analytics and no crash reports. It never reads your store logins, passwords or tokens.

What SIGF sends, and to whom:

- sigf.ai (SIGF): the mashup catalog and the list of mashups being built when the app starts, and a mashup's recipe when you install it. No account, no identifier.
- Steam, Epic Games and Modrinth image servers: pictures of the games and mashups on screen. These servers can tell which games are shown. You can turn this off.
- Steam store search: the name of a game that has no picture, to find one. You can turn this off.
- sigf.ai lobbies: while the Lobbies tab or "Play with friends" is open, the games you own that SIGF has mashups for, so you only see lobbies you can join. You can turn this off: SIGF then gets every public lobby and picks yours on your PC.
- sigf.ai, only when you host a lobby: your display name, the lobby settings and the join address. SIGF asks before it puts your PC's local network (LAN) address in it. Anyone with the invite link can see the address. sigf.ai keeps a salted hash of your IP address with the lobby to limit abuse.
- GitHub and Modrinth: file downloads, only when you install a mashup or join a lobby.
- Microsoft: only on a PC without WebView2, this installer downloads it.

Your choice: at the end of this installation, SIGF asks whether to allow the requests you can turn off. The first time it starts, it shows each choice again before it sends anything but the catalog request. You can change your choices any time under Privacy in the app.

Full privacy policy: https://sigf.ai/privacy
```
