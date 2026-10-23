# NNNN — Feature name

| Field | Value |
| --- | --- |
| Status | Draft · Proposed · Accepted · Shipped · Superseded |
| Milestone | vX.Y (see `docs/roadmap.md`) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | e.g. `crates/sync`, `crates/ado` |
| Serves | Snappy · Less noise (pick at least one; a feature that serves neither is cut) |
| Depends on | Other specs / ADRs by path |
| Design | `docs/design/*.md` owned by ux-designer (visuals, motion, sound, copy) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

The pain, in the user's words. What is slow or noisy today in the Azure DevOps web UI or email flood? One paragraph, no solution.

## 2. Users and job to be done

- **Persona:** developer on Azure DevOps drowning in PR / work item / pipeline noise and a slow web UI. Narrow it if the feature targets a sub-group (reviewer, PR author, on-call).
- **Job to be done:** "When ___, I want to ___, so I can ___."
- **Smallest delightful slice:** the one thing this spec must nail.

## 3. UX flow

Numbered happy path, keyboard-first. Then alternate paths. Link the design doc for layout and motion; do not restate visuals here.

### States

| State | Behavior |
| --- | --- |
| Empty | |
| Loading / first sync | |
| Error | |
| Offline / stale | |
| Throttled by ADO | |

## 4. Functional requirements

Numbered `FR-n`. Each one is a behavior, not an implementation.

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |

Mandatory flags for every feature:
- **Rate limits:** expected TSTU impact (see `docs/specs/0002-sync.md` §ADO realities) and how the feature backs off.
- **No push to local apps:** freshness is bounded by the polling tier this feature relies on.
- **PAT scopes:** exact `vso.*` scopes; mark high-privilege scopes and what degrades without them.

## 6. Acceptance criteria

Testable, numbered `AC-n`, Given / When / Then. Each must be verifiable by an automated test (mock ADO server, fake `az`, injected clock) or a scripted manual check named explicitly. No "should feel fast".

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

adoken ships no telemetry. Define how we know this worked: a local-only stat the user can see, a dogfood observation, a build-in-public reply pattern, a support-question that stops appearing.

## 9. Risks

## 10. Open questions (for the user)

Numbered `Q-n`, each with the PM's recommended answer so the user can reply "yes" or override. Once the user decides, rename the heading to `## 10. Open questions — resolved YYYY-MM-DD` and replace each recommendation with `**Resolved:** <decision>`, linking the FR/AC that now encodes it.

## 11. Out of scope

What this spec explicitly does not do, and where it went (a later spec, `docs/ideas.md`, or cut).

## 12. References

Links to Microsoft Learn / REST reference pages used for §5.
