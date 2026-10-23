# adoken design principles

Status: v0.1. Owner: ux-designer. Every view, component, and spec UX section follows this doc.

Index:
- [principles.md](principles.md) — this file: principles, window anatomy, universal states, copy, accessibility.
- [tokens.md](tokens.md) — color/type/spacing/radius/elevation/motion/sound tokens, theme files, custom backgrounds.
- [onboarding.md](onboarding.md) — first run: az cli detection, PAT, first sync.
- [command-palette.md](command-palette.md) — Cmd+P universal search, prefixes, result rows, full keyboard map.
- [sound.md](sound.md) — event → sound map, the built-in sounds, volume, quiet hours.
- [today.md](today.md) — Today as "my day": day summary with a mood illustration, Your PRs with stage bars, Up next, Waiting on you, Loose ends (spec 0017).
- [ken.md](ken.md) — opt-in arcade layer: combos, K.O., streaks.
- [pins.md](pins.md) — pinned PRs, work items and pipelines in the sidebar.
- [work-item-detail.md](work-item-detail.md) — work item detail: rich description, fields, inline editing, links, discussion, freshness.
- [activity.md](activity.md) — statusbar sync item, network indicator and resource meter, Activity drawer (sync activity, network log).
- [pr-review-polish.md](pr-review-polish.md) — PR review: file filter, Threads view, group reviewers, status actions, copy title and branches (spec 0019).
- [tours.md](tours.md) — first-use guided tours: coach mark, spotlight, every step's copy (spec 0020).

Specs (`docs/specs/`) own behavior, acceptance, and performance. These docs own visuals, motion, sound, copy.

## 1. Principles

1. **Instant beats animated.** Local-first means every read is local. Selection, focus, typing, and navigation are `motion.instant`. Animation only explains a change the user didn't directly cause (a new notification, a merged PR), smooths a spatial change (pane opening), or celebrates your own finished work once Azure DevOps confirms it (spec 0022, [ken.md](ken.md) §12). Budgets from AGENTS.md are design constraints: frame < 16 ms, Cmd+P < 50 ms, cold start < 500 ms.
2. **Keyboard first, mouse welcome.** Every action is reachable from Cmd+P and shows its shortcut where it's offered (button tooltips, menu rows, palette rows). No hover-only affordance: anything that appears on hover also appears on keyboard focus.
3. **Calm by default, loud by choice.** Default install: no sounds, no Ken layer, badges only for things addressed to *you*. Users turn up the volume (sounds, arcade, custom backgrounds); we never turn it up for them. The one default-on effect is a short celebration of your own finished work (spec 0022): calm style, silent, rate-limited, off with one switch in Settings › General or `⌘P`.
4. **Honest freshness.** Local data can be stale. Always show when it was last synced, never pretend a write succeeded before ADO confirmed it (optimistic UI shows a pending state, not a done state).
5. **One place for each thing.** A PR has one canonical view; notifications link into it; the palette opens it. No duplicate half-views.
6. **Theme tokens only.** No literal colors, sizes, durations, or sounds in components ([tokens.md](tokens.md)).
7. **Playful is opt-in and original.** The Ken layer ([ken.md](ken.md)) never blocks work, never pressures, never borrows Capcom/Street Fighter names, sprites, poses, or audio. The calm celebration of your own finished work (spec 0022) is the one playful touch on by default (§1.3); its arcade style stays with the opt-in Ken layer.
8. **Accessible without a mode.** AA contrast, full keyboard, screen-reader labels, reduce motion, and mute work in every theme and with every background. They are not an "accessibility theme".

