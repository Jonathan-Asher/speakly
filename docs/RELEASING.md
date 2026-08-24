# Releasing Speakly

Releases are driven by conventional commits on `main`. `release.yml` computes
the version (semantic-release), tags, creates a **draft** GitHub release, then
builds a signed + notarized dmg plus updater artifacts on a macOS runner and
attaches everything to the draft. **Publishing the draft is the ship switch** —
`https://github.com/Jonathan-Asher/speakly/releases/latest/download/latest.json`
(the in-app updater endpoint) only serves published releases.

## One-time setup: repository secrets

Same Apple values as the legacy repo, renamed to what `tauri-apps/tauri-action`
expects, plus three new ones:

| Secret | Value | Was (legacy repo) |
| --- | --- | --- |
| `APPLE_CERTIFICATE` | base64 .p12 Developer ID Application cert | `CSC_LINK` |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 password | `CSC_KEY_PASSWORD` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: <name> (<team>)` | *(new — `security find-identity -v -p codesigning`)* |
| `APPLE_ID` | Apple ID email | `APPLE_ID` |
| `APPLE_PASSWORD` | app-specific password | `APPLE_APP_SPECIFIC_PASSWORD` |
| `APPLE_TEAM_ID` | team id | `APPLE_TEAM_ID` |
| `TAURI_SIGNING_PRIVATE_KEY` | **content** of `~/.tauri/speakly-updater.key` (not the path) | *(new)* |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | empty string | *(new)* |

## Updater key custody

The updater keypair was generated with
`pnpm tauri signer generate -w ~/.tauri/speakly-updater.key --password ""`.

- Private key: `~/.tauri/speakly-updater.key` on the dev Mac — **never in the
  repo**. Back it up offline; losing it strands every installed app on its
  current version (the public key baked into shipped builds won't trust a new
  key's signatures).
- Public key: `~/.tauri/speakly-updater.key.pub`, embedded in
  `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.

## Cutting a release

1. Merge conventional commits to `main` (`feat:` → minor, `fix:` → patch,
   `feat!:`/`BREAKING CHANGE` → major). `chore:`/`docs:`/`refactor:` alone
   release nothing.
2. The `version` job creates `vX.Y.Z` + draft release with generated notes;
   `scripts/sync-version.mjs` keeps `package.json`, `tauri.conf.json`,
   `Cargo.toml` and `Cargo.lock` in lockstep via a `chore(release)` commit.
3. The `build` job attaches: `Speakly_X.Y.Z_aarch64.dmg`, the updater
   `.app.tar.gz` + `.sig`, and `latest.json`.
4. Sanity-check the dmg from the draft, then **Publish release**. Installed
   apps pick it up on their next launch check (or Settings → Check for
   updates).

Failed build for an existing tag? Actions → Release → *Run workflow* with the
tag.

## First release bootstrap

The repo starts at `v2.0.0` with no prior tag: semantic-release treats the
next qualifying push as the first release and will propose `1.0.0` — override
once by creating tag `v2.0.0` on the current release commit
(`git tag v2.0.0 && git push origin v2.0.0`) **before** enabling the workflow,
so versioning continues from 2.x. Subsequent releases need no intervention.

## How the in-app updater works

- On launch (Settings toggle, default on) and via Settings → Check for
  updates, the app fetches `latest.json` from the endpoint above.
- `latest.json` carries the new version + platform URL + minisign signature;
  the app verifies the signature against the baked-in public key, downloads
  the `.app.tar.gz`, swaps itself, and relaunches on user confirmation.
- Dev builds log check failures at `debug` and stay silent — there is nothing
  published to compare against.
