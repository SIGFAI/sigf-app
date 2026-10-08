# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

**Status:** our application to SignPath Foundation is pending. Until it is accepted, releases are built and published
by the workflow below but are **not code-signed**, and this page describes the process that will apply.

## What we sign

- We sign only files built from the source code in this repository,
  [SIGFAI/sigf-app](https://github.com/SIGFAI/sigf-app): the SIGF app (`SIGF.exe`) and its Windows installer
  (published as `SIGF-Setup-<version>.exe`).
- Every signed file is built by the release workflow in
  [`.github/workflows/release.yml`](.github/workflows/release.yml), on GitHub-hosted runners, from a tagged commit.
  Nothing built on a personal computer is ever signed.
- Separately from Authenticode, releases carry an updater signature (minisign, `.sig`) made over the final installer
  and the macOS update archive (after the Authenticode signing above). Installed apps accept an update only with a
  valid signature from the project's release key.
- Every release needs a manual approval in SignPath by one of the approvers below before it is signed.
- We do not sign third-party software. Mods and game files that the app downloads at run time are not signed by us and
  are not part of the signed installer.
- SignPath signs the Windows files only. The macOS app (`SIGF-<version>-mac.dmg`) is built by the same workflow and
  carries an ad-hoc signature, which names nobody; it is not signed with an Apple Developer ID or notarized yet. When
  it is, it will be signed in the same workflow with the Developer ID of the organization that publishes SIGF, never
  with a personal certificate.

## Team roles

- **Committers:** [pejisdev](https://github.com/pejisdev), [SIGFAI](https://github.com/SIGFAI) (the project's own account)
  - Committers may change the source code without additional review.
- **Reviewers:** [pejisdev](https://github.com/pejisdev)
  - Every change from anyone else (pull requests from contributors) is reviewed by a reviewer before it is merged.
- **Approvers:** [pejisdev](https://github.com/pejisdev)
  - An approver checks each release (tag, changes since the last release, build run) and approves its signing request
    in SignPath.

All committers, reviewers and approvers use multi-factor authentication on GitHub and on SignPath.

## Privacy policy

The full privacy policy is [docs/PRIVACY.md](docs/PRIVACY.md), also at [sigf.ai/privacy](https://sigf.ai/privacy). In
short: the app has no account, no ads, no telemetry, no analytics and no crash reports, and no machine or install id.
It sends information to other networked systems only to show you the catalog, pictures and lobbies, to tell you about
new versions of the app, and to install and play what you choose:

- **sigf.ai:** the mashup catalog at start (no identifier, no cookie; the only request before the first-launch privacy
  screen), the list of mashups being built and the free hosted server regions, a mashup's recipe when you install it,
  and a lobby id when you open an invite link. When the Lobbies tab or a mashup's "Play with friends" panel is open, the
  lobby list request includes the ids of the games you own among those SIGF has mashups for (optional). When you host
  a lobby: your display name, the lobby settings, the join address and the player count. Your PC's local network (LAN)
  address goes into the join address only when you click "Use my LAN address", or by itself if you chose "Fill in for
  me" (the default is "Ask each time"); anyone with the invite link can see it. sigf.ai keeps a salted hash
  (HMAC-SHA256 under a server-side secret) of your IP address with a lobby to limit abuse. sigf.ai runs behind
  Cloudflare.
- **GitHub (update check):** after the privacy screen and every 6 hours, a request for the latest release's
  `latest.json` (no identifier). An update downloads only when you click "Update and restart", and installs only if
  it carries a valid minisign signature from the project's release key.
- **GitHub and Modrinth's CDN:** file downloads when you install a mashup or join a lobby.
- **Steam, Epic Games and Modrinth image servers:** pictures of the games and mashups on screen (optional). For a game
  with no picture, the app sends the game's name to the Steam store search (optional).
- **Microsoft:** on a PC without the WebView2 runtime, the installer downloads it from Microsoft.

**Shown during installation, with options to turn it off.** The installer's first page shows the privacy text and a
link to the full policy, and at the end of the installation it asks "Allow SIGF's optional online requests?" (No turns
them all off). On its first start, before anything but the catalog request goes out, the app shows each choice ("Game
pictures", "Find missing pictures on Steam", "Lobbies for the games I own", "My local network address when I host":
Ask each time or Fill in for me). They can be changed any time under **Privacy** in the app, and the app's Rust core
enforces them.

Privacy requests: open a private report through
[GitHub Security Advisories](https://github.com/SIGFAI/sigf-app/security/advisories/new); general questions:
[issues](https://github.com/SIGFAI/sigf-app/issues). Third-party services have their own privacy
policies: [Steam](https://store.steampowered.com/privacy_agreement/), [Epic Games](https://www.epicgames.com/site/privacypolicy),
[GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement),
[Modrinth](https://modrinth.com/legal/privacy), [Microsoft](https://privacy.microsoft.com/privacystatement),
[Cloudflare](https://www.cloudflare.com/privacypolicy/).

## System changes and uninstall

The app installs per user, in `%LOCALAPPDATA%\Programs\SIGF`, and never asks for administrator rights. It registers
the `sigf://` link type for your user account. It changes game files only when you install a mashup, after saving a
copy of each original, and **Restore vanilla** puts the originals back. Its data (download cache, backups of original
files, the list of installed mashups, privacy choices) lives in `%LOCALAPPDATA%\SIGF`, a separate folder.

To uninstall: first use **Restore vanilla** on each installed mashup, then remove SIGF from **Settings > Apps >
Installed apps** (or run the uninstaller in its install folder). The uninstaller warns you if mashups are still
installed. Uninstalling removes the app and keeps `%LOCALAPPDATA%\SIGF`, even when you tick "Delete the application
data" (that removes only the app's WebView2 profile), so the backups of your original files survive. You can delete
that folder yourself once nothing is installed.
