# 0005 — Notification center: one inbox, your rules, your hours

| Field | Value |
| --- | --- |
| Status | Draft |
| Milestone | v0.0.4 (inbox + filters) → v0.1.0 (snooze, quiet hours, delivery windows, OS banners) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/notify` (rules, scheduling, delivery), `crates/app` (inbox UI), `crates/store` |
| Serves | Less noise (primary), Snappy (triage at keyboard speed) |
| Depends on | `docs/specs/0002-sync.md` (event stream FR-11, self-suppression FR-12), `docs/specs/0006-smart-nudges.md` (nudges arrive as notifications) |
| Design | `docs/design/principles.md`, `docs/design/sound.md` (event → sound, quiet-hours interplay) |

## 1. Problem

Azure DevOps notifies by email. A normal day is dozens of "Build succeeded", "X voted", "Policy updated", "Y commented" mails, interleaved with the three that matter (you were mentioned, your build broke, a review is waiting on you). Email filters are brittle, ADO's own subscription settings are per-project and verbose, and nothing respects "don't ping me about pipelines after 6 pm". People either drown or turn it all off and miss the important ones.

## 2. Users and job to be done

- **Persona:** dev whose inbox is 80% ADO noise; team lead who needs mentions but not every build.
- **JTBD:** "When something in ADO needs me, I want exactly one signal at a time I choose, and everything else quietly available, so I stay in flow without missing what matters."
- **Smallest delightful slice:** a keyboard inbox with sensible default priorities, self-caused and duplicate noise already gone, snooze that actually comes back, and quiet hours.

## 3. UX flow

1. **Inbox view** (default home screen). Rows grouped by *subject* (one row per PR / work item / pipeline run), newest activity first, showing the latest event and a count ("3 new comments").
2. **Filter bar:** chips for Source (PRs, Work items, Pipelines, Mentions, Nudges), Priority (P0–P3), State (Unread, All, Snoozed, Done). Chips are keyboard toggleable (`1`–`5` sources, `Shift+1..4` priority, per `docs/design/principles.md`). The filter set persists.
3. **Triage keys** (also in `Cmd+K`): `Enter` open (PR → 0004; WI/pipeline → web or detail), `e` done, `u` toggle unread, `s` snooze menu, `m` mute subject, `Shift+M` mute… (repo / pipeline definition / work item type / author), `z` undo last action.
4. **Snooze menu:** 1 hour · 3 hours · Tomorrow 9:00 · Monday 9:00 · Until it changes · Custom date/time. Times use the user's working-hours start (0006 settings) instead of 9:00 when set. Snoozed items return as unread with a "snoozed" badge.
5. **Settings → Notifications:**
   - **Priority rules** table: event kind → priority (defaults in FR-3), editable; plus per-source minimum priority to appear in the inbox at all.
   - **Delivery:** for each priority, choose *Inbox only* / *Inbox + banner* / *Inbox + banner + sound*. Defaults: P0 banner+sound (sound only if sounds enabled), P1 banner, P2/P3 inbox only.
   - **Quiet hours:** schedule (days + from/to, overnight ranges allowed). **Off by default**; offered on the last onboarding screen (0001 step 10) pre-filled weekdays 19:00–08:00 + all weekend. During quiet hours no banners or sounds; items still land in the inbox. Option "Let P0 through" (default on).
   - **Delivery windows per source:** e.g. Pipelines only Mon–Fri 09:00–18:00. Outside a source's window its events are held; at the window's start one summary banner is delivered ("While you were away: 4 pipeline updates").
6. **Dock badge:** count of unread P0 + P1 (configurable: none / P0 / P0+P1 / all unread).

### States

| State | Behavior |
| --- | --- |
| Empty (inbox zero) | Calm empty state (optionally Ken-flavored per `docs/design/ken.md`); shows next snoozed item's return time. |
| First sync | Silent backfill per 0002 FR-13; inbox shows current "needs you" items computed from state, not a flood of historic events. |
| Offline / stale | Inbox fully usable; triage actions are local and always work; status pill shows staleness. |
| OS notifications not authorized | One-time inline prompt explaining banners; inbox works regardless. |

## 4. Functional requirements

- **FR-1 Ingestion.** Every `SyncEvent` (0002 FR-11) and every nudge (0006) becomes a `Notification {id, subject, source, kind, priority, actor, created_at, read, done, snoozed_until, snooze_mode, muted, held_until}`. Ingestion is idempotent on the event's dedupe key.
- **FR-2 Noise removal before anything else.**
  - Drop `self_caused` events (toggle to show).
  - **Coalesce** events on the same subject within 2 minutes into one notification (count + latest summary). The inbox row always represents one subject.
  - Drop events on muted subjects / muted repos / muted definitions / muted authors.
  - `BuildCompleted` with result `succeeded` for a run that was already green last time = P3 and inbox-only by default ("still green" is not news); failure after success, or success after failure, keeps its priority.
- **FR-3 Default priorities.**

  | Event | Default |
  | --- | --- |
  | `MentionedInPr`, `MentionedInWorkItem` | P1 |
  | `ReviewRequested` | P1 |
  | `BuildCompleted` failed, on my PR or requested by me | P1 |
  | `BuildCompleted` failed, pinned definition (spec 0014), not mine | P2 |
  | `PrMergeConflict` on my PR | P1 |
  | `TokenRejected` (0001 FR-15, Q-6) | P1, Inbox only by default: no banner, no sound (amended 2026-10-01, owner notes; accepted 2026-10-01; replaces `TokenExpiring`) |
  | `TokenReplaced`, `TokenReplaceFailed` (0001 FR-20) | P3, never a banner or sound, whatever the delivery settings and user rules (amended 2026-10-01, owner notes; accepted 2026-10-01) |
  | `PrCommentAdded` on my PR / threads I'm in | P2 |
  | `PrVoteChanged` on my PR (reject / wait for author) | P1; approvals P2 |
  | `WorkItemAssigned` | P2 |
  | `PrStatusChanged` (my PR completed/abandoned) | P2 |
  | Nudges (0006) | P2 |
  | `PrPushed` on a PR where I set auto-complete (0004 FR-12 guard) | P1 |
  | `PrPushed` on PRs I review | P3 |
  | `WorkItemChanged` | P3 |
  | `BuildCompleted` succeeded / partiallySucceeded / canceled | P3 |

  P0 is reserved for user rules (e.g. "mentions from my lead", "failed build on `main` of repo X") — no default rule produces P0.
- **FR-4 User rules.** Ordered list; first match wins; match on source, kind, project, repo, pipeline definition, author/actor, and "I'm required reviewer". Actions: set priority, mute, skip banner. Rules are data in the settings file and editable in UI.
- **FR-5 Snooze.** Time-based snoozes survive restarts and sleep: on wake, overdue snoozes resurface once. **Until it changes** resurfaces on the next non-self-caused event for that subject.
- **FR-6 Quiet hours & delivery windows** gate *interruptions* (banners, sounds, dock badge bumps), never inbox content. Held interruptions are released as a single summary per source at window end; if the summary would contain > 1 item it lists counts per kind.
- **FR-7 OS delivery.** macOS user notifications via the system notification center, grouped per subject (thread identifier = subject id), actions "Open" and "Snooze 1h". Clicking focuses adoken on the item. The OS's own Focus/Do Not Disturb is respected in addition to quiet hours.
- **FR-8 Sound** is only played if sounds are enabled globally (0007) and the priority's delivery includes sound; never during quiet hours or held windows.
- **FR-9 Retention.** Done items kept 30 days, then pruned; muted-subject list kept until unmuted.
- **FR-10 Local triage.** Read/done/snooze/mute are local state. They are not written back to ADO (ADO has no notification-read API).

## 5. ADO API surface and limits

- **No ADO calls.** The notification center consumes the local event stream only.
- **Rate limits:** zero additional cost.
- **No push:** end-to-end latency = sync tier interval + processing. Mentions and review requests land within the hot interval (≤ 60 s foreground, ≤ 5 min background). Stated honestly in Settings ("adoken checks every minute while open").
- **Coverage limits (inherited from 0002):** PR mentions are only detected on hot-set PRs; there is no ADO "mentions feed" API.
- **Not used:** ADO notification subscriptions (`vso.notification*`) — they configure email/service-hook delivery, which cannot reach a local app. adoken does not modify the user's ADO email settings (suggestion to reduce ADO email once adoken is trusted: `docs/ideas.md`).
- **PAT scopes:** none beyond 0002.

## 6. Acceptance criteria

Harness: event fixtures injected into `crates/notify` with an injected clock and a fake OS-notification sink and sound sink.

- **AC-1** Given 5 `PrCommentAdded` events on PR 912 within 90 s, the inbox contains one row for PR 912 with count 5, and at most one banner is delivered.
- **AC-2** Given a `PrVoteChanged` with `self_caused = true`, no notification is shown with default settings; enabling "show my own activity" shows it.
- **AC-3** Given default rules, `MentionedInPr` gets P1 and a banner; `PrPushed` gets P3 and no banner.
- **AC-4** Given quiet hours 18:00–08:00 and a P1 event at 22:00, the item is in the inbox immediately, no banner or sound fires until 08:00, and at 08:00 exactly one summary banner fires.
- **AC-5** Given "Let P0 through" and a user rule producing P0 at 22:00, the P0 banner fires at 22:00.
- **AC-6** Given a Pipelines delivery window Mon–Fri 09:00–18:00 and 4 `BuildCompleted` events on Saturday, zero pipeline banners fire on the weekend and one summary banner "4 pipeline updates" fires Monday 09:00.
- **AC-7** Snooze "Tomorrow 9:00" at 17:00 resurfaces the item as unread with a snoozed badge at 09:00 next day; with the clock jumped past 09:00 while "asleep", it resurfaces once on wake.
- **AC-8** Snooze "Until it changes" resurfaces on the next non-self-caused event for that subject and not on a self-caused one.
- **AC-9** Muting repo `web` hides existing and future notifications from that repo; unmuting restores future ones only.
- **AC-10** A green build following a green build of the same definition+branch is P3/inbox-only; a green build following a red one keeps its configured priority ("fixed").
- **AC-11** `e`, `u`, `s`, `m`, `z` act on the selected row and the list re-renders within one frame; `z` restores the previous state exactly.
- **AC-12** Dock badge equals the count of unread P0+P1 by default and updates within 1 s of ingestion.
- **AC-13** With OS notification permission denied, ingestion and inbox behave identically and no error is shown more than once.
- **AC-14** Replaying the same event twice (same dedupe key) produces one notification.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Inbox render from store, 10k notifications | first frame < 50 ms, scroll 60 fps | Frame recorder |
| Triage action (done/snooze/mute) | visible on next frame (< 16 ms) | UI trace |
| Event → inbox row | < 100 ms after store commit | Timestamp diff |
| Event → OS banner (when allowed) | < 1 s after store commit | Fake sink timestamp |
| Rule evaluation | < 50 µs per event, 100 rules | Bench |

## 8. Success signal (telemetry-free)

- Dogfood: builder creates an email rule archiving ADO notification mail after week 1 and doesn't revert it (the strongest "trust" signal).
- Local-only weekly card: "adoken filtered 212 events, interrupted you 9 times" — users share screenshots; the ratio is the headline number for build-in-public.

## 9. Risks

- **Coverage gaps** (PR mentions outside the hot set) could erode trust in "I can turn off email". Mitigation: explain coverage in Settings and let users pin PRs into the hot set (0002 FR-2). Considered and cut: reading the user's ADO notification emails to fill gaps (needs mailbox access; violates local-first privacy).
- **macOS notification permission** requires a signed app bundle; unsigned dev builds can't show banners. Release engineering dependency (release-manager).

## 10. Open questions — resolved 2026-09-25

All recommendations accepted by the user.

- **Q-1** Quiet hours on or off at install? **Resolved:** off, offered on the last onboarding screen with weekdays 19:00–08:00 + all weekend pre-filled (flow step 5, 0001 step 10).
- **Q-2** P1 banners on by default? **Resolved:** yes — P1 (mentions, review requests, my failed builds) gets banners by default (flow step 5 defaults, AC-3).
- **Q-3** Triage state (read/done/snooze) local-only, not synced across Macs? **Resolved:** yes for v0.1 (FR-10); multi-device sync is Later.

## 11. Out of scope

- Writing back read state or changing ADO email subscriptions.
- Mobile / cross-device sync of triage state.
- Menu bar mini-inbox (`docs/ideas.md`).
- Team-wide notification routing (Later).

## 12. References

- Rate limits (no extra cost here, but polling latency): https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
- Service hooks Web Hooks consumer (why ADO can't push to us): https://learn.microsoft.com/en-us/azure/devops/service-hooks/services/webhooks?view=azure-devops
- ADO notification scopes (`vso.notification*`): https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/oauth?view=azure-devops#available-scopes
