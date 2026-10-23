# Today — "my day"

Status: v0.1, **Accepted** with spec 0017 (2026-10-01, owner: all recommendations; loose-end age label overridden to "Open {N}d", spec Q-2); replaces the 2026-09-30 design (ribbon, Focus hero, card grid, track lines). Owner: ux-designer. Behavior and acceptance: [`docs/specs/0017-today-redesign.md`](../specs/0017-today-redesign.md); nudge logic (Focus choice, waiting order, blockers, triage): [`docs/specs/0006-smart-nudges.md`](../specs/0006-smart-nudges.md). Tokens: [tokens.md](tokens.md). States: [principles.md](principles.md) §3. Layout reference (arrangement only): `output/today-redesign-2026-10-01/references/today-layout.png`.

The owner on the previous Today: "the progress bar displayed for the PRs is not pretty at all… the PR title is not displayed entirely… It's very thin… it's just text, numbers, statuses, and progress bars." Today now answers, top to bottom and left to right: **how is my day going** (a sentence, a few numbers, a picture), **what changed on my PRs**, and **what do I do next**.

## 1. Concept

- **Day summary**: one card. Left: a headline, one honest sentence, activity counts. Right: the mood illustration. Same size and arrangement in every mood; only the words, numbers and picture change.
- **Your PRs**: my open PRs and today's merges, most recently active first. Full title, then a compact segmented stage bar, then what happened today.
- **Up next**: the single next action (the old Focus, same choice).
- **Waiting on you**: reviews I owe, with how long they've waited.
- **Loose ends**: my in-progress work items a nudge flags, with how long they've been in their current state.

Nudges are still not a list: they pick Up next, set blockers, and attach to their subject's row, so `s` / `d` / `x` work on any row.

## 2. Layout

Two columns when Today's width is ≥ 960 px; one column below. Content max width 1280, centered. Page padding `space.24` horizontal, `space.16` vertical. One vertical scroll for the whole page.

