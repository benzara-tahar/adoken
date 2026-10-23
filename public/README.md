# adoken.dev

The site for adoken. The homepage (`/`, "Instant") and two alternative designs, `/v1/` ("Arcade Dojo")
and `/v3/` ("Ink & Iron"), the macOS download (GitHub Releases) and the release-notes list. Every product visual
is a hand-built HTML/CSS/TS mock on fictional data, not an app screenshot. Astro, static output, deployed to Cloudflare. Client JS is vanilla TS bundled per page
(mock animations, cookieless analytics loaded on idle, and a tiny script that tags launch-list signups with their source).


## Commands

Run from `public/`:

| Command          | What it does                                                                       |
| :--------------- | :--------------------------------------------------------------------------------- |
| `bun install`    | Install dependencies                                                               |
| `bun run dev`    | Dev server at `localhost:4321`                                                     |
| `bun run build`  | Production build to `./dist/`                                                      |
| `bun run preview`| Serve `./dist/` locally                                                            |
| `bun run assets` | Re-render `static/favicon.svg`, `favicon.ico`, `apple-touch-icon.png`, `og.png`    |

Astro runs on the Bun runtime (`bunx --bun astro …`), so a local Node older than 22 is fine.

## Where things live

| Path                          | Purpose                                                                   |
| :---------------------------- | :------------------------------------------------------------------------ |
| `src/copy.ts`                 | Shared copy: `meta`, `og` (OG image), `repo`, `download`, `launchList`, footer legal line, and the privacy/404/subscribed/confirmed pages. |
| `src/v1/`, `src/v2/`, `src/v3/` | One design each: `Layout.astro`, `copy.ts` (every word and all fake mock data), components, scripts, styles. |
| `src/pages/index.astro`       | Homepage, built from `src/v2/`. `v1/` and `v3/` are the alternative designs (`noindex`). `privacy.astro` is the privacy page. `404`, `subscribed`, `confirmed` use `layouts/Notice.astro`. |
| `src/theme.ts`                | Color tokens for `layouts/Base.astro` pages, the art and the OG image.    |
| `src/art/fist.ts`             | The original pixel-art fist (grid → SVG rects).                           |
| `src/components/`             | Fist and follow-on-X button used by the privacy and notice pages; `Download.astro`, the download CTA all three designs share. |
| `src/integrations.ts`         | PostHog token + proxy host, Buttondown form URL. Public values only.      |
| `src/scripts/analytics.ts`    | Cookieless PostHog loader, production guard and privacy filtering.       |
| `src/scripts/instrumentation.ts` | Explicit section, demo, navigation and launch-form event wiring.    |
| `src/scripts/engagement.ts`   | Pure attention, scroll-depth, attribution and privacy helpers.          |
| `scripts/render-assets.ts`    | Renders favicon/OG PNGs from the same art, tokens and copy.               |
| `static/`                     | Static files copied as-is to `dist/` (see below).                         |

**Static dir:** this Astro project's root is the repo's `public/` folder, so Astro's default static dir
would be `public/public/`. It's configured as `publicDir: './static'` in `astro.config.mjs` instead.

**Editing copy:** homepage words and mock data live in `src/v2/copy.ts` (`src/v1/copy.ts`, `src/v3/copy.ts` for the
alternatives); shared bits in `src/copy.ts`. Run `bun run build`. If you touched `meta` or `og`, also run
`bun run assets` so the OG image matches, then commit the regenerated PNGs. The OG image is rasterized with system
fonts (Helvetica Neue, Menlo, Hiragino Sans), so render it on macOS.

**Design switcher:** only the alternative designs have a small pill linking `/v1/`, `/` (v2) and `/v3/` (`switcher` in each
`copy.ts`). Old `/v2/` links redirect to `/` (`static/_redirects`).

## Cloudflare deploy

Either flow works; `wrangler.jsonc` (static-assets Worker named `adoken`) is used by the Workers flow and ignored by Pages.

### Workers flow (Workers & Pages → Create → Import a repository)

