# SIGF desktop app: privacy policy

Last updated: 2026-10-07. This file is the source of truth for the sigf.ai privacy page (`https://sigf.ai/privacy`), the
installer's privacy text (`src-tauri/windows/privacy.txt`, quoted at the end and checked against this file by
`cargo test`) and the app's Privacy screen (`src/views/Privacy.tsx`). Change them together. Paths are relative to the
app's folder. The app runs on Windows and macOS and sends the same things on both; where the two differ (folders, the
installer), this policy says so, and [On macOS](#on-macos) lists the differences.

## In short

SIGF has no account, no ads, no telemetry, no analytics and no crash reports. It never reads your store logins,
passwords or tokens, and it has no machine id or install id. It sends information to other systems only to show you
the catalog, pictures, live streams and lobbies, to check for a newer version of the app, and to install and play what you choose. Every request you did not ask for directly
can be turned off, and the app asks you about them before it makes any of them.

## What is sent, to whom, and why

| What | To | When | Why | Can you turn it off? |
|---|---|---|---|---|
| A request for the mashup catalog. No identifier, no cookie | sigf.ai | App start | To list the mashups | No: it is the app's content. It is the only request made before you answer the privacy screen |
| Requests for the list of mashups being built (launchpad) and for the free hosted server regions. No identifier | sigf.ai | App start, after the privacy screen | To show builds in progress and whether "Host on SIGF" is available | No |
| Requests for what is live (the main stream's build and the launchpad agents building), and each live build's latest picture. No identifier | sigf.ai | Only while the Live tab is open: the list every 15 seconds, the pictures every 4 seconds | To show the live builds | Do not open the Live tab |
| The video of the stream you open in the Live tab: its HLS playlist and video segments, about every 2 seconds. No identifier, no cookie | sigf.ai (it relays the main stream's live video and the launchpad agents' streams from SIGF's own media server, so the app only talks to sigf.ai) | Only while that stream's player is open. Back, Esc or leaving the Live tab closes it and stops the download | To play the stream inside the app | Do not open a stream: the Live tab's list and pictures load no video. Not tied to "Game pictures", which covers other companies' image servers, not sigf.ai's own content |
| Update check: a request for the latest version's `latest.json`. No identifier, no data beyond a normal request | GitHub (`github.com`, which redirects to its release file servers `objects.githubusercontent.com` / `release-assets.githubusercontent.com`) | App start, after the privacy screen, then every 6 hours | To tell you when a new SIGF version is out ("SIGF x.y.z is available") | No. GitHub sees your IP address, like any request. The update itself downloads from the same GitHub release only when you click "Update and restart", and the app installs it only if it carries SIGF's release signature |
| Image requests for game, mashup and creator pictures. These servers can tell which games are on your screen | Steam (`*.steamstatic.com`), Epic Games (`cdn1.epicgames.com`, `cdn2.unrealengine.com`) and Modrinth (`cdn.modrinth.com`) image servers | While pictures are on screen | To show pictures | **Yes**: "Game pictures". Off: plain colored tiles. Pictures already in Steam's own cache on your PC still show (they are read from disk) |
| The name of a game that has no picture | Steam store search (`store.steampowered.com`, then `api.steampowered.com` with that game's Steam app id) | A tile with no picture (some Ubisoft, GOG and Epic games) | To find its picture. The answer is cached on your PC | **Yes**: "Find missing pictures on Steam". Also off when "Game pictures" is off |
| The ids of the games you own, among the games SIGF has mashups for | sigf.ai | While the Lobbies tab or a mashup's "Play with friends" panel is open, every 10 seconds | So sigf.ai lists only lobbies you can join | **Yes**: "Lobbies for the games I own". Off: the app gets every public lobby and picks yours on your PC |
| A request for a mashup's recipe | sigf.ai | When you click Get or join a lobby | To know what to download | You asked for the install |
| File downloads | GitHub release servers, Modrinth's CDN | When you install a mashup or join a lobby | To install it | You asked for the install. Like any download, these servers see your IP address |
| Build downloads: pinned source files, and once a compiler and Python. No identifier | GitHub (`github.com` with `codeload.github.com` for source archives, `raw.githubusercontent.com` for single source files, and its release file servers for w64devkit) and python.org (`www.python.org`, the Python 3.12.10 embeddable package) | Only when you install a mashup that builds a file on your PC (`player_build`). The compiler (w64devkit 2.10.0) and Python are downloaded the first time only, then kept in `%LOCALAPPDATA%\SIGF\tools`. Windows only: on macOS these mashups are not offered | To build files SIGF may not distribute (for example a library compiled from a game's decompiled code). Every file is pinned by its hash; the build itself needs no network | You asked for the install. These servers see your IP address |
| Your own copy of a game file (a ROM you dumped) | Nobody | Only for a mashup that uses one (`own_copies`), when you click Get | SIGF looks for it in your Downloads, Desktop, Documents and ROM folders, or you pick it, checks its SHA-1 on your PC and copies it into that mashup's folder | It never leaves your PC: SIGF never ships, downloads or uploads it. Restore vanilla deletes the copy |
| A lobby id | sigf.ai | When you open an invite link | To show you the lobby before you confirm the join | You asked to join |
| Your display name, the lobby mode, the player limit, the join address, and the player count (every 30 seconds) | sigf.ai | Only when you host a lobby | So friends can find and join you | Do not host. **The join address is filled in with your PC's local network (LAN) address only when you click "Use my LAN address"**, or by itself if you chose "Fill in for me" under Privacy. Anyone with the invite link can see the address; a public lobby list never shows it |
| The lobby and the region you pick | sigf.ai | Only when you choose "Host on SIGF (free)" | To start a Minecraft server for your lobby | Do not use it |
| A status check (Minecraft Server List Ping) | Your own Minecraft server | Every 30 seconds while you host a Minecraft lobby | To count players | Do not host |
| A bug report: the app and system versions, the mashup and its install state, the games found (store and build), the last error the app showed and the last 40 lines of the mashup's build log, if one was kept. Your home folder becomes `~`; your user name and anything that looks like a password or token are removed | Nobody, from SIGF. GitHub (`github.com`) only if you click "Open on GitHub", and then from your web browser | Only when you click "Report a bug" (a mashup's page, the Installs tab) or "Report an app bug" (Privacy) | The report is written on your PC and shown to you in full first. "Open on GitHub" opens a new issue page in your browser with that text, on the tracker you pick: the mod author's for a bug in the mod itself (not offered when the author takes no reports), the mashup's SIGFAI copy for an install or app problem, or SIGFAI/sigf-app for the app. You edit it and submit it there, or close the page: nothing is posted until you submit it. "Copy report" is always there | Do not click it. The app itself never sends the report |
| The WebView2 runtime download | Microsoft | Windows only: during installation, only on a PC without WebView2 | The app's window needs it | No |

Pages you open from the app (store pages, GitHub pages and bug reports, the launchpad, a stream's "Open on sigf.ai" link, Prism Launcher's download page) open in your
web browser, under that site's own privacy policy.

## How to turn things off

- **In the installer** (Windows): the first page shows the short text quoted at the end, with a link to this policy. At the end of the installation, SIGF
  asks "Allow SIGF's optional online requests?": No turns all of them off.
- **On first start**: before anything but the catalog request goes out, SIGF shows each choice ("Game pictures",
  "Find missing pictures on Steam", "Lobbies for the games I own", and "My local network address when I host": Ask
  each time or Fill in for me), with the installer's answer filled in. Nothing else happens until you click Continue.
- **Any time**: Privacy, at the bottom of the app's left bar.

The choices are saved in `%LOCALAPPDATA%\SIGF\privacy.json` on Windows, `~/Library/Application Support/SIGF/privacy.json`
on macOS (or `<SIGF_HOME>/privacy.json`). The app's core enforces
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
- **Mashup submissions** (the sigf.ai/submit form, not the app): what the modder typed (repo, games, description,
  release tag), the automatic pre-check's result and the review's status and reason. The optional contact (X handle or
  email) is seen only by the SIGF team and deleted 90 days after the decision; a salted hash of the IP address (same
  salt) limits submissions per address and is deleted after 30 days. Withdrawing from the secret status link deletes
  the submission.
- **Rate limits**: sigf.ai counts requests per IP address in memory, in one-minute windows, to stop abuse. These counts
  are not stored.
- **Catalog, recipe and image requests**: SIGF keeps no record of them. sigf.ai runs behind Cloudflare, which carries
  every request to the site and keeps its own logs of them under
  [Cloudflare's privacy policy](https://www.cloudflare.com/privacypolicy/).

What stays on your PC only: the list of your games and their folders, downloads, backups of original game files, the
copies of your own game files a mashup uses and the files built for it (inside that mashup's folder, deleted by Restore
vanilla), the build tools (`%LOCALAPPDATA%\SIGF\tools`, kept for the next build), the list of installed mashups, the picture search cache, your privacy choices, the secrets of your hosted lobbies' worlds
(for 7 days), and your host display name, all under `%LOCALAPPDATA%\SIGF` and the app's WebView2 profile (on macOS:
`~/Library/Application Support/SIGF` and the app's WebKit data, see below).
The program itself is installed in `%LOCALAPPDATA%\Programs\SIGF`. Uninstalling it keeps `%LOCALAPPDATA%\SIGF`
(it holds the backups of your original game files); delete that folder yourself, once no mashup is installed, to
remove this data.

## On macOS

The Mac app sends exactly what the table above lists, with these differences:

- **No installer step.** The Mac app comes as a disk image (`.dmg`): you drag SIGF to Applications. The privacy choices
  are asked on the first start, before anything but the catalog request goes out, as on Windows.
- **Folders.** SIGF's data (downloads, backups of original game files, the list of installed mashups, privacy choices,
  the picture search cache) is in `~/Library/Application Support/SIGF`. The app's window keeps its web data in
  `~/Library/WebKit/ai.sigf.app` and `~/Library/Caches/ai.sigf.app`. The program is `SIGF.app` where you put it
  (usually `/Applications`). Moving it to the Trash removes the program and keeps
  `~/Library/Application Support/SIGF` (it holds the backups of your original game files); delete that folder yourself,
  once no mashup is installed, to remove this data.
- **What the scan reads** (on your Mac only, nothing is sent): Steam's library files in
  `~/Library/Application Support/Steam`, Epic's install manifests in
  `~/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests`, GOG games' own `goggame-*.info` files in
  `/Applications`, `~/Applications` and `~/GOG Games`, and the Prism Launcher, Modrinth App and Minecraft launcher
  folders in `~/Library/Application Support`.
- **No player builds and no WebView2.** Mashups that build a file on your PC need Windows and are not offered on a
  Mac, so the compiler and Python downloads never happen there. macOS ships its own web view.
- **Apple.** When you open an app, macOS itself may check its signature with Apple. That is macOS, not SIGF: SIGF
  sends nothing to Apple.

## Other services

These services have their own privacy policies: [Steam](https://store.steampowered.com/privacy_agreement/),
[Epic Games](https://www.epicgames.com/site/privacypolicy),
[GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement),
[Modrinth](https://modrinth.com/legal/privacy), [Python Software Foundation](https://www.python.org/privacy/), [Microsoft](https://privacy.microsoft.com/privacystatement),
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
- sigf.ai live streams: only while you watch a stream in the Live tab, the video of that stream. Closing the player stops it. No identifier.
- Steam, Epic Games and Modrinth image servers: pictures of the games and mashups on screen. These servers can tell which games are shown. You can turn this off.
- Steam store search: the name of a game that has no picture, to find one. You can turn this off.
- sigf.ai lobbies: while the Lobbies tab or "Play with friends" is open, the games you own that SIGF has mashups for, so you only see lobbies you can join. You can turn this off: SIGF then gets every public lobby and picks yours on your PC.
- sigf.ai, only when you host a lobby: your display name, the lobby settings and the join address. SIGF asks before it puts your PC's local network (LAN) address in it. Anyone with the invite link can see the address. sigf.ai keeps a salted hash of your IP address with the lobby to limit abuse.
- GitHub: when the app starts and every 6 hours, a check for a newer version of SIGF. Nothing is sent beyond a normal request. An update downloads only when you click "Update and restart".
- GitHub and Modrinth: file downloads, only when you install a mashup or join a lobby. A mashup that builds a file on your PC also downloads pinned sources from GitHub and, the first time, a compiler (w64devkit, from GitHub) and Python (from python.org).
- Nobody: a mashup that uses your own copy of a game file (a ROM you dumped) finds or asks for it on your PC and copies it into its own folder. That file is never uploaded or sent anywhere.
- Microsoft: only on a PC without WebView2, this installer downloads it.

Your choice: at the end of this installation, SIGF asks whether to allow the requests you can turn off. The first time it starts, it shows each choice again before it sends anything but the catalog request. You can change your choices any time under Privacy in the app.

Full privacy policy: https://sigf.ai/privacy
```