```
┌───────────────────────────────────────────────────────────────────────────────────────┐
│ Today                                                    Updated 18m ago · Refresh ⌘R │ header (label only when stale)
│ Thursday, October 1                                                                   │
│ ┌───────────────────────────────────────────────────────────────────────────────────┐ │
│ │ Something shipped                                          ┌────────────────────┐ │ │
│ │ 1 PR merged today · 1 conflict to resolve                  │                    │ │ │ day summary
│ │                                                            │   illustration     │ │ │ 176 high
│ │ 1        │ 3        │ 5                                    │   264 × 176        │ │ │
│ │ merged   │ reviews  │ commits                              └────────────────────┘ │ │
│ └───────────────────────────────────────────────────────────────────────────────────┘ │
│ ┌─────────────────────────────────────────────────────┐ ┌───────────────────────────┐ │
│ │ Your PRs  5                                         │ │ Up next                   │ │
│ │ ┌─────────────────────────────────────────────────┐ │ │ (MK) Mara is blocked on   │ │
│ │ │ Fix session timeout on reconnect   Updated 3m ago│ │ │      you                  │ │
│ │ │ ▬▬ ▬▬ ▭▭ ▭▭  ⎇ In review              (JD)(MS)   │ │ │ !4821 Fix login throttle  │ │
│ │ │ !66231 · 2 commits today · 1 new comment         │ │ │ ▤ 3 · 2d                  │ │
│ │ └─────────────────────────────────────────────────┘ │ │ [ Review now          ↩ ] │ │
│ │ ┌─────────────────────────────────────────────────┐ │ │ s Snooze · d Done · e Hide│ │
│ │ │ Add keyboard shortcuts to PR review Updated 12m │ │ └───────────────────────────┘ │
│ │ │ ▬▬ ▬▬ ▬▬ ▬▬  ✦ Merged today           (AK)(LB)   │ │ ┌───────────────────────────┐ │
│ │ │ !66218 · Merged today at 14:32                   │ │ │ Waiting on you  2         │ │
│ │ └─────────────────────────────────────────────────┘ │ │ Review offline retry (AK)2h│ │
│ │ ┌─────────────────────────────────────────────────┐ │ │ Review project fil…  (LB)1d│ │
│ │ │ Improve supplier search            Updated 28m  │ │ └───────────────────────────┘ │
│ │ │ ▬▬ ▬▬ ▭▭ ▭▭  ◷ Changes requested      (PR)(TC)   │ │ ┌───────────────────────────┐ │
│ │ │ !66197 · 1 commit today · 2 new comments         │ │ │ Loose ends  2             │ │
│ │ └─────────────────────────────────────────────────┘ │ │ ☑ Update release no…    Open 3d │
│ └─────────────────────────────────────────────────────┘ │ ☑ Close migration t…    Open 6d │
│                                                         └───────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

- Columns: Your PRs flexes (min 520); the right column is 320 wide; gap `space.16` between columns and between panels.
- **Narrow (< 960 px)**: one column, full width: summary, Up next, Your PRs, Waiting on you, Loose ends. Up next comes before the PRs because it is the one action.
- Panels: `bg.raised`, `radius.md`, 1 px `border.default` (tokens: cards), padding `space.16`. Panel header: title `type.heading` `text.primary`, count in a pill (`type.label`, `text.secondary` on `bg.hover`, `radius.full`), `space.12` below the header.
- Header: "Today" `type.title`; the local long date ("Thursday, October 1") `type.small` `text.secondary`. Right: the `StalenessLabel` only when data is stale (spec FR-10). No sync label, no nudge counter, no footer.

## 3. Day summary card

### 3.1 Anatomy

- Card: `bg.raised`, `radius.md`, 1 px `border.default`, full content width, height 176 at `font_scale` 1.0 (grows only if the text column needs more at a larger `font_scale`; never between moods). `overflow: hidden` so the art clips to the radius.
- Text column: left padding `space.24`, top and bottom `space.20`, right edge = the art box. Vertically: headline, sentence `space.4` below it, counts row `space.16` below that.
  - Headline: `type.display` (28 / 34, 650) `text.primary`, one line, no trailing period.
  - Sentence: `type.body` `text.secondary`, one line; overflow ellipsis with the full sentence in the tooltip and the region label.
  - Counts row (fixed height): slots of a number (`type.title` with `type.numeric` tabular figures, `text.primary`) over a label (`type.small`, `text.secondary`). Slots are separated by a 1 px `border.subtle` rule with `space.24` either side. Zero slots draw number and label in `text.tertiary` (quiet, still 4.5:1; `text.disabled` is for disabled controls only). First sync draws "–" in `text.tertiary`.
- Art box: 264 × 176, flush to the card's top, right and bottom edges; the illustration is fitted inside it with `ObjectFit::Contain`, right-aligned. When the card is narrower than 640 px, the box is 198 × 132, flush right and vertically centered. Text never overlaps the box.
- Nothing on the card is interactive; it is not in the keyboard selection.

### 3.2 Headlines and sentences

| Mood | Art | Headline | Typical sentence |
| --- | --- | --- | --- |
| `fresh-start` | `01-fresh-start` | A fresh start | "2 reviews wait on you" / "Nothing needs you yet" |
| `finding-flow` | `02-finding-flow` | Finding your flow | "5 commits pushed today · 1 review waits on you" |
| `helping-grow` | `03-helping-grow` | Helping things grow | "3 reviews done today" / "2 discussions today" |
| `waiting` | `04-waiting` | Waiting to bloom | "2 PRs waiting on reviewers" |
| `blocked` | `05-blocked` | Blocked | "1 conflict to resolve · 1 failed build to fix" |
| `shipped` | `06-shipped` | Something shipped | "2 PRs merged today" / "1 PR merged today · 1 conflict to resolve" |
| `clear` | `07-clear` | Clear for now | "Nothing needs your attention right now" |
| `stalled` | `08-stalled` | Nothing moved today | "No reviews, commits or merges yet today · 2 reviews wait on you" |
| first sync (fresh-start art) | `01-fresh-start` | Getting your day ready | "First sync in progress · counts appear when it finishes" |
| stale (waiting art) | `04-waiting` | Can't see the latest | "Offline · counts are from 14:02" / "Sync delayed · counts are from 14:02" |

Composition rules (lead, attention parts, at most three parts, " · " separator, plurals) are spec FR-11; strings live in `crates/nudge/src/copy.rs`. Voice: no blame ("yet", never "you didn't"), no exclamation marks, numbers as digits, "PR" for pull request (principles §5, tight space). Waiting is neutral: no warning or danger tone anywhere on the card in any mood; the card never uses status colors at all, the picture carries the mood.

### 3.3 Counts row

Fixed order: **merged · reviews · discussions · commits · done**. Every non-zero metric shows; zeros from merged, reviews, commits fill up to three slots. Singular labels at 1 ("1 review"). "N+" when a commit count is a lower bound. A slot whose scope is missing (done without `vso.work`) is not drawn. Tooltip per slot names the source: "Your pull requests merged today", "Pull requests you voted on today", "Discussions you joined or that were resolved today", "Commits pushed to your pull requests today", "Your work items completed today".

### 3.4 Illustrations

- **Set.** Eight owner-generated originals, one per mood: `output/today-redesign-2026-10-01/images/01-fresh-start.png` … `08-stalled.png` (1536 × 1024, transparent, no text, no counters, no UI; checksums in `assets.json`). Distinct scenes, silhouettes and dominant colors: sprout and sunrise in pale peach; diagonal teal stream; honey-gold lanterns and bridges; paper boat and misty lavender bridge; broken crossing in dark red and charcoal; flowering tree and emerald wave in green and pink; tea and moon in indigo; dry stream and bare branches in orange and terracotta.
- **Shipped form.** Already embedded as WebP at 528 × 352 (2× the box): `assets/images/today/*.webp`, painted with `gpui::img(adoken_ui::images::TODAY_*)`, which decodes off the UI thread; not re-added or re-encoded. Each ≤ 350 KB, ≤ 2.5 MB together (0.59 MB measured). Listed in `assets/manifest.toml` (`kind = "illustration"`, `ship = true`, provenance: generated by the owner from `image-generation-prompts.txt`). Art carries no text, so nothing needs localizing.
- **Dark and light themes.** The art is painted as is on `bg.raised`: no tint, recolor, opacity, blur, frame, shadow or per-theme variant. It was checked composited on a Dojo Night card (`#1B1D24`) and a white card on 2026-10-01: all eight are clean cutouts (semi-transparent pixels are 1–13 % of the canvas, on the scene edges; no glow halos), the dark scenes (blocked, clear) read as vignettes on light cards and the pale ones (fresh start, waiting) stay legible on dark cards. With a custom background the card keeps `bg.raised` at `surface_opacity` (tokens §10.2) and the art sits on it at full opacity. Arcade and Sakura need nothing extra. If a future illustration fails a theme, the art is regenerated; themes never style it.
- **Loading.** Decoded off the UI thread the first time a mood needs it. Until ready the box shows the card surface only (no skeleton, no spinner). At most two decoded at a time.
- **Change.** When the mood changes while Today is on screen, the new picture crossfades over the old at `motion.base` (`ease.in_out`, tokens §7.2 crossfades). Reduce Motion: instant swap. Opening Today with a different mood than last time: no crossfade, the picture is simply there.
- **Accessibility.** Decorative: no role, no label. The headline says the mood.

## 4. Your PRs

### 4.1 Row

Each PR is a sub-card inside the panel: `bg.surface`, `radius.sm`, 1 px `border.subtle`, padding `space.12`, `space.8` between rows. Selected: `bg.selected` and the focus ring (principles §4).

- **Line 1**: title `type.body_strong` `text.primary`, wrapping, up to 3 lines then ellipsis (full title in tooltip and label). Right: "Updated 3m ago" `type.small` `text.tertiary`, top-aligned, never wraps.
- **Line 2** (`space.8` below): stage bar, `space.8`, state glyph (`size.icon.sm`) and label (`type.small`, weight 560, in the state's tone). Right: reviewer faces.
- **Line 3** (`space.6` below): `!id` (`type.code_inline`, `text.tertiary`) · activity (`type.small`, `text.secondary`).
- Parked row (snoozed / done / muted): `opacity.dimmed` with its glyph (`alarm-clock` / `check` / `bell-off`, `size.icon.xs`, `text.tertiary`) before "Updated".

### 4.2 Stage bar

Four segments, Draft · Review · Approved · Merged: each 44 × 6, `radius.full`, gap `space.4` (188 wide). Filled segments use the state tone; empty ones `border.strong` (reads at a glance on every surface). No dots, circles, hairlines or faces on the bar.

| State | Filled | Tone | Glyph | Label |
| --- | --- | --- | --- | --- |
| Merged today | 4 | `status.success.fg` | `sparkles` | Merged today |
| Merge conflict | by stage | `status.danger.fg` | `git-merge` | Merge conflict |
| Build failed | by stage | `pipeline.failed` | `circle-x` | Build failed |
| Changes requested | 2 | `vote.waiting` | `clock` | Changes requested |
| Waiting on reviewers (N4 due) | 2 | `text.secondary` | `users` | No reviews · 2d |
| Approved | 3 | `vote.approved` | `badge-check` | Approved |
| In review | 2 | `accent.fg` | `git-pull-request` | In review |
| Draft | 1 | `pr.draft` | `git-pull-request-draft` | Draft |

Tones are progress: the accent while in review, amber when it needs me, red when it's stuck, green once approved or merged. Waiting on reviewers stays neutral. The label and glyph always say the state (color never alone).

### 4.3 Merged today

Only PRs completed today: the sub-card fills with `status.success.wash`; the label row shows the static `sparkles` glyph (Lucide, 14 px, `status.success.fg`, the same glyph spec 0022 uses as its Reduce Motion fallback) and "Merged today"; line 3 reads "!66218 · Merged today at 14:32". No animation, no confetti, no sound (effects belong to spec 0022).

### 4.4 Reviewer faces

Up to three faces (`size.avatar.md`, `space.4` apart), required reviewers first, then "+N" (`type.label`, `text.tertiary`). A non-zero vote adds a 12 px vote glyph badge (tokens §2.8 `vote.*` glyph and color) on a `bg.raised` disc at the face's bottom-right; no-vote faces are `opacity.dimmed`. Group reviewers (`Person.is_group`) are hidden by default (spec 0019 `[review] show_group_reviewers`); shown, they come after the people as initials (principles §4: groups keep initials). Tooltip per face: "Kim Becker · Approved" / "Required · No vote yet".

### 4.5 Activity line copy

| Situation | Line 3 |
| --- | --- |
| Merged today | `!66218 · Merged today at 14:32` |
| Today's activity | `!66231 · Opened today · 2 commits today · 1 new comment` (parts that apply, in this order) |
| Commit count is a lower bound | `3+ commits today` |
| Push seen, commit list not synced yet | `New commits today` |
| Commit count unknown (synced before the change) | `1 push today` |
| Nothing today, blocked | `Conflict for 5h` · `Build failed 2h ago` · `Changes requested by Kim · 1d` · `Waiting on Kim (required), Sam · 2d` |
| Nothing today, approved | `All reviewers approved` |
| Nothing today | `No activity today` |

"New comment" counts others' comments only; mine aren't news.

## 5. Right column

### 5.1 Up next

The old Focus, same choice (0006 FR-10), in a panel titled "Up next".

- Top: the face (`size.avatar.lg`) of the author blocked on me (N1) or the reviewer who asked for changes (N2), else the nudge's glyph (`size.icon.md`, its tone); beside it the headline `type.heading` ("Mara is blocked on you", "Kim asked for changes", "Your build failed", "Your PR has a conflict", "No one has reviewed yet", "Merged, still open", "Still on it?").
- `!4821 Fix login throttle` `type.body_strong`, up to 2 lines.
- Meta `type.small` `text.secondary`: changed files (`file-diff`) when synced, today's activity when there is some ("2 new comments"), the age in its heat tone.
- Button, full panel width, `size.control.md`: the nudge's action with an `↩` keycap ("Review now", "Open", "Open failing run", "Open in ADO", "Still on it?"). Primary when due, secondary when it's the calm not-yet-due review.
- Hints `type.small` `text.tertiary`: `s` Snooze · `d` Done · `e` Hide, with keycaps.
- Clear: a `check` glyph in `status.success.fg` and "Nothing waits on you" `type.body` `text.secondary`.
- First sync: "Up next appears after the first full sync".
- Hidden by `e`, before working hours, or on a non-workday: the panel is not drawn; Waiting on you moves up.

### 5.2 Waiting on you

One row per review I owe (0006 FR-11 order; parked last, dimmed with their glyph). Row height `size.row.*` by density: title `type.body` `text.primary`, one line with ellipsis; the author's face `size.avatar.md`; a `shield` (`size.icon.xs`, `text.tertiary`) when I'm required; the age right-aligned, `type.small` with tabular figures, in its heat tone:

| Heat | When (0006 FR-11) | Token |
|---|---|---|
| Cool | under the N1 threshold | `text.tertiary` |
| Warm | 1× – 3× the threshold | `status.warning.fg` |
| Hot | ≥ 3× | `status.danger.fg` |

The age ring is gone: at row size it was unreadable, and the number already is the signal. Empty: "No reviews waiting on you" `type.small` `text.tertiary`.

### 5.3 Loose ends

One row per flagged work item (N6, due N7): type glyph in its `wi.*` color (`size.icon.sm`), title `type.body` one line, and an age pill (`type.label`, `text.secondary` on `bg.hover`, `radius.full`, padding `space.2` / `space.8`) reading "Open 3d": whole days in the item's current state (spec FR-22: from `state_since`, the state change date; the changed date when unknown). No glyph in the pill. Tooltip: the state and the nudge's reason, "Active for 3 days · PR !901 merged 2 days ago" (N6) / "Active for 8 days · no change for 8 days" (N7). Empty: "No loose ends".

## 6. States

| State | Shows |
|---|---|
| Loading (first open) | After 150 ms: card frame with two `bg.hover` text bars and an empty art box; three PR row bars; three right-column bars. Shimmer off under Reduce Motion. |
| No account yet | `EmptyState` "Today fills in after the first sync". |
| First sync | Card: fresh-start art, "Getting your day ready", "First sync in progress · counts appear when it finishes", counts "–". Synced rows show; Up next: "Up next appears after the first full sync". |
| Busy / mixed day | Everything; the card's picture follows the best thing that happened, the sentence names the blockers too. |
| Clear | Card "Clear for now"; Up next "Nothing waits on you". |
| Up next hidden / before hours / weekend | No Up next panel. |
| Empty org | Card as computed; Your PRs: "No open pull requests" · "Your pull requests show here while they're open and on the day they merge."; right panels' one-line empties. |
| Error | `Banner` "Couldn't load Today. {reason}. [Retry]"; last good content below if any. |
| Offline / stale | Header `StalenessLabel`; card "Can't see the latest" on the waiting art when it would otherwise claim clear or nothing moved. |

## 7. Keyboard and accessibility

One selection across the page. All keys are in ⌘P and the `?` cheat sheet.

| Key | Two columns | One column |
|---|---|---|
| `↓` `↑` / `j` `k` | Next / previous row in the same column (right column: Up next, Waiting rows, Loose ends rows) | Next / previous row |
| `→` `←` / `l` `h` | Other column, the row nearest vertically | Same as `↓` `↑` |
| `Home` / `End` | First / last row of the page | same |
| `↩` | Up next: run its action. Rows: open the PR review, work item, or failing run | same |
| `e` | Hide Up next until tomorrow | same |
| `s` / `d` / `x` | Snooze until next workday / Done until it changes / Not for this item, on the selected row's nudge | same |
| `c` | Copy reviewer names (N4 row) | same |
| `⌘D` | Pin the selected row's subject (spec 0014) | same |
| ⌘P only | Nudge: Turn off this nudge type · Today: Refresh | same |

Spoken labels (AccessKit, principles §6):

- Card region "Day summary": "Something shipped. 1 PR merged today, 1 conflict to resolve. 1 merged, 3 reviews, 5 commits."
- PR row: "Pull request !66231 Fix session timeout on reconnect. In review, stage 2 of 4. 2 commits today, 1 new comment. Updated 3 minutes ago. Reviewers: Jane Doe approved, Mike Smith no vote."
- Merged row: "Pull request !66218 Add keyboard shortcuts to PR review. Merged today at 14:32. Reviewers: …"
- Up next: "Up next: Mara is blocked on you. !4821 Fix login throttle, 3 files, waiting 2 days. Return: Review now."
- Waiting row: "Review !4347 Fix Terraform state locking from Kim Becker, waiting 18 days, required reviewer."
- Loose end: "Task #48213 Fix SSO redirect, Active for 3 days. Its pull request !901 merged 2 days ago."
- Parked rows append "snoozed until Monday" / "marked done" / "muted".
- Sections are lists labeled "Your pull requests, 5", "Waiting on you, 2", "Loose ends, 2". Mood changes are not announced.

Color is never alone: stage states have a label and glyph, heat has the number, votes have badges, merged-today has its label.

## 8. Motion and sound

- The only motion: the illustration crossfade on a mood change while visible (`motion.base`). Reduce Motion swaps it instantly. Selection, sorting and row changes are instant.
- No sounds on Today.

## 9. Gallery

Stories in `crates/gallery/src/today.rs` (fixed clock, fixtures), one snapshot per built-in theme (Dojo Night, Dojo Day, Arcade, Sakura). Wide stories 1180 × 900 (two columns); `today-narrow` 720 × 1300.

`today-mood-fresh-start`, `today-mood-finding-flow`, `today-mood-helping-grow`, `today-mood-waiting`, `today-mood-blocked`, `today-mood-shipped`, `today-mood-clear`, `today-mood-stalled`, `today-busy-day` (mixed day: shipped art, "1 PR merged today · 1 conflict to resolve", every panel filled, one parked row), `today-first-sync`, `today-stale`, `today-loading`, `today-no-account`, `today-empty-org`, `today-error`, `today-narrow`, `today-up-next-hidden`, `today-group-reviewers-shown`, `today-long-titles` (a 2-line and a 3-line-plus title).

Removed with their snapshots: `today-focus-hidden`, `today-clear-day`, `today-new-user`.

## 10. Tokens and assets used

- Surfaces: `bg.raised` (card, panels), `bg.surface` (PR sub-cards), `bg.selected`, `bg.hover` (pills, skeleton bars), `status.success.wash` (merged today).
- Borders: `border.default` (card, panels), `border.subtle` (PR sub-cards, count rules), `border.strong` (empty segments), `border.focus` (selection).
- Text: `text.primary`, `text.secondary`, `text.tertiary` (also zero counts).
- Tones: `accent.fg`, `pr.draft`, `vote.approved`, `vote.waiting`, `pipeline.failed`, `status.danger.fg`, `status.warning.fg`, `status.success.fg`, `wi.*`.
- Type: `type.display`, `type.title`, `type.heading`, `type.body`, `type.body_strong`, `type.small`, `type.label`, `type.code_inline`, `type.numeric`.
- Sizes: `size.avatar.md` / `lg`, `size.icon.xs` / `sm` / `md`, `size.control.md`, `size.row.*`, `radius.sm` / `md` / `full`, `space.2`–`space.24`, `opacity.dimmed`.
- Motion: `motion.base`, `ease.in_out`.
- New Lucide glyph: `sparkles` (Lucide 1.48.0, ISC, `assets/icons/lucide`). Every other glyph is already compiled in (`crates/ui/src/icon.rs`).
- New assets: the eight mood illustrations (§3.4).

## 11. What changed from the 2026-09-30 design

| Before | Now | Why |
|---|---|---|
| Ribbon timeline with event dots and counts | Day-summary card with counts, sentence, mood art | Says how the day is going in words; one summary instead of two (spec Q-5) |
| Focus hero card | Up next panel (same choice) | Same action, less space; the card above is the hero now |
| Waiting cards, 3 per row, faces in age rings | One-line rows, age in its heat tone | Rings were unreadable small; rows scan faster (spec Q-6) |
| Track lines: truncated title, thin hairline, dot, faces on the track | Full title row, 6 px segmented stage bar, activity line | The owner's notes |
| Tracks ordered blocked first, merges last | Most recently active first; merges in place, green | The owner's notes |
| Group reviewers shown | Hidden by default (spec 0019) | The owner's notes |
| Fired-nudges counter in the header | Removed | The inbox owns nudges |
| Nothing animates | One optional crossfade on a mood change | Explains a change the user didn't cause (principles §1.1) |
