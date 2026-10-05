# SIGF trademark policy

The SIGF app's code is open source (AGPL-3.0). You can copy, change and share it under that license. The license
covers the code. It does not give you the right to use the **SIGF** name or logo. This policy explains what you can and
cannot do with them. It follows the approach of the Mozilla and Rust trademark policies, in short form.

"SIGF marks" means the name "SIGF", the SIGF logo and wordmark, the app icon, and names or logos that look or sound
confusingly similar.

## You can, without asking

- Say true things about SIGF: "works with SIGF", "a mashup for SIGF", "I contributed to SIGF".
- Say your fork or project is **"based on SIGF"** or **"a fork of SIGF"**, as plain text, as long as your own name comes
  first and stands on its own.
- Use the name in articles, reviews, videos, streams, tutorials and talks about SIGF.
- Build and run unmodified SIGF from this repository for yourself, your team or your class, under its original name.

## You must rebrand a fork

If you distribute a modified version of the app (a fork, a repackage, a build with changes), you must:

- give it a different name, and replace the SIGF logo and app icon with your own;
- change the app identifier (`identifier` in `src-tauri/tauri.conf.json`, currently `ai.sigf.app`) and the `sigf://`
  link scheme, so your build cannot be mistaken for SIGF or take over SIGF invite links;
- point it at your own catalog and lobby service, or state clearly that it uses sigf.ai.

## You cannot

- Use the SIGF marks in a way that suggests SIGF made, endorses, sponsors or checked your product, fork, mod, service
  or token.
- Use the SIGF marks in your product, company, domain, social account or app-store name.
- Ship a modified build under the SIGF name or with the SIGF logo.
- Use the SIGF marks on anything misleading, harmful or illegal, including malware and phishing pages.

## Questions and permission

For anything not covered here, ask first: open an issue on [SIGFAI/sigf-app](https://github.com/SIGFAI/sigf-app). We may change this policy; the version in this repository is the current one.

Game names and game art shown in the app belong to their owners. SIGF is not affiliated with any game publisher.
