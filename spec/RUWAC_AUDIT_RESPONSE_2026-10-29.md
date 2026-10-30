# Linux v0.4.50 audit: implementation ledger

Reviewed 2026-10-29 against delivered v0.4.51, commit
`dba654e86a738299e51f26a6f32d37ea8e787012`. The independent audit was
source-only. Generated tests and owner observations below retain separate scope.
This ledger does not declare the full product qualified.

| Finding | Verified disposition | Remaining acceptance |
| --- | --- | --- |
| P0 cross-computer PTY sizing | Still open in released51. A separate unpublished candidate implements opt-in controller-owned sizing, canonical pane geometry, connection leases and viewer cameras; it needs reconciliation with later operational fixes. | Preserve original PID/grid and SIGWINCH count through small viewers, per-pane splits/zoom, disconnect, takeover races, legacy/SSH/nested paths and crash recovery. Do not duplicate or blindly import the candidate. |
| P1 early-release history selection | Confirmed in released51. Unreleased candidate defers one bounded copy intent, validates the starting viewport, cancels on focus/typing/target changes and refuses unavailable or timed-out history. Native0/100/500/2000ms reply-delay cases pass with off-screen markers and exact PRIMARY; cancellation cases remain under qualification. | Bounded target/generation-bound copy intent or explicit refusal; immediate/100ms/500ms/2s replies, output/focus changes, truncation and UTF-8. |
| P1 RDP wheel loss | Confirmed: send classified vertical/horizontal wheel as disposable motion. Current unreleased correction retains exact wheel batches in the existing ordered bounded queue; only pure MOVE can be dropped. Five focused queue tests pass; the two added regressions fail original51. | Saturated128 channel, reverse/horizontal/small deltas, mixed batches, click/key ordering, capacity errors and focus/transport cancellation. External GNOME/Windows gestures remain separate. |
| P1 stale release notes | Released51 notes now match51. Unreleased local controller adapter and GitHub publication guard reject a wrong/missing notes version or absent/duplicate changelog section; four bounded tests pass. Older releases retained as history. | Exercise the integrated guard in the next normal controller run and verify the next public body. |
| P1 stale acceptance ledger | Updated with delivered51 exact-commit summary and hosted links; detailed private receipts retained by maintainer. | Hosted deterministic selection invocation and sanitized downloadable receipts; fast-release and physical/endurance cases remain open. |
| P1 dependency security | RSA0071 remains unsuppressed. libz-sys1.1.31 additionally fixes newly detected0418 in51. crypto-bigint0.7.6 yank is an upstream API correction, not evidence of recurrence of its earlier advisory. | Qualify compatible0.7.10 separately with locked tests and production dependency graph; investigate upstream RSA feature reduction without a cryptographic fork. Retain atomic-polyfill/rustls-pemfile maintenance warnings. |
| P2 full-grid protocol cost |51 avoids full projections when the visible metadata/epoch is unchanged, including hidden output. Seven regression cases and19-pane/four-client benchmark pass; visible changed grids remain full snapshots. |400x103/nested/UTF-8/5-and20-session comparisons, allocations and checkpoint writes. Consider negotiated deltas only with legacy fallback. |
| P2 grapheme fidelity | Open. Canonical historical text lacks complete style/grid provenance; current width-based reflow is not a full grapheme oracle. | ZWJ families, variation selectors, flags, skin tones, bidi and multi-column cases against actual terminal cursor grids. |
| P2 issue/presentation hygiene | README/About/Support improvements are delivered; no new issue/PR state changes made for this audit. | Reconcile Web Preview issue2 with external acceptance, retain sizing issue4, review contributor PR/checks and add sanitized screenshots. |

## Historical quota pause checkpoint — 2026-10-29

Paused for the owner40-percent rule on2026-10-29. This candidate is unreleased.
Five focused queue tests and four notes-guard tests pass. Final-build native
0/100/500/2000ms reply-delay and retained held-drag cases pass. The focus case
clears selection, but its final clipboard check cannot run: the fixture failed to
restore canvas focus after its About window, yielding “No active clipboard view”.
That is not evidence of a changed clipboard and does not qualify cancellation.
Typing, timeout, target-change, busy-output and truncation acceptance remain open.
The225 desktop-unit pass predates the final small guards and must be rerun.

The inline8MiB warning currently follows viewport validation. A truncated prefix
that no longer matches the starting screen may therefore show an output-change
message first. Accurate limit feedback/refusal before validation remains required.
No new version, publication or installation has occurred for this audit candidate.

## Scoped release follow-up — 2026-10-30

The owner separately requested another release while the broad goal stays paused.
The focused cancellation test now closes its actual About window with native
Alt+F4, restores canvas focus and verifies unchanged PRIMARY. It passes without
removing the earlier exact clipboard assertion. The new inline truncation check
refuses an incomplete prefix before viewport validation, displays its8MiB limit
and preserves PRIMARY. Its focused test uses the real worker's accumulation cap
with explicitly synthetic sub1MiB pages behind a real frozen daemon snapshot;
normal delay tests leave canonical data unchanged.

The existing release controller will freeze and rerun all19 categories and the
nine native history cases on the newly stamped binaries. These focused passes
alone do not establish publication, installed delivery or physical qualification.
P0 sizing and the other carried findings remain open for a later increment.

## Order and release boundaries

1. Reconcile controller sizing candidate with current released code, including
   projection-stamp fields and repaired dependencies; keep its unpublished scope
   until integration and acceptance are complete.
2. Finish and qualify fast-release selection and wheel ordering; preserve failed
   negative controls and distinguish generated input from external desktops.
3. Qualify dependency maintenance, notes validation and reproducible public
   acceptance evidence; retain RSA disclosure.
4. Continue history fidelity, performance and external/endurance qualification.

The normal controller owns versioning and publication. Delivered51 is complete;
postchecks must not allocate a new patch. Unreleased corrections below this
baseline are not installed fixes. Preserve owner sessions and credentials, and
save a handoff before the owner's40-percent quota pause threshold.
