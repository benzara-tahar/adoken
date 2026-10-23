# 0013 — Website engagement and launch-list analytics

| Field | Value |
| --- | --- |
| Status | Accepted — user approved continuation on 2026-09-30 |
| Milestone | Website, independent of desktop releases |
| Owner | Website analytics |
| Files | `public/src/scripts/analytics.ts`, website analytics helpers/tests, `public/src/copy.ts`, `public/README.md` |
| Serves | Measure interest in the product and improve the launch-list conversion |
| Depends on | Existing PostHog EU cookieless setup and Buttondown launch list |

## Problem and scope

The founder wants to know which website sections visitors read, which interactive examples they use, and whether those visits lead to launch-list subscriptions. Existing tracking only counts pageviews, form submissions, and X-link clicks. This spec concerns adoken.dev; the desktop app continues to send no usage analytics.

## Behavior

1. Preserve cookieless mode, daily anonymous identity, DNT/GPC, no person profiles, no replay or autocapture, the idle-loaded slim SDK, and email URL redaction.
2. Measure section reach once per page visit and foreground active time for the homepage's search/hero, review/features, speed, themes, roadmap, and launch sections. Track only the section occupying the largest visible area, pause on hidden pages or window blur, and stop counting after 30 seconds without visitor activity. Emit bounded summaries on section changes, periodic flush, and page exit; document time as an approximation of attention.
3. Capture explicit theme selection, speed replay, section navigation, design switching, signup form start, OS selection, signup submission, and X-link interaction. Read only known element attributes and allowlisted options; never collect entered email addresses or arbitrary typed values. Existing event names remain compatible.
4. Summarize page depth using fixed 25/50/75/90 percent milestones. Do not emit continuously on scroll.
5. Add a fixed website version property and safe campaign attribution (`ref`, `utm_source`, `utm_medium`, `utm_campaign`), with limited lengths and allowed characters. Buttondown remains the only service receiving subscribers' email addresses.
6. Keep submitted, signup redirect reached, and confirmation redirect reached distinct. Buttondown's confirmed subscriber count is authoritative. `/confirmed/` pageviews are an approximate confirmation signal and can be visited manually. Do not join email identities to PostHog or promise a multi-day confirmation funnel: cookieless identity rotates daily.
7. Configure a PostHog dashboard for production-domain visitors, signup submissions by placement, X clicks, section reach/time, interaction counts, scroll depth, and an in-session visit-to-submit funnel. Only create future-event reports with clear descriptions that data starts after deployment. Filter localhost and test traffic using the existing project rules and production host filter.
8. Improve Buttondown subscriber visibility for status, signup source, placement, and OS where the current plan/UI permits. Preserve existing opt-in, 24-hour reminder, welcome, cleanup, and redirect settings. Do not send campaigns, change subscriber status, purchase upgrades, or enable email open/click tracking.

## Acceptance and verification

- Unit tests use fake capture, fake time, and visibility/activity inputs; never send test events or subscriptions to external services.
- Verify time allocation excludes hidden/blurred/idle periods, does not double count two sections, and flushes without duplicate time.
- Verify that signup field values and email-bearing URL parameters never reach captured properties.
- Verify section/interaction/milestone events use bounded names and properties.
- Build the Astro site and inspect desktop/mobile previews; confirm forms and interactive demos still work.
- Inspect the live PostHog and Buttondown UI to verify saved reports/configuration. Record deployment and live-event verification separately; local implementation alone does not mean it is live.

## Performance and limitations

Reuse existing delegated listeners and section observation; no new SDK, network host, framework, animation, or blocking work before first paint. Summaries are bounded to avoid per-scroll event volume. Engagement is evidence of interest, not proof that a feature is useful in the real app. The existing hero search demonstration is animated and must not be counted as visitor interaction.

## References

