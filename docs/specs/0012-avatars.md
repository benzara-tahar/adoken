# 0012 — Avatars: real profile pictures, initials fallback

| Field | Value |
| --- | --- |
| Status | Accepted (user request 2026-09-30) |
| Milestone | v0.0.5 (see `docs/roadmap.md`) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/model` (person image reference), `crates/sync` (capture + low-priority fetch through the governor and ledger), `crates/store` (avatar cache metadata), `crates/app` (rendering, decode, in-memory cache), `crates/gallery` (stories + snapshots); `crates/demo` for generated demo images |
| Serves | Less noise (people recognized at a glance instead of reading names) · Snappy (no spinner, no layout shift, no blocking) |
| Depends on | `docs/specs/0001-onboarding.md` (scopes, sign-out), `docs/specs/0002-sync.md` (governor FR-7–FR-9, ledger FR-10, TSTU budget), `docs/specs/0003-command-palette.md` (FR-8 `avatars` setting), `docs/adr/0001-local-store.md` (per-org data dir) |
| Design | `docs/design/principles.md` §4 Avatars, `docs/design/tokens.md` (`size.avatar.*`, `radius.full`) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

In adoken every person is an initials circle. "DK" the reviewer and "DK" the author look the same, and to see who left a comment or who is still waiting on you, you have to read the names. The Azure DevOps web UI shows profile pictures, so on this point adoken tells you less at a glance than the slow tool it replaces.

## 2. Users and job to be done

- **Persona:** developer on Azure DevOps who reviews and gets reviewed by the same 5–30 people every week.
- **Job to be done:** "When I scan my inbox, a PR's reviewers or a thread, I want to see faces I know, so I can tell who is involved without reading every name."
- **Smallest delightful slice:** reviewer lists, thread authors and inbox rows show real pictures, with no spinner, no jank and no layout shift. Anyone without a picture keeps today's initials.

## 3. UX flow

Visuals and motion: `docs/design/principles.md` §4 Avatars.

1. Any view that draws a person paints the initials circle at once, from the store. Surfaces: the signed-in user (sidebar account row and Settings › Account, Q-2), PR authors and reviewers (lists and review view), thread comment authors, work item assignees, inbox/notification rows, nudges, palette Person rows and `author_avatar` / `assigned_avatar` / `from_avatar` fields. Anywhere `palette::avatar` or an initials circle is drawn today.
2. If that person's picture is in the disk cache, it is read and decoded off the UI thread. Once decoded, it replaces the initials in the same box (same size, same position, no fade, no blank frame).
3. If it is not cached, the person is queued for a low-priority fetch (FR-4). When the image arrives it is written to disk, decoded off-thread and swapped in the same way. Nothing else on screen changes or waits.
4. Cached pictures are revalidated at most once per 24 h with a conditional request (FR-6). A changed picture shows up on the next draw after revalidation.
5. People without a picture, group and team identities (containers), and every failure show initials. The user never sees an error for an avatar.

No keyboard interaction: avatars are decoration. Screen readers keep the person's name as the label (principles §6).

### States

| State | Behavior |
| --- | --- |
| Empty | Person unknown or has no display name: the view's existing fallback (kind glyph in the palette) stays. |
| Loading / first sync | Initials. No spinner, no skeleton, no shimmer. |
| Error | Initials. Negative cache per FR-7; nothing is shown to the user. |
| Offline / stale | Cached pictures render; no avatar requests (0002 FR-5); stale pictures stay until the next revalidation online. |
| Throttled by ADO | Avatar fetches pause first (FR-5); cached pictures and initials render as usual. |
| Scope missing | `vso.identity` not granted: initials everywhere, zero avatar requests. Avatar endpoint refuses anyway: same, until the token changes (FR-9). |
| Demo | Generated images for some demo people, initials for the others; zero network (FR-13). |

## 4. Functional requirements

- **FR-1 Capture during sync.** When sync stores a person (PR author, reviewer, comment author, work item identity, "me" from `connectionData`), it keeps the identity GUID (the key for the image request, §5), the `imageUrl` when it is an `identityImage` URL on the org's own host, and whether the identity is a container (group/team). This is additive to `IdentityRef` / `Person` and costs no extra request.
- **FR-2 Lazy.** A picture is requested only after an avatar for that person has been drawn at least once. Sync never prefetches pictures for people who aren't on screen.
- **FR-3 One request per person.** Requests are deduplicated per identity (many rows showing the same person produce one request). Container identities are never requested; they always show initials.
- **FR-4 Through the governor and ledger.** Every avatar request goes through the 0002 rate governor and is recorded in the debug ledger (0002 FR-10) under its own class, `avatar`. The class ranks below cold (user-initiated > hot > warm > cold > avatar). At most 2 avatar requests are in flight at once, inside 0002 FR-7's 4 per host.
- **FR-5 Back off first.** Avatar requests pause while higher-priority requests are queued, while `Retry-After` is pending, and while `X-RateLimit-Remaining / Limit` < 50% (0002 FR-8). The class has its own ceiling of 1 TSTU per 5 minutes, measured from `X-RateLimit-Cost` or estimated at 60 requests per 5 minutes when the header is absent. It counts toward 0002 FR-9's ≤ 10 TSTU steady state.
- **FR-6 Disk cache and revalidation.** Image bytes are cached on disk inside the org data dir (`<data_dir>/orgs/<org>/avatars/`, ADR 0001; `0700` directory, `0600` files). Cache metadata (identity, validator `ETag` / `Last-Modified`, last checked, state) lives in the org store. A cached picture is revalidated at most once per 24 h with `If-None-Match` / `If-Modified-Since`. A `304` keeps the file, a `200` replaces it. Cached pictures render without any request.
- **FR-7 Negative cache and placeholders.** ADO's default images are stored as "no picture" and shown as initials: an `image/svg+xml` response (generic silhouette for unknown and service identities), any image response without an `ETag` (generated glyphs), a `404` and an empty body. "No picture" is re-checked at most once per 24 h. Transient failures (5xx, timeout, network) back off exponentially, with at most 3 attempts per identity per 24 h.
- **FR-8 Response guards.** Only a `200` with a raster image content type (`image/png`, `image/jpeg`, `image/gif` first frame, `image/webp` if the decoder supports it) and a body ≤ 1 MiB is decoded. HTML (a `203` sign-in page, a `400` for a malformed id) and oversized bodies count as failures (FR-7). SVG is never decoded (FR-7 placeholder).
- **FR-9 Scope gate and refusals.** Avatar requests are made only when the stored token was granted `vso.identity` (both scope sets in 0001 §5.2 include it; a pasted PAT may lack it). Without it: initials, zero requests. If the avatar endpoint still answers `401`/`403` while the rest of sync succeeds, the org is marked "avatars unavailable": initials everywhere and no further avatar requests until the token changes (Replace token, sign in again; amended 2026-10-01, owner notes; accepted 2026-10-01). An avatar `401` never trips the governor's unauthorized latch, never shows the 0001 FR-15 "token rejected" state and never pauses sync. If the token really is revoked, the next sync request's 401 does that (0001 AC-13).
- **FR-10 Org host, one endpoint.** Pictures are requested only from the `identityImage` endpoint on the connected org's own host (`dev.azure.com/{org}` or `{org}.visualstudio.com`), built from the identity GUID. `GraphProfile/MemberAvatars` URLs (the `_links.avatar.href` and the `imageUrl` in build payloads) are never requested because they answer `401` to an org PAT. Neither is any other captured URL. The `Authorization` header is never sent to any other host. There is no adoken server in the path (roadmap principles).
- **FR-11 Off the UI thread, bounded memory.** Disk reads, decoding and downscaling run on a background executor, never on the UI thread. Images are decoded once and downscaled to the largest rendered size (`size.avatar.lg` × the window scale factor). The in-memory decoded cache is bounded to 512 entries or 8 MiB, whichever comes first, least-recently-drawn evicted.
- **FR-12 Rendering.** Circle (`radius.full`) at the existing avatar sizes (`size.avatar.sm` / `md` / `lg` = 16 / 20 / 28). The image box has exactly the bounds of the initials circle it replaces (zero layout shift). Initials are both the fallback and the loading state. The swap is instant (`motion.instant`), so reduce motion needs no special case.
- **FR-13 Demo and gallery.** The demo org and gallery never use real photos. Some demo people get locally generated images (procedural, or bundled generated PNGs with their source recorded), the others keep initials, so both paths are visible. Demo mode makes zero avatar requests. Gallery story "Avatar": loaded, initials fallback, container identity, and each size; one snapshot per built-in theme (AGENTS.md).
- **FR-14 Palette setting.** `[palette] avatars = false` (0003 FR-8) hides avatars in palette rows, and palette rows then trigger no avatar fetch. Other views are unaffected. No new setting (Q-3).
- **FR-15 Local data lifecycle.** The avatar cache is local data of the org. Sign-out with "delete local data" (0001 §3, AC-15) removes it together with the store. Sign-out without it keeps it, like the store.

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Image reference (capture only) | Existing payloads: PR `createdBy` / `reviewers[]`, thread `comments[].author`, identities, `connectionData.authenticatedUser`: identity `id`, `imageUrl`, `isContainer` | `vso.code`, `vso.identity` (already requested) | No extra request (FR-1). In PR payloads `imageUrl` is already the `identityImage` URL. |
| **Picture (chosen)** | `GET https://dev.azure.com/{org}/_api/_common/identityImage?id={identity-guid}` (also on `{org}.visualstudio.com`); optional `&size=` variant | `vso.identity` gate (FR-9); works with the org-scoped PAT adoken minted (verified live) | Undocumented web endpoint (Risks). Picture → `200 image/png` (44 × 44 by default; `size=2` gives 220 × 220, ~96 KB) with `ETag: 1-<guid>`. `If-None-Match` → `304`. `Cache-Control: no-cache`, so adoken schedules its own revalidation (FR-6). Measured `X-RateLimit-Cost` ≈ 0.001–0.02 TSTU per request. |
| Rejected: GraphProfile member avatars | `GET https://dev.azure.com/{org}/_apis/GraphProfile/MemberAvatars/{descriptor}` (`_links.avatar.href`; build `imageUrl`) | — | `401` with an org PAT (verified live). Never called (FR-10). |
| Rejected: Graph avatars | `GET https://vssps.dev.azure.com/{org}/_apis/graph/Subjects/{subjectDescriptor}/avatars?api-version=7.1` | `vso.graph` (not in adoken's scope sets, 0001 §5.2) | Documented, with `isAutoGenerated`, but the Graph APIs answer `401` to adoken's PAT, and adding `vso.graph` would grow least privilege and force a re-mint for every user. |

Verified live on 2026-09-30 by the gpui-engineer against the personal demo org (PAT, read-only GETs; never the work org). Observed responses from the chosen endpoint: team/group id → `200 image/png` generated glyph without `ETag`; unknown or service identity → `200 image/svg+xml` silhouette (982 B); non-GUID id → `400` HTML; no `404` seen. A real user *without* a picture could not be observed in that org (Q-1).

- **Rate limits:** ≈ 0.001–0.02 TSTU per request (measured). Bounded by FR-5 (≤ 1 TSTU / 5 min, below cold, paused under 50% remaining) and by FR-2 / FR-6 / FR-7 (only people on screen, at most one request per person per 24 h after the first fetch). A reviewer's team of ~30 people costs ~30 requests on day one, then ~30 conditional `304`s a day: well under 1 TSTU.
- **No push to local apps:** a changed picture appears within 24 h plus the next draw (FR-6). Nothing here needs freshness.
- **PAT scopes:** no new scope. Requests are gated on `vso.identity` (FR-9). Without it, pictures degrade to initials, which matches the onboarding copy "Avatars show initials" (`docs/design/onboarding.md` §5, `crates/ado/src/onboarding/scopes.rs`).

## 6. Acceptance criteria

Harness: 0002 mock ADO server with recorded real-shaped fixtures, injected clock, temp data dir; frame recorder for perf ACs.

- **AC-1 (capture)** Given real-shaped PR, thread, identity and `connectionData` fixtures, after one sync pass every stored person has the image reference from its payload, and container identities are flagged. (unit, `crates/sync`)
- **AC-2 (lazy)** Given 50 synced people and a view that draws 10 of them, the mock server receives avatar requests for exactly those 10 and none for the other 40.
- **AC-3 (dedupe)** Given 200 rows by the same 3 people, exactly 3 avatar requests are made.
- **AC-4 (cache hit)** Given pictures fetched in a previous run less than 24 h ago (injected clock), relaunch renders them with zero avatar requests.
- **AC-5 (revalidation)** At 24 h + 1 min, each drawn person gets exactly one conditional request carrying the stored validator. A `304` keeps the file byte for byte. A `200` replaces it, and the next draw shows the new image.
- **AC-6 (negative cache)** An `image/svg+xml` silhouette, a PNG without `ETag` (fixtures recorded from the live org) and a `404` each render initials and produce no further request for that person within 24 h. A `500` is retried at most 3 times in 24 h.
- **AC-7 (guards)** A `203` HTML body, a `400` HTML body and a 2 MiB PNG are not decoded, render initials, and are negative-cached.
- **AC-8 (scope gate, refusal)** Given a token without `vso.identity`, zero avatar requests are made and initials render. Given the avatar endpoint returns `401` while every sync endpoint returns `200`, initials render everywhere, the next 30 simulated minutes make zero avatar requests, hot-tier sync requests continue, and no "token rejected" banner appears. After a token replace, avatar requests resume.
- **AC-9 (governor + ledger)** Every avatar request appears in the ledger under `avatar`. With queued hot/warm/cold work, no avatar request starts before them. With `X-RateLimit-Remaining / Limit` at 40%, zero avatar requests. Never more than 2 in flight.
- **AC-10 (budget)** Over a simulated steady hour with 300 distinct people drawn, the summed mocked `X-RateLimit-Cost` of the `avatar` class is ≤ 1 TSTU in every 5-minute window, and the total across all classes stays ≤ 10 TSTU (0002 FR-9).
- **AC-11 (endpoint, hosts)** Given fixtures whose `_links.avatar.href` / `imageUrl` point to `GraphProfile/MemberAvatars` or to `https://example.com/…`, the mock records zero requests to those URLs. Every avatar request is an `identityImage?id=` on the org host, and no request carries an `Authorization` header to a host other than the org's ADO hosts.
- **AC-12 (offline, 401)** Offline: zero avatar requests, cached pictures render. After a sync `401` (0001 AC-13): zero avatar requests until a replacement verifies (amended 2026-10-01, owner notes; accepted 2026-10-01).
- **AC-13 (no layout shift)** For each size, the bounds of an avatar element are identical in the initials, loading and loaded states (layout test), and a row's height and text x-position don't change when its picture arrives (gallery snapshot pair).
- **AC-14 (frames)** Scrolling a 1,000-row PR list with 200 distinct people while their pictures are fetched and decoded: no frame > 16 ms (frame recorder), and no decode or file read runs on the UI thread (executor assertion in debug builds).
- **AC-15 (memory)** After drawing 1,000 distinct people, the decoded cache holds ≤ 512 entries and ≤ 8 MiB. (unit)
- **AC-16 (palette setting)** With `[palette] avatars = false`, palette rows show no avatar and opening and scrolling the palette makes zero avatar requests. Other views still show pictures.
- **AC-17 (sign-out)** Sign-out with "delete local data" leaves no `avatars/` directory under the org data dir. Sign-out without it leaves the cache intact.
- **AC-18 (demo, gallery)** With `ADOKEN_DEMO=1`, zero network requests, and demo people show a mix of generated images and initials. Gallery story "Avatar" (loaded, initials, container, sm/md/lg) has a snapshot per built-in theme. No bundled demo/gallery image is a photograph of a real person (source of each asset recorded).
- **AC-19 (live, scripted manual check)** Connected to the personal demo org, the signed-in user's picture and at least one other person's picture render. Screenshot saved as `docs/screenshots/avatars-live.png`, showing no real photo other than the user's own.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Any frame while avatars load, decode or swap | < 16 ms | Frame recorder (AC-14) |
| Cached picture: first draw → on screen | < 50 ms (off-thread read + decode) | Timer in the gallery harness |
| Layout shift on swap | 0 px | AC-13 |
| Cold start impact | 0 ms: no avatar disk read or decode before the first frame | Launch timer (AGENTS.md < 500 ms) |
| Decoded cache memory | ≤ 8 MiB | AC-15 |
| Network | `avatar` class ≤ 1 TSTU / 5 min, total within 0002 FR-9 (≤ 10 TSTU / 5 min steady state) | Ledger (AC-10), dogfood day |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- Debug ledger (0002 FR-10) on a dogfood day: after the first day, `avatar` requests are mostly `304`s and never exceed one per person per 24 h.
- Dogfood: the builder identifies who is waiting on a review from the reviewer row without reading names. No "avatars flicker/jump" bug in the first week.
- Build-in-public: before/after screenshot of the review view **in the demo org only** (generated faces).

## 9. Risks

- **Endpoint stability.** `identityImage` is an undocumented web-portal endpoint and may change without notice. Keep the fetch behind one function. Any failure falls back to initials (FR-7, FR-9), so a breaking change costs pictures, never sync.
- **Scope mismatch.** The documented Graph endpoints need `vso.graph` and answer `401` to adoken's PAT. Adding the scope grows the least-privilege set and forces a re-mint for every user, so it needs a separate user decision.
- **401 misclassification.** A scope-missing avatar `401` must not look like a revoked token (FR-9, AC-8), and a revoked token must still be caught by sync (0001 AC-13).
- **Real photos leaking into marketing.** All captures use the demo org (FR-13). The live screenshot (AC-19) shows only the user's own picture.
- **Disk growth.** One small file per person ever drawn. Large orgs stay in the low megabytes because of FR-8's 1 MiB cap and typical sizes of a few KB. Pruning is out of scope.
- **Cost drift.** Measured at ≈ 0.001–0.02 TSTU per request on a small org. The FR-5 ceiling holds regardless: higher cost only delays pictures.

## 10. Open questions

- **Q-1** Endpoint and "no picture" detection. **Verified live 2026-09-30** (§5): `identityImage?id=` on the org host works with adoken's PAT; `GraphProfile/MemberAvatars` and Graph answer `401`; placeholders are SVG or PNG without `ETag`. **Still open:** what ADO returns for a real *user* without a picture (not observable in the personal org). **Recommended:** treat it by FR-7's rule (SVG or no `ETag` ⇒ initials) and confirm on the first org that has such a user (read-only). Also record which `size` variant is used: the smallest one ≥ `size.avatar.lg` at 2× (56 px). The default 44 px is enough only up to `size.avatar.md`.
- **Q-2** The sidebar's bottom row shows the org name with an initials circle today. Which picture goes there? **Recommended:** the signed-in user's picture, with the org name as the caption and "{me} · {org}" as the tooltip, because ADO orgs have no picture and the user asked for "the signed-in user". Settings › Account shows the same picture next to the identity.
- **Q-3** Add a global "Show profile pictures" switch? **Recommended:** no. The only avatar preference today is the palette's `avatars` (0003 FR-8). Add a global switch only if beta users ask (screen-share privacy is the likely reason).

## 11. Out of scope

- Uploading or changing a picture (needs a write scope; the web UI does this).
- Pictures for groups, teams, orgs or projects (containers keep initials; repo artwork is `docs/specs/0010-zeron-reuse-batch-2.md` Z-6).
- Animated avatars (GIF shows its first frame).
- Pruning or size limits for the whole avatar cache.
- Multi-org (deferred to Later, `docs/roadmap.md`).
- A global avatars setting (Q-3).

## 12. References

- Graph Avatars — Get: https://learn.microsoft.com/en-us/rest/api/azure/devops/graph/avatars/get?view=azure-devops-rest-7.1
- Identities — Read Identities: https://learn.microsoft.com/en-us/rest/api/azure/devops/ims/identities/read-identities?view=azure-devops-rest-7.1
- Scopes table: https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/oauth?view=azure-devops#available-scopes
- Rate limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
