# adoken

ADO + 拳 (ken, "fist"). Local-first desktop client for Azure DevOps: snappy PR reviews, universal Cmd+P search, notification control, smart nudges. Open source (MIT, see `LICENSE`), built in public by @BenzyPrime.

## Stack
- Rust (edition 2024) + GPUI. macOS first, Windows at v0.3, Linux best-effort.
- Local store: see `docs/adr/` (storage decision). Secrets (PAT) in OS keychain only, never in the DB or logs.
- Auth: PAT (az cli-assisted or pasted) behind an `AuthProvider` trait so Entra ID OAuth can land later without touching callers.
- Scope: one ADO org per install, many projects. Multi-org is deferred to Later (user decision 2026-09-30; draft kept in `docs/specs/0011-organizations-and-local-data.md`).
- Reference codebase: [zeron](https://github.com/zeronsh/zeron) (MIT, by @winglee). Reuse is allowed with attribution in `THIRD_PARTY_NOTICES.md`. Never copy without recording the source path + commit.

## Layout
- `crates/` Rust workspace (app, ui, theme, store, ado client, sync, search, notify).
- `docs/specs/` one spec per feature (`NNNN-slug.md`, template in `docs/specs/0000-template.md`). No code without an accepted spec.
- `docs/adr/` architecture decisions.
- `docs/roadmap.md` milestones → versions.
- `marketing/` build-in-public: pain-point bank, image prompts. The strategy, tweet calendar, post drafts, media, `docs/pricing.md` and `docs/competitive/` are kept local (gitignored), not published.
- `public/` Astro site for adoken.dev (Cloudflare Pages).
- `.omp/agents/` specialist agents (product-manager, pricing-analyst, landing-page, social-writer, gpui-engineer, ux-designer, release-manager).

## Workflow
1. product-manager drafts/refines a spec → user accepts.
2. gpui-engineer implements with unit tests; every UI component gets a gallery story and a snapshot per built-in theme.
3. release-manager cuts versions: SemVer, Conventional Commits, `CHANGELOG.md` (Keep a Changelog).
4. social-writer turns shipped work into posts.

## Rules
- Performance is a feature: interactions < 16 ms frame, Cmd+P results < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.
- All UI colors/spacing/motion/sound come from theme tokens. No hard-coded colors.
- Every sound and animation is optional and respects reduce-motion.
- "Ken" flavor is playful but original: no Capcom/Street Fighter names, sprites, or audio.
- Git identity: personal (benzara.tahar@gmail.com), set automatically via `includeIf`.
