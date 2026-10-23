# Contributing to adoken

Thanks for helping. adoken is a small project with strong opinions about speed and noise, so this page explains
how changes land. Build and run instructions are in the [README](README.md); the product rules are in
[`AGENTS.md`](AGENTS.md).

## Before you start

- **Bugs:** open an issue with the bug template. Include your macOS version, adoken version (Settings › About) and
  steps to reproduce. If it involves synced data, try to reproduce it in demo mode (`ADOKEN_DEMO=1`) so you can
  share screenshots without exposing your org.
- **Features:** open an issue first. Every feature needs a spec in `docs/specs/` (template:
  [`docs/specs/0000-template.md`](docs/specs/0000-template.md)) that is accepted before code is written. A feature
  has to make adoken snappier or quieter; one that serves neither is cut.
- **Small fixes** (typos, obvious bugs, docs) can go straight to a pull request.

## Never share secrets

Don't paste a Personal Access Token, an organization URL you'd rather keep private, or real work item / PR
content into issues, logs or screenshots. The Network log export (`ADOKEN_SYNC_LOG`) is redacted, but read it
before you attach it.

## Development

```sh
cargo run -p adoken                 # the app, against your own org
ADOKEN_DEMO=1 cargo run -p adoken   # fictional Contoso org, no network, no Keychain
cargo run -p adoken-gallery         # every UI component; ⌘T cycles themes
cargo test --workspace              # unit, smoke and snapshot tests
cargo fmt --all
cargo clippy --workspace --all-targets
```

## Rules a pull request must follow

- **Performance is a feature.** Interactions stay under a 16 ms frame, ⌘P results under 50 ms on 100k items, cold
  start under 500 ms. Run `scripts/perf.sh` when you touch a hot path; regressions are bugs.
- **Theme tokens only.** Colors, spacing, motion and sound come from `crates/theme` tokens
  ([`docs/design/tokens.md`](docs/design/tokens.md)). No hard-coded colors.
- **UI components** get a gallery story and a snapshot per built-in theme. Rewrite goldens with
  `UPDATE_SNAPSHOTS=1 cargo test -p adoken-gallery --test snapshots` and check the PNG diff before committing.
- **Motion and sound are optional** and respect Reduce Motion.
- **Secrets** live in the OS keychain only: never in the store, settings, logs or test fixtures.
- **Azure DevOps differs per org.** Don't hard-code a process template's work item types or state names; use the
  state categories Azure DevOps returns.
- **Original art and sound.** The "Ken" flavor is playful but original: no Capcom/Street Fighter names, sprites or
  audio.
- **Third-party code:** when you copy or adapt code, add the source path, commit and license to
  [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) in the same change.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/): `feat(review): …`, `fix(sync): …`, `docs: …`.
- Add a line under `## [Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md) for user-visible changes.
- Keep pull requests focused; one feature or fix each. CI runs format, clippy and tests on macOS.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