- [PostHog cookieless tracking](https://posthog.com/tutorials/cookieless-tracking)
- [PostHog conversion goals](https://posthog.com/docs/web-analytics/conversion-goals)
- [Buttondown metadata](https://docs.buttondown.com/metadata)

## Acceptance

Accepted on 2026-09-30: the user replied “ok continue” after being asked to unlock the computer and accept this spec. Implementation and service configuration may proceed within this scope.

### Accepted amendment — homepage design selector

On 2026-09-30 the user requested removal of the design version switcher from the main website. Remove the switcher from the v2 homepage layout. Preserve the alternate design URLs and their existing functionality. Verify the homepage on desktop and mobile before publishing the focused website change.

### Accepted amendment — optional session replay

On 2026-09-30 the user requested recording configuration and selected “Add optional recording consent.” Preserve the existing cookieless analytics until visitors explicitly allow recording. Offer equally accessible allow/decline choices and a footer control to change the decision. Record only the public homepage, never signup/confirmation notice pages or alternate designs. Honor DNT/GPC even after consent. Mask all inputs, block hidden/file inputs, remove query/hash values from replay URLs, and disable console, network, canvas, and iframe capture. Keep person profiles and autocapture disabled. Record consenting sessions at 100% initially, with a 10-second minimum and the existing 30-day retention. Load the larger recorder only after consent and after first paint; host it with the site so the CSP remains unchanged. Update privacy copy and verify consent, revocation, payload redaction, layout, and production publication.

## Configuration record — 2026-09-30

Verified through the signed-in browser:

- Created [adoken.dev — Engagement & launch list](https://eu.posthog.com/project/284804/dashboard/987295) with a reading guide and 12 saved reports. The description distinguishes website attention, signup attempts, and confirmed subscribers.

| Report | Saved insight |
| --- | --- |
| Daily visitors — production site | [QYdkiQLw](https://eu.posthog.com/project/284804/insights/QYdkiQLw) |
| Signup attempts — form placement | [JfnDyo1e](https://eu.posthog.com/project/284804/insights/JfnDyo1e) |
| X follow clicks — placement | [HLWMmXdp](https://eu.posthog.com/project/284804/insights/HLWMmXdp) |
| Sections reached — measured sessions | [S3if4pMT](https://eu.posthog.com/project/284804/insights/S3if4pMT) |
| Active attention — total seconds by section | [yzIctkO5](https://eu.posthog.com/project/284804/insights/yzIctkO5) |
| Visitor interactions — demos & navigation | [YK3i7Luu](https://eu.posthog.com/project/284804/insights/YK3i7Luu) |
| Reading depth — page visits reaching milestones | [pVTfIbAe](https://eu.posthog.com/project/284804/insights/pVTfIbAe) |
| Visit → launch signup — same session | [hCoVxtVU](https://eu.posthog.com/project/284804/insights/hCoVxtVU) |
| Launch interest — requested operating systems | [vSPsCPwP](https://eu.posthog.com/project/284804/insights/vSPsCPwP) |
| Signup sources — campaign effectiveness | [oN6ItRNp](https://eu.posthog.com/project/284804/insights/oN6ItRNp) |
| Signup & confirmation receipts — approximate | [rXCmFMIz](https://eu.posthog.com/project/284804/insights/rXCmFMIz) |
| Average active seconds — per page visit & section | [xWedxcZp](https://eu.posthog.com/project/284804/insights/xWedxcZp) |

- Trend/funnel reports use a rolling 30-day range, `$host = adoken.dev`, and internal/test filtering. The visit-to-submit funnel aggregates unique sessions with a 30-minute window.
- The SQL average sums increments per `site_design`, `section`, and page-local `page_visit_id` before averaging visits. It includes section reaches with zero recorded time. It restricts the production host and website version, with a fixed rolling 30-day upper window and optional `{filters}`. Production-only filtering excludes the project's existing localhost test rules. Smaller dashboard date overrides can narrow this window; overrides cannot extend it.
- Created [Launch list submission (attempt)](https://eu.posthog.com/project/284804/data-management/actions/162919), matching `launch_list_submitted` with `$host = adoken.dev`. Verified it appears in Web analytics → Goals.
- Saved the Web analytics preset “Production — monthly launch conversion”: production domain, last 30 days, internal/test filtering.
- Buttondown Analytics shows confirmed subscribers, confirmation rate, new subscribers, active subscribers, unsubscribes, and blocked subscribers. Current-plan history is limited to the past month. Email tracking remains opted out.
- Buttondown Subscribers shows built-in Source and metadata `placement` and `source`, alongside status and subscription date. The `os` metadata field was not yet offered in the column picker; the existing forms already submit optional OS values.
- Verified Buttondown public signup, 24-hour confirmation reminder, welcome email, subscriber cleanup, and redirects to `https://adoken.dev/subscribed/` and `https://adoken.dev/confirmed/`. No subscriber status changes, campaigns, or upgrades.
- Verified PostHog cookieless mode is enabled and the production domain is authorized. Its UI notes that IP-based GeoIP/bot enrichment does not run on cookieless events because the raw IP is stripped.

### Implementation and publication

- Added pure measurement/privacy helpers and DOM instrumentation for all three site designs. Updated the privacy page and maintainer README. Preserved the slim SDK, delayed loading, DNT/GPC, and no autocapture/replay.
- Passed 10 unit tests (34 assertions), strict TypeScript checking, Astro production builds, and focused whitespace checks.
- Local browser verification used the actual instrumentation with a fake capture sink and prevented external form submission/navigation. Verified homepage theme/replay and Windows signup events, all four scroll milestones, v3 theme selection/design switching, and actual keystrokes in the v1 search demo. No entered email/search value appeared in event properties, and no browser errors were reported.
- Verified desktop and 390 × 844 mobile layouts; mobile had no horizontal overflow. Proof images are attached to the task.
- After the user completed Cloudflare login, built an isolated snapshot of the tracked website plus only the analytics changes. Published the existing `adoken` static Worker with `--keep-vars --strict`, version `93e3eb25-e903-4ec7-a73d-6d1c2d0b4f39`, tag `engagement-2026-09-30`.
- Production HTTP returned 200. The homepage, analytics asset, and privacy page matched the tested deployment build byte for byte. Unrelated agents' uncommitted work was not included.
- Production Chrome navigation continued to time out, although HTTP verification succeeded. Fresh production engagement events have not yet been verified in PostHog. Empty new-event charts are expected until measured visits arrive; do not treat publication alone as end-to-end event verification.

## Optional replay and homepage publication — 2026-09-30

- Removed the design switcher from the homepage layout. Alternate design URLs remain available.
- Added an optional homepage recording-consent prompt with equally styled allow/decline buttons and a footer Recording settings control. The choice expires after 180 days and contains no visitor identifier. Changing recording mode reloads the page to keep cookieless and replay identities separate. Withdrawal stops capture immediately, clears replay storage, and updates other open tabs.
- The default and all non-homepage routes retain the slim cookieless SDK. Consenting homepage visits load a self-contained recorder after first paint and idle (~196 KB gzip); PostHog persistence uses tab session storage, with no analytics cookies. DNT/GPC override consent. Person profiles, autocapture, surveys, and feature flags remain disabled.
- In [PostHog replay settings](https://eu.posthog.com/project/284804/settings/project-replay), verified recording enabled, 100% default sampling, 10-second minimum, 30-day retention, and normal input masking. Disabled console and network capture; header/body/canvas capture are off. Added a root-only URL trigger and a blocklist for other paths. Verified the public remote configuration returns the saved URL patterns and duration.
- Local masking explicitly masks all inputs, blocks hidden/file inputs, iframes and editable content, removes query/hash/credentials from replay URLs, and masks email-bearing attributes. Privacy copy and maintainer documentation describe both modes.
- Passed 14 tests (64 assertions), strict TypeScript checking, focused whitespace checks, and the isolated Astro production build. Desktop and 390 × 844 mobile previews showed the consent controls without horizontal overflow.
- Exercised the real recorder against a local fake endpoint using the actual analytics source and masking configuration (only host/token and compression changed for inspection). Before consent and after declining there were no snapshots. After consent, received 11 snapshot batches with no synthetic email address or private query parameters. Withdrawing consent returned to cookieless mode and produced zero further snapshot batches. No test subscriptions or production test recordings were sent.
- Published only the focused website snapshot, preserving unrelated agent work and existing Worker variables: Cloudflare version `d0507358-a4e7-4883-b26f-87650084eab5`, tag `replay-consent-2026-09-30`.
- Production homepage, privacy page, consent script, analytics loader, and recorder asset returned success and matched the tested build byte for byte. Production Chrome navigation still timed out; the first live PostHog recording has not been verified. Recordings require an actual consenting homepage visit of sufficient duration.

## Event contract


| Event | Properties | Purpose |
| --- | --- | --- |
| `section_viewed` | `section`, `site_version` | Reach: one event per section per page visit |
| `section_engaged` | `section`, numeric `active_seconds`, `site_version` | Foreground active-time increments; report summed time per section and avoid treating each flush as a new visitor |
| `site_interaction` | allowlisted `interaction`, `target`, `section`, `site_version` | Theme selection, replay, navigation, design switching |
| `scroll_depth_reached` | numeric `percent` in 25/50/75/90, `site_version` | Depth: once per milestone per page visit |
| `launch_list_started` | `placement`, `site_version` | First form focus; never the field value |
| `launch_list_os_selected` | allowlisted `os`, `placement`, `site_version` | Intended work platform |
| `launch_list_submitted` | `placement`, optional allowlisted `os`, safe source/campaign fields, `site_version` | Existing submission-attempt event, extended compatibly |
| `follow_x_clicked` | `placement`, `site_version` | Existing secondary conversion event |

Use `section_viewed` unique sessions for reach; sum `active_seconds` for attention. For average section time, aggregate increments by page-local `page_visit_id` and section before averaging visits, rather than averaging periodic flush events. A visit-to-submit funnel must be bounded to one session; email confirmation belongs in separate reporting backed by Buttondown.

Every new event also carries `site_design`, `site_version`, safe source/campaign labels, and the in-memory `page_visit_id`. It is not a persistent visitor identity.
