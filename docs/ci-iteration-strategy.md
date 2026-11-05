# CI iteration and release policy

Use this policy to choose verification by risk. The [CI workflow](../.github/workflows/ci.yml),
[selector](../tools/ci-scope.py) and [suite inventory](../tools/browser-suites.json)
define the current automation. The release owner records evidence for any required
check that the selector does not schedule.

## Change matrix

For mixed changes, use the combined coverage. Start with the smallest local test
that exercises the change and its boundary. Check the workflow's selection summary
on the PR and the release. Every selected browser job uses the real server.

| Change | Local and PR review | Current automatic PR selection | Evidence before publishing, in addition to the delivery gates below |
| --- | --- | --- | --- |
| Documentation and UI skill bundle | Review instructions, examples and links. | Selector tests and identity-provider contracts. | No new application image is normally needed. If included in an application release, the other changes determine scope. |
| Local presentation | Typecheck/build and exercise the affected component or browser behavior. Include compact layout, keyboard or visual checks when affected. | Frontend checks. Unmapped components get full browser coverage. CSS-only changes in the recognized UI directories get the layout suite. | Confirm the relevant behavior on the release revision. Targeted selection for more components belongs to [#348](https://github.com/hbjoroy/sproyt/issues/348). |
| Global CSS | Check shared layouts, responsive breakpoints, focus, scrolling and touch in the affected views. | Recognized CSS-only changes get the layout suite in Chromium and its maintained WebKit subset. Legacy assets or mixed unclassified runtime code can select full. | Preserve cross-browser layout and interaction coverage, including affected mobile paths. |
| Feature logic | Exercise feature rules, failure cases and the affected integration boundary. | Declared frontend features select their suites and smoke (imagegen has its own suite). Backend changes select Rust, PostgreSQL, codec and operational contracts, with browser coverage for shared contracts and recovery for selected paths. | Include the affected end-to-end flow where the change crosses a client/server boundary. |
| Shared client state | Test navigation, focus, scrolling, drafts, reconnect or account isolation as affected. | Shared/unclassified frontend code selects full browser regression. Shared Rust/protocol changes can also select backend and recovery checks. | Keep broad browser coverage for shared state, transport and protocol changes. |
| Auth and access | Test both allowed and denied paths, session recovery and account isolation. Use the configured provider where integration matters. | Provider-only paths select provider contracts. Application auth follows path rules, so direct changes to `src/auth.rs` can select backend checks without browser checks. | Verify affected session, authorization and provider behavior explicitly. Add relevant browser/integration evidence when current automation omits it. |
| Persistence and migrations | Exercise old and new data, upgrade compatibility, failure recovery and restore as affected. | Migration, database, domain and selected runtime paths get Rust, PostgreSQL and backup/restore. Shared paths also select full browser checks. | Verify forward migration, compatibility with the rollback image and integrity after restore when the change affects durable data. |
| Build and runtime | Check the changed build/runtime contract, including deploy configuration or operational scripts where affected. | Unknown build, dependency and configuration paths fall back to full. Explicit CI-policy-only, codec-only and dedicated native paths have narrower rules. | Retain delivery checks. Exercise actual rollout and rollback in isolation for changes that affect those paths. |

The current selector reads the whole ancestor-to-head diff, including deletions
and both sides of renames. Changed browser specs are added to bounded selections,
and mapped features combine their test sets. Missing suite references fail
selection. Missing, invalid or non-ancestor baselines, version tags, scheduled
runs and `force_full=true` select full regression. Full means all Chromium specs
and the maintained iPhone WebKit suite, split across four and two isolated jobs.
Bounded browser selections run in Chromium and intersect with the maintained
WebKit suite. Quality builds the frontend and browser server once for all
shards. Each job installs its selected engine, uses the same-run artifact and
starts its own temporary SQLite database and server. Keep one worker per job
because tests within a file can share server state. Retain timing reports on
every run and traces and reports on failure. Backend selections also run the
frontend build and boundary checks. The dedicated
[native interoperability workflow](../.github/workflows/native-probe.yml)
has its own path-filtered PR and main-push triggers.

[#348](https://github.com/hbjoroy/sproyt/issues/348) owns more precise UI mappings,
with shared state and global interactions retaining broader coverage.
[#349](https://github.com/hbjoroy/sproyt/issues/349) owns risk-based backend
selection, including the auth coverage gap and the current coupling of codec
and operational checks to every backend change. Until those changes land, use
the actual selection and supply any missing required evidence. Run full
regression when the change boundary or baseline is uncertain.

## Delivery phases

- **Local and PR.** Use the matrix for focused feedback. PR CI always tests the
  selector and runs identity-provider contracts after successful selection.
  Backend, frontend and browser work follow the selection. The browser gate
  requires success from every selected shard or an explicit no-browser decision.
- **Publish a new application revision.** Dispatch CI from reviewed `main`
  with `publish_image=true`, or use a reviewed `v*` release tag. The canonical
  run validates the exact application revision. Pass the last verified application
  revision as `base_revision` to cover the full release interval, or omit it for
  full checks. A PR run validates its PR revision. Merging does not trigger an
  additional automatic main-push run of `CI`. Verify the run's head SHA before
  accepting its artifacts. Publication requires successful selection,
  quality, browser gate, dependency audit and ARM64 delivery. PostgreSQL and
  recovery may skip only when the selector explicitly excludes them. The
  delivery job builds the web and media worker images, validates revision and
  architecture, checks Helm delivery, generates SBOMs and scans both images.
  Publication imports those same artifacts as immutable commit tags and records
  registry digests. Heart archive publication follows its separate checks.
- **Promote the verified Canary release.** Reuse the same image digests and chart
  revision. Validate the production values and GitOps Application, then verify
  Argo, ready replicas, public readiness and the deployed revision. Application
  CI evidence follows the digest. Assess separate migration, auth, backup or
  network changes and record worker and authenticated smoke evidence as
  applicable. See the [release checklist](release-checklist.md).
- **Weekly regression.** Monday at 03:17 UTC, run full browser, backend,
  PostgreSQL, recovery, dependency and ARM64 delivery checks without publishing.
  CI/release reviews failures and omissions, including regressions missed by
  scoped runs. A manual `force_full=true` dispatch with both publish inputs off
  provides the same breadth without publishing.

## Rollout and rollback

- **Automated.** CI checks Helm lint, rendering and delivery contracts, and
  backend selections exercise the production rollout verifier against fixtures.
  Selected PostgreSQL jobs exercise repository, upgrade and backup/restore
  contracts. These checks cover manifests, verifier behavior and data contracts.
- **Manual.** The release owner verifies actual Canary and production rollout
  using the [release checklist](release-checklist.md), records evidence and
  identifies the last verified rollback digest. Changes to the chart, migration,
  runtime or rollout/rollback path trigger a rehearsal on an isolated target
  before promotion. Record the applicable install, scale, upgrade and rollback
  results, including data compatibility. Use the [operations runbook](operations.md)
  for rollback and incident handling.
- **Deferred.** The native AMD64 kind install/scale/rollback job is disabled in
  CI. `tools/kind-smoke.sh` is available for isolated execution with development
  auth. It exercises replica changes and Helm rollback using one preloaded
  image. CI/release re-evaluates automation once a bounded, independently
  verified cache backend or retention plan can support it alongside ARM64
  delivery.

## Required GitHub statuses

Read-only audit of `hbjoroy/sproyt` main at `f1a63938a5633af9a1fd5f6312eee1b75c51de0c`
on 2026-11-05 at 07:57 UTC. Run the following as `gh api <path>`.

| API path | Observed result |
| --- | --- |
| `repos/hbjoroy/sproyt/branches/main` | `protected: false` |
| `repos/hbjoroy/sproyt/branches/main/protection` | HTTP 404, `Branch not protected` |
| `repos/hbjoroy/sproyt/rulesets?includes_parents=true` | `[]` |
| `repos/hbjoroy/sproyt/rules/branches/main` | `[]` |

GitHub currently shows no required merge statuses for `main`. The workflow's
publication conditions are enforced separately. Requiring PR CI for merges
would need a branch protection or ruleset change by CI/release. Require the
stable `Selected browser checks` job, which verifies `Select relevant checks`
and `Format, lint, and test` as well as the selected browser matrix. Enforcing
selected PostgreSQL and recovery jobs at merge would need a stable aggregate
that checks their results and explicit scope exclusions. No settings were
changed in this audit.

## Iteration budget

These initial warm-cache targets for PR feedback on GitHub-hosted runners
will be evaluated against measured results in #350.

| PR class | Median elapsed to PR feedback | Median total runner time |
| --- | ---: | ---: |
| Documentation only | 3 min | 5 runner-min |
| Bounded local presentation or mapped frontend behavior | 12 min | 25 runner-min |

In [#350](https://github.com/hbjoroy/sproyt/issues/350), compare representative
runs of the same class before and after selection changes. Record the commit,
baseline, selected checks, cache hit/miss and sample size. Measure elapsed time
from run creation to the last applicable PR check completing. Sum each
executed job's start-to-completion time for runner minutes. Record queue and
setup separately using job/step timestamps, and use shard timing reports to
separate setup from test execution. Report warm and cold samples separately and
check that selected gates passed. Review parallelism if setup dominates. Measure
publication and promotion separately from PR feedback.

Historical examples for context. In [run 40751547076](https://github.com/hbjoroy/sproyt/actions/runs/40751547076),
sequential Chromium and WebKit spent 1,578 s and 739 s in the quality job.
ARM64 compilation took 377 s independently. In [run 40768799498](https://github.com/hbjoroy/sproyt/actions/runs/40768799498),
ARM64 dependency compilation took 7m04s with no restored BuildKit layers.
Two main Cargo archives then occupied about 6.6 GiB. That is consistent with
cache storage pressure, and the API did not retain a verified eviction order.
The workflow caches Cargo dependencies without workspace binaries, with only
main writing those archives. Separate ARM64 web and worker BuildKit scopes
retain intermediate layers. Keep cache state visible when comparing runs.