| Setting          | Value                                              |
| :--------------- | :------------------------------------------------- |
| Root directory   | `public`                                           |
| Build command    | `bun install --frozen-lockfile && bun run build`   |
| Deploy command   | `bunx --bun wrangler deploy`                       |
| Non-production branch deploy command | `bunx --bun wrangler versions upload` |
| Production branch | `main`                                            |

The Worker name in the dashboard must match `name` in `wrangler.jsonc` (`adoken`). Same env vars as below.
Custom domain: Worker → Settings → Domains & Routes → Add → Custom domain → `adoken.dev`.

### Pages flow (Workers & Pages → Create → Pages → Connect to Git)

| Setting                | Value                                              |
| :--------------------- | :------------------------------------------------- |
| Framework preset       | Astro                                              |
| Root directory         | `public`                                           |
| Build command          | `bun install --frozen-lockfile && bun run build`   |
| Build output directory | `dist`                                             |
| Production branch      | `main`                                             |
| Build system version   | v3                                                 |

Environment variables (Production and Preview):

| Variable       | Value    |
| :------------- | :------- |
| `BUN_VERSION`  | `1.4.0`  |
| `NODE_VERSION` | `22`     |

Then Custom domains → add `adoken.dev` (the zone is already on Cloudflare, so DNS is set up automatically).
Optionally add `www.adoken.dev` and a Bulk Redirect `www` → apex.

`static/_headers` sets security headers (CSP, nosniff, frame-deny) on every response. `404.html` is served
for unknown paths automatically by Pages.

## Analytics and launch list

**PostHog** (EU Cloud, org/project `adoken` / `adoken.dev`, id 284804):

- Cookieless by default: "Cookieless tracking" is on in Project settings → Web analytics. Until homepage recording
  consent, the SDK runs with `cookieless_mode: 'always'` and no analytics storage. `person_profiles: 'never'` and
  DNT/GPC apply in both modes. The separate consent preference contains only the decision and a 180-day expiry.
  Turn off the project setting and cookieless events are dropped.
- Slim SDK (`posthog-js/dist/module.slim`, ~49 KB gzip) by default, imported dynamically once the page is idle, production
  builds only. A separate self-contained replay SDK is fetched only after explicit homepage recording consent. No autocapture, surveys or feature flags; `disable_external_dependency_loading` keeps
  `script-src 'self'` intact.
- Optional session replay: the homepage offers equally accessible allow/decline buttons and a footer Recording
  settings control. Allowing recording switches to `module.full.no-external` (~196 KB gzip, delayed until consent
  and idle), anonymous session-storage persistence, and the project's remote JSON replay config. Choosing a
  different mode reloads the page to separate identities; withdrawal immediately stops capture and clears this
  site's replay storage, including in other open tabs. No analytics cookies. Other routes remain cookieless.
- Replay covers only `/`; a matching URL trigger and a blocklist for other paths provide a second guard. Current
  project settings: 100% of consenting visits, 10-second minimum, 30-day retention, normal input masking. Local
  masking also blocks hidden/file inputs, iframes and editable content, strips query/hash/credentials from replay
  URLs, and masks email-bearing attributes. Console, network, payload, canvas and iframe capture are disabled.
- Production domain only: `analytics.ts` never loads the SDK on local or preview hosts, or when DNT/GPC is enabled.
- Explicit events: `launch_list_started`, `launch_list_os_selected`, `launch_list_submitted`, `follow_x_clicked`,
  `download_click` (the macOS DMG link), `github_clicked` (repo and releases links),
  `site_interaction` (theme/replay/navigation/design choices and visitor input in the v1 search demo),
  `section_viewed`, `section_engaged`, and `scroll_depth_reached` (25/50/75/90%). Animated examples do not count as input.
- `instrumentation.ts` uses existing public element attributes and fixed section maps for all three designs.
  `engagement.ts` allocates time to the section with the largest visible area. Hidden/blurred pages and time past
  the 30-second idle cutoff are excluded. It flushes increments every 15 seconds, on section changes after at
  least one second, and on exit. Tiny section visits are retained until a later flush. SDK loading is still after
  first paint and idle, so very early exits can go unmeasured.