## 2. Window anatomy

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   ‹ ›   Contoso / Payments ▾          ⌘P Search…            ◔  ⚙ │ titlebar 38
├────────────┬──────────────────────┬──────────────────────────────────────┤
│ My Work  3 │ Pull requests        │ !4821 Retry idempotency keys         │
│ Inbox   12 │ ─────────────────────│ ──────────────────────────────────── │
│ PRs        │ ▸ !4821 Retry idem…  │ Overview · Files 14 · Commits 6      │
│ Work items │   !4817 Bump azure…  │                                      │
│ Pipelines  │   !4809 Drop legacy… │   detail pane (flex)                 │
│ ────────── │                      │                                      │
│ Projects   │   list pane 380      │                                      │
│  Payments  │                      │                                      │
│  Identity  │                      │                                      │
│ sidebar 232│                      │                                      │
├────────────┴──────────────────────┴──────────────────────────────────────┤
│ ● Synced 2m ago · Contoso        Quiet hours until 08:00 ⇅ 200 ×3 COMBO │ statusbar 24
└──────────────────────────────────────────────────────────────────────────┘
```

- **Titlebar:** back/forward through views (`Cmd+[` / `Cmd+]`, mouse back/forward buttons; spec 0018), project switcher (`Cmd+O`), palette trigger (click opens palette; shows `⌘P`), sync indicator ◔ (click = sync now, `Cmd+R`), settings (`Cmd+,`).
- **Sidebar sections (fixed order):** My Work (nudges + my active items), Inbox (notification center), Pull requests, Work items, Pipelines, then Projects (multi-select filter for the single org; one org per install). Counts are unread/actionable only. Collapse to a 56 px icon rail with `Cmd+B`: labels become tooltips, counts become badges on the icons, and the settings gear and org avatar stay at the bottom. The state persists in `settings.toml [sidebar]`.
- **Pinned group** (spec 0014, [pins.md](pins.md)): after the sidebar sections, the user's pinned PRs, work items and pipelines, one row each with a live status and a change dot; `⌘J` lists them in the palette. It scrolls inside itself (sections and the bottom block stay put) and folds to one summary line. In the rail each pin is one icon, and hovering or focusing them peeks the full group over the content without reflowing it.
- **List pane:** virtualized; two-line rows by default (`size.row2.*`). Sort + filter bar at top (`/` focuses filter).
- **Detail pane:** the canonical view for the selected item. `Cmd+Shift+Enter` toggles it full-width (list hidden) in Work items and Pipelines. The work item detail shows a properties rail (280) when the pane is ≥ 1000 px wide ([work-item-detail.md](work-item-detail.md) §2).
- **Pull requests: one pane at a time** (user decision 2026-09-30; spec 0004 §3.1): the full-width list, or the full-width review of one PR, never side by side, so long titles and diffs get the whole width. `↩` / a click opens the review; `Esc` or the back button leading the review header's title row returns to the list with the row still selected and the scroll kept. `Cmd+[` is history back (spec 0018): it returns to the list the same way when the review was opened from it, and to the Inbox, Today, a pin or the palette's origin otherwise. PR rows are taller than `size.row2.*`: two title lines, then the meta line with badges and vote-grouped reviewer avatars.
- **Statusbar (left → right):** sync state (button: opens the Activity drawer's Sync tab), org name; center: quiet-hours / snooze state; right: network indicator (opens the Network tab; becomes the Offline pill while offline), Ken combo counter (only if enabled), resource meter (spec 0023; optional, off by default), demo pill.
- **Activity drawer** (`⌘⇧A`, [activity.md](activity.md), spec 0016): docks above the statusbar in the content column; the sidebar or rail keeps its full height.
- Minimum window 900 × 600. Below 1100 px width, the list pane and detail pane become a stack (detail slides over list; `Esc` returns).

## 3. Universal states

Every view (and every spec) defines these five states using the patterns below. Engineers implement them once in `crates/ui` as `EmptyState`, `SkeletonList`, `InlineError`, `Banner`, `StalenessLabel`.

### 3.1 Empty

Centered in the pane, max width 360: 20 px Lucide icon in `text.tertiary`, one-line headline (`type.heading`, `text.primary`), one line of body (`type.body`, `text.secondary`), at most one primary action button with its shortcut keycap.

Two kinds, never confused:
- **Nothing here (good):** "No reviews waiting on you" · body "You're all caught up." · no action. (Ken layer: may show "Clean sweep" toast once per day, see ken.md.)
- **Filtered to nothing:** "No pull requests match `author:@me is:draft`" · action [Clear filters `⌘⌫`].

### 3.2 Loading

- **First sync only** shows skeletons (`SkeletonList`: rows at the view's row height, `bg.raised` blocks, shimmer unless reduce motion). After first sync, views render from the store immediately and never show a skeleton again; background sync updates rows in place.
- Anything expected to take < 150 ms shows nothing. If still loading at 150 ms, show the indicator; once shown, keep it at least 300 ms (no flash).
- Remote-only fetches (e.g. a PR's full diff the first time it's opened) show a 2 px indeterminate bar under the pane header plus cached content if any.
- Buttons that trigger a network write show an inline spinner replacing the icon and keep their width; label stays.

### 3.3 Stale / syncing

- Statusbar: `● Synced 2m ago` (dot `status.success.fg`) · while syncing `◔ Syncing…` (`status.info.fg`) · `▲ Sync delayed — retrying in 30s` (`status.warning.fg`) when the last attempt failed but a previous one succeeded · `‖ Sync paused · until 15:00` (`pause`, `text.secondary`) while the user paused sync (spec 0016 FR-9). Per-kind delays and failures live in the Activity drawer's Sync tab ([activity.md](activity.md) §4).
- Data older than 15 min in a view shows a `StalenessLabel` in the pane header: "Updated 18m ago · Refresh `⌘R`".
- Pending writes (vote, comment, work item field, link) render immediately with a `clock` 12 px glyph + `text.tertiary` "Sending…"; on confirm the glyph disappears (no success toast for routine writes); on failure → inline error on that element (§3.4).

### 3.4 Error

Specific, local to the thing that failed, with a fix:
- **Inline** (default): under the failing element, `status.danger.fg` icon + one sentence + action. "Couldn't post your comment. Your draft is saved. [Retry]".
- **Banner** (pane-level, below pane header, `status.danger.wash`): used when the whole pane can't load. "Couldn't load pipeline runs for Payments. ADO returned 503. [Retry] [Details]".
- **Token rejected or expired** (spec 0001 FR-15): no banner, modal or toast. The statusbar sync item reads "Token rejected" / "Token expired", the Activity drawer's Sync tab shows one empty state with **Replace token…**, a write tried meanwhile fails inline ("Azure DevOps rejected adoken's token. Replace it in Settings › Account."), and the inbox gets one quiet item per episode ([onboarding.md](onboarding.md) §10).
- Never show raw stack traces. [Details] expands a monospaced block with status code, request id (`x-vss-activityid`), and time, plus [Copy].
- Error copy template: *what happened* + *what's kept safe* + *what to do*.

### 3.5 Offline

- Detection: network reachability change or 2 consecutive failed requests with a network error.
- Statusbar right: the network indicator becomes the pill "Offline" (`text.secondary` on `bg.raised`, icon `wifi-off`; same visuals, defined in [activity.md](activity.md) §2.3). No modal, no banner.
- Reads keep working from the store. Write actions (vote, comment, complete, work item edits) stay visible but disabled with tooltip "Offline — reconnect to send". Comment and description drafts keep saving locally.
- On reconnect: pill fades out (`motion.base`), sync runs, no toast.

## 4. Components that recur

- **Keycap** (`type.kbd`, `radius.xs`, 1 px `border.default`, `bg.raised`, min width 18, height 18). Shown in tooltips, menus, palette rows, empty-state buttons. macOS glyphs `⌘ ⌥ ⇧ ⌃ ↩ ⌫ ⎋`; Windows/Linux words `Ctrl Alt Shift Enter`.
- **Toast** (bottom-right stack, max 3 visible, 360 wide, `elevation.2`, `radius.md`): auto-dismiss 5 s (8 s if it has an action), pauses on hover/focus. `F6` cycles focus: content → Activity drawer (when open) → toast region. Used for results of explicit user actions and for notification delivery when adoken is focused.
- **Focus ring:** 2 px `border.focus` outside the element with 1 px gap, radius = element radius + 2. Visible on keyboard focus only (`focus_visible`), always visible in lists as the row selection.
- **Selection:** focused list = `bg.selected`; unfocused list = `bg.selected_muted`. Hover never looks like selection (hover = `bg.hover` only).
- **Avatars:** the person's Azure DevOps profile image once it is cached on disk (behavior, fetch and cache rules: `docs/specs/0012-avatars.md`), else initials on a color derived from the identity id (hash → one of the 6 accent preset bases, text `accent.on` of that preset). Circle (`radius.full`) at `size.avatar.sm` / `md` / `lg` (16 / 20 / 28). Initials are also the loading state: no spinner, no skeleton, no blank frame; the image replaces the initials in place (`motion.instant`, same box, zero layout shift). Groups and teams (container identities) and anyone without a picture keep initials.

## 5. Copy voice

- Sentence case everywhere (titles, buttons, menu items). No all-caps except Ken lettering.
- Direct and short. Buttons are verbs: "Approve", "Snooze 1h", "Replace token…". Never "OK"; "Cancel" only in dialogs.
- Second person, no blame: "Couldn't reach Azure DevOps" not "You are offline".
- No exclamation marks outside the Ken layer. No emoji in UI chrome.
- Use ADO's own nouns: pull request (PR in tight spaces), work item, pipeline, run, reviewer, vote. IDs as ADO writes them: `!4821` PR, `#1234` work item.
- Times: relative under 7 days ("2m", "3h", "Tue"), absolute after ("Sep 12"), full timestamp in tooltip and accessible label.

## 6. Accessibility

- **Contrast:** AA everywhere — 4.5:1 text, 3:1 large text (≥ 18.66 px bold / 24 px), 3:1 focus rings, control outlines, and state icons. Enforced by the theme validator ([tokens.md](tokens.md) §9.3) and by the background legibility pass (§10.4).
- **Keyboard:** full operation without a pointer; visible focus; no traps (every overlay closes with `Esc`, focus returns to the invoking element). Tab order = visual order. Lists use arrow keys (`↑ ↓`, `j k`) not Tab per row.
- **Screen readers:** GPUI (zui fork) exposes AccessKit roles via `.role(gpui::Role::…)`, `.aria_label(…)`, `.aria_selected(…)`, `.aria_expanded(…)`, `.on_a11y_action(…)`. Every interactive element sets a role and label; icon-only buttons label the action ("Approve pull request !4821"), not the icon. Lists: `Role::List` / `Role::ListItem` with label = the row's full text (title, id, state, relative time spelled out: "2 minutes ago"). Live regions: toasts and sync status announce politely; auth errors assertively.
- **Reduce motion:** `system | on | off` ([tokens.md](tokens.md) §7.4). **Quiet mode:** one switch (palette "Quiet mode: turn on or off", "Quiet mode for 1 hour") silences sounds, celebrations and the Ken layer (spec 0025 FR-8).
- **Color independence:** every status has an icon or text label, not only a color.
- **Text scale:** `font_scale` 0.9–1.3 reflows without truncating primary actions.
- **Targets:** ≥ 24 × 24 px pointer targets.
