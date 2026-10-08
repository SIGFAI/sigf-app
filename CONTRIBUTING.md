# Contributing to SIGF

Thanks for helping. Issues and pull requests are welcome.

## Before you start

- **Security problems:** do not open an issue or a pull request. Report them privately through
  [GitHub private vulnerability reporting](https://github.com/SIGFAI/sigf-app/security/advisories/new), see
  [SECURITY.md](SECURITY.md).
- **Bugs:** open an issue with the bug report form. The app's **Report a bug** button (a mashup's page, the Installs
  tab, or Privacy > Report an app bug) writes a report you can paste.
- **A bug in a mod itself** (its gameplay, not the install): report it to the mod's author; each catalog card links to
  its tracker.
- **Larger changes** (a new feature, a new store or launcher, a change to the recipe format): open an issue first so we
  can agree on the approach before you spend time on it.

## Build and test

The build steps are in the [README](README.md#build-from-source). Before you open a pull request, run from the repo
root:

```bash
npm ci
npm run build                 # type check (tsc) + UI build; the Rust tests need dist/
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run check-i18n            # every UI text goes through the dictionaries in src/i18n/
```

On Windows, build the Steam helper once first (`node steam-helper/build.mjs`): the Windows build bundles it.

## Pull requests

- Keep a pull request to one change, and say what it changes and how you tested it.
- Add or update tests for engine changes (`src-tauri/src/install/`, `src-tauri/src/scan/`): the tests use the synthetic
  fixtures written by `src-tauri/tests/make-fixtures.mjs`, never real game or mod files.
- New UI text goes into `src/i18n/en.ts` (other languages may stay in English until a translator updates them).
- Do not commit binaries (DLL, EXE, JAR, ASI), game files, game art or anything you may not redistribute.
- Changes to what the app sends over the network must update the privacy table in the README and `docs/PRIVACY.md`.
- Dependency changes go through `package-lock.json` and `Cargo.lock` (`npm ci` and `cargo --locked` must pass).
  Changes to `.github/workflows/`, `src-tauri/capabilities/` or the updater configuration get an extra review.
- Every pull request is reviewed by a maintainer before it is merged (see the
  [code signing policy](CODE-SIGNING-POLICY.md) for the roles).

## License

SIGF is licensed under the GNU Affero General Public License version 3 only (`AGPL-3.0-only`). By submitting a
contribution, you agree that it is licensed under the same license, and you confirm that you have the right to submit
it under that license.

Everyone taking part in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
