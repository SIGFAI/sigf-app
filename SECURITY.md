# Security policy

## Reporting a vulnerability

Please report security problems privately. Do not open a public issue.

Use **GitHub private vulnerability reporting**: on [SIGFAI/sigf-app](https://github.com/SIGFAI/sigf-app), open the
**Security** tab and choose **Report a vulnerability**
([direct link](https://github.com/SIGFAI/sigf-app/security/advisories/new)). Only the maintainers can see the report.
This is the only reporting channel. Privacy requests, or anything about your own data, go through the same private
report ([privacy policy](docs/PRIVACY.md)).

Please include:

- what an attacker can do, and what they need first (a click on a link, a malicious lobby, control of a network,
  local access...);
- the app version (Settings or the installer name) and your Windows version;
- steps to reproduce, or a proof of concept;
- whether you want to be credited, and under which name.

## Scope

In scope:

- **The SIGF desktop app** in this repository: the UI (`src/`) and the Rust core (`src-tauri/`), including the
  installer we publish on the [Releases](https://github.com/SIGFAI/sigf-app/releases) page.
- **The recipe engine** (`src-tauri/src/install/`): downloads, hash checks, destination path checks, snapshots and
  Restore. Examples: a recipe that writes outside its target folder, a file that is used without matching its hash, a
  Restore that loses an original file.
- **The `sigf://` link handler and multiplayer joins** (`src-tauri/src/join.rs`, the Lobbies screens).
- **The catalog and lobby API the app uses** at `https://sigf.ai/api/app/*`, for example a recipe that passes the
  catalog's checks when it should not, or a way to read a lobby's address without its invite link.
- A mashup listed in the SIGF catalog that is malicious or does something its page does not say. We will pull it from
  the catalog.

Out of scope:

- Bugs in a mod's own gameplay code: report them to the mod's author (each catalog card links to its bug tracker).
  Tell us too if the bug is a security problem.
- The games, the stores (Steam, Epic, GOG, Ubisoft), Prism Launcher and other third-party software.
- The other pages of sigf.ai that the app does not use.
- Reports that need an attacker who already runs code as you on your PC.
- Missing hardening headers, rate limits or best practices with no demonstrated impact.

## What you can expect

SIGF is maintained by a small team. These are the times we can honestly commit to:

- **Acknowledgement within 7 days** of your report.
- **A first assessment within 14 days**: whether we can reproduce it and how serious we think it is.
- **A fix or a mitigation as fast as we can.** For a serious issue in the app, our goal is a patched release within
  30 days. Server-side issues on sigf.ai can often be fixed or mitigated (for example by pulling a catalog entry) the
  same day.
- We keep you informed until it is fixed, and we credit you in the release notes unless you ask us not to.

We do not run a paid bug bounty.

Please give us a reasonable time to ship a fix before you publish details. We will not take legal action against
good-faith research that stays within this policy: test on your own PC and your own lobbies, do not access other
people's data, and do not degrade sigf.ai for other players.

## Supported versions

Only the latest release gets security fixes. The app does not update itself: download the latest installer from the
Releases page.