- Common properties: `site_version`, `site_design`, `recording_allowed`, safe source/campaign labels, and a random `page_visit_id` that
  exists only in memory for this page. Aggregate section increments by that ID before averaging visits;
  averaging individual flushes would understate attention. This measures website interest, not app usefulness.
- `before_send` keeps only safe public campaign query parameters in URL properties. Buttondown redirects can
  contain email addresses; email, credentials, tokens, and unsafe campaign values are stripped before analytics.
- [Engagement & launch-list dashboard](https://eu.posthog.com/project/284804/dashboard/987295).
  Buttondown confirmed subscribers are authoritative; `/confirmed/` visits are an approximate signal.
  Cookieless identity rotates daily, so do not use multi-day email-confirmation funnels or retention as exact people.
- Web analytics has a `Launch list submission (attempt)` conversion action and a saved
  `Production — monthly launch conversion` preset. The dashboard includes a reading guide and 12 reports;
  the average-time SQL sums increments per page visit before averaging. Publication and verification are
  recorded in `docs/specs/0013-website-analytics.md` at the repository root.
- Tests: `bun test src/scripts` (engagement, recording and platform detection; fake capture/time/storage, no real service calls).
- Managed reverse proxy: `e.adoken.dev`, a DNS-only CNAME in Cloudflare → PostHog EU (Organization settings →
  Managed reverse proxy). Keep it grey-clouded.
- "Filter out internal and test users" excludes `$host` localhost/127.0.0.1 (on by default in insights).

**Buttondown** (newsletter `adoken`, <https://buttondown.com/adoken>):

- Each design's `components/LaunchForm.astro` is a plain `<form>` POST to `embed-subscribe/adoken`, so it works without JS. Buttondown may show
  a Turnstile check, then double opt-in. Redirects (Settings → Subscribing): `/subscribed/` after submit,
  `/confirmed/` after the confirm link.
- Subscriber metadata: `os` (optional radio), `placement` (`v{1,2,3}-hero` / `v{1,2,3}-footer`; v2 is the homepage), `source` (`?ref=`, else
  `utm_source`, else the referring host, else `direct`).
- Firewall: aggressive filtering; suspicious signups are accepted and marked Blocked (review them) rather than
  rejected, so corporate VPN users aren't silently dropped.
- Analytics displays confirmed/new/active subscribers, confirmation rate, unsubscribes, and blocked subscribers.
  The Subscribers table shows Source, metadata placement, and metadata source. Add an OS column when the UI
  offers that metadata field. Existing double opt-in, the 24-hour reminder, welcome, cleanup and redirects remain enabled.

Tag every link to the site in posts with `?ref=<week>-<slot>` (see `marketing/calendar.md`). `/privacy/`
(`privacy` in `src/copy.ts`) describes this exact setup; change it whenever the setup changes. New hosts
must be added to both `src/integrations.ts` and the CSP in `static/_headers`. Astro must never inline
scripts (`vite.build.assetsInlineLimit: 0`), because the CSP has no hashes.

## Rules of the ring

- Dark-first (v3 is the light exception), system fonts only, no web fonts. Client JS is vanilla TS, bundled per
  page, no frameworks. The only third-party hosts are `e.adoken.dev` (PostHog proxy) and `buttondown.com` (form POST).
  The analytics SDK loads only after first paint and idle (`src/scripts/analytics.ts`); loading it earlier costs
  ~1.3 s of mobile LCP. Keep it there.
- Every animation sits behind `prefers-reduced-motion: no-preference`, pauses off screen, and never hides content
  in its first frame: entrances move content, they don't fade it in from invisible, so a throttled or frozen
  animation clock still shows a finished page.
- Original art only. No Capcom / Street Fighter names, sprites, poses or audio.
- Primary CTA: Download for macOS (`download.url`, the stable `releases/latest/download/adoken-macos-arm64.dmg`
  asset), with the build's limits stated next to it. Secondary: Star on GitHub, the release-notes list (one email per
  release) and follow @BenzyPrime on X. Nothing is gated behind the list.
