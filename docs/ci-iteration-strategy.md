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
| Local presentation | Typecheck/build and exercise the affected component or browser behavior. Include compact layout, keyboard or visual checks when affected. | Frontend checks and the exact mapped component contracts plus smoke. Unmapped components get full browser coverage. | Confirm the relevant behavior on the release revision. |
| Global CSS | Check shared layouts, responsive breakpoints, focus, scrolling and touch in the affected views. | Recognized CSS-only changes get the layout suite in Chromium and its maintained WebKit subset. Legacy assets or mixed unclassified runtime code can select full. | Preserve cross-browser layout and interaction coverage, including affected mobile paths. |
| Feature logic | Exercise feature rules, failure cases and the affected integration boundary. | Declared frontend features select their suites and smoke (imagegen has its own suite). Backend changes select the whole Rust gate. Separate PostgreSQL, codec and operator checks follow the affected contracts. | Include the affected end-to-end flow where the change crosses a client/server boundary. |
| Shared client state | Test navigation, focus, scrolling, drafts, reconnect or account isolation as affected. | Shared/unclassified frontend code selects full browser regression. Shared Rust/protocol changes also select backend checks. Recovery is assessed separately. | Keep broad browser coverage for shared state, transport and protocol changes. |
| Auth and access | Test both allowed and denied paths, session recovery and account isolation. Use the configured provider where integration matters. | Provider-only paths select provider contracts. Application auth and access paths select full browser regression plus the Rust and PostgreSQL integration gates. | Verify affected session, authorization and provider behavior, including live-provider evidence when the change affects that boundary. |
| Persistence and migrations | Exercise old and new data, upgrade compatibility, failure recovery and restore as affected. | Migration, database and domain paths get Rust and PostgreSQL. Shared paths also select full browser checks. Restore follows the independent recovery policy below. | Verify forward migration and compatibility with the rollback image. Require restore evidence when recovery assumptions change. |
| Build and runtime | Check the changed build/runtime contract, including deploy configuration or operational scripts where affected. | Unknown build, dependency and configuration paths fall back to full. Explicit CI-policy-only, codec-only and dedicated native paths have narrower rules. | Retain delivery checks. Exercise actual rollout and rollback in isolation for changes that affect those paths. |

The current selector reads the whole ancestor-to-head diff, including deletions
and both sides of renames. Changed top-level browser specs are added to bounded
selections, and mixed changes combine the relevant feature, layout and imagegen
sets. Removed frontend files and changed nested specs use full selection. Missing
suite references fail selection. Missing, invalid or non-ancestor baselines,
version tags, scheduled runs and `force_full=true` select full regression.
Full means all Chromium specs and the maintained iPhone WebKit suite, split
across four and two isolated jobs.
Bounded browser selections run in Chromium and intersect with the maintained
WebKit suite. Quality builds the frontend and browser server once for all
shards. Each job installs its selected engine, uses the same-run artifact and
starts its own temporary SQLite database and server. Keep one worker per job
because tests within a file can share server state. Retain timing reports on
every run and traces and reports on failure. Backend selections also run the
frontend build and boundary checks. The dedicated
[native interoperability workflow](../.github/workflows/native-probe.yml)
has its own path-filtered PR and main-push triggers.

### Bounded UI examples

The [#348](https://github.com/hbjoroy/sproyt/issues/348) mappings follow the
component imports and the paths that exercise the rendered controls.

| Representative change | Why these browser contracts apply |
| --- | --- |
| Thread-directory stylesheet | Only the thread directory imports this stylesheet. Directory, thread discovery and reading specs cover opening and closing the directory, finding threads and returning to reading. Both engines run these specs. |
| Work-item supplements | Registration and the work-item task view share supplement rendering. The supplements spec covers adding and reviewing supplements. The information spec checks the adjacent task view's drafts and requester/reviewer controls. Both engines run these specs. |
| Invitation card | The preview renders the card in channel and thread messages. The invitation spec exercises accept and decline across those surfaces in Chromium. WebKit keeps its existing touch smoke coverage. |

The exact stylesheet mapping applies while its selectors affect only the thread
directory. If effects extend outside that component, update the mapping and use
the cross-browser layout suite or full regression as the changed boundary
requires. Shared application, session, connection and conversation views keep
full browser coverage.

The selector now schedules three browser jobs for the thread stylesheet
(previously four) and two each for work-item supplements and the invitation
card (previously six each). Shared CSS still selects four jobs. These counts
describe selection. CI duration is measured separately.

### Backend risk selection

The [#349](https://github.com/hbjoroy/sproyt/issues/349) gates follow these
boundaries.

- **Rust.** Keep formatting, Clippy and the locked all-targets, all-features
  suite together for backend changes. Shared modules and integration tests cross
  feature boundaries. The selector has no per-test Rust filters.
- **PostgreSQL.** The repository job covers real PostgreSQL
  contracts and upgrade compatibility. Keep it for backend changes unless a
  reviewed file has no database integration, such as imagegen prompt
  interpretation. Backup/restore has a separate decision below.
- **Auth and access.** CI keeps the provider contracts in Quality. The separate
  OIDC discovery verifier fixture follows provider and application auth paths.
  Changes to application auth and access code also
  exercise real-server browser session, reconnect and permission coverage and
  the existing native grant and repository contracts.
- **Codec and operator checks.** Codec verification follows the isolated media
  worker and its decoder, preview, dependency and build/runtime boundaries.
  Operator fixtures follow changes to operational configuration, verifiers and
  their app or MCP contracts.

Use the union for mixed risks. Unknown changes and additions, deletions or type
changes in backend files keep full regression. A rename contributes both paths.

### Independent recovery decision

Full application regression does not imply a backup/restore drill. Ordinary UI,
backend, dependency and release changes, including a missing baseline or a new
tag, do not alone invalidate recovery evidence. Additive migrations retain Rust,
PostgreSQL and upgrade compatibility checks without automatically running restore.

The synthetic CI drill runs weekly, on changes to the recovery SQL fixtures or
post-restore cleanup, and when the `recovery-drill` job itself changes. Other
workflow edits do not trigger it when the job can be compared with the baseline.
Removing a migration also selects recovery. A migration that affects recovery
must include `-- ci-recovery: required` with a short reason. This is an explicit
review decision, not an SQL safety classifier.

For other recovery risks, dispatch CI with `force_recovery=true` and a verified
`base_revision`, with publication disabled for a rehearsal. Examples include
changed dump/restore tooling, database version or storage format, recovery
credentials, encryption, and incompatible data transformations. Record the
reason and successful run in the release evidence. When the change range is
unavailable, the release owner must assess these risks explicitly; broad
application checks cannot replace that assessment. Check the recovery decision
in the workflow summary before accepting a release.

Reuse documented recovery evidence while the relevant mechanism, data contract
and environment assumptions remain valid. Reusing an image for promotion does
not require another drill. The selector does not yet deduplicate recovery checks
across PR and publish runs when both include a recovery-triggering change.
The weekly synthetic drill complements the quarterly operational restore in the
[runbook](operations.md#backup-and-restore-drill); it does not create a production backup.

## Delivery phases

- **Local and PR.** Use the matrix for focused feedback. PR CI always tests the
  selector and runs identity-provider contracts after successful selection.
  Backend, frontend and browser work follow the selection. The browser gate
  requires success from every selected shard or an explicit no-browser decision.
- **Publish a new application revision.** Dispatch CI from reviewed `main`
  with `publish_image=true`, or use a reviewed `v*` release tag. The canonical
  run validates the exact application revision. Pass the last verified application
  revision as `base_revision` to cover the full release interval, or omit it for
  full application checks. Recovery is selected independently. A PR run validates its PR revision. Merging does not trigger an
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
  scoped runs. A manual `force_full=true` and `force_recovery=true` dispatch with both publish inputs off
  provides the same breadth without publishing.

## Rollout and rollback

- **Automated.** CI checks Helm lint, rendering and delivery contracts, and
  selected operator checks exercise the production rollout verifier against
  fixtures. The PostgreSQL repository and recovery jobs cover upgrade and
  backup/restore contracts. These checks cover manifests, verifier behavior and
  data contracts.
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

These warm-cache targets apply to PR feedback on GitHub-hosted runners.
See [measured iterations](#measured-iterations) for the #350 samples.

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

## Measured iterations

Run durations below come from the GitHub Actions job API for each run's latest
attempt. Wall time runs from workflow creation to the last executed job
completing. Initial queue runs from creation to the first job starting and
excludes trigger delays. Maximum job queue uses each job's own creation time,
which separates prerequisite waits. Runner time sums executed job durations,
excluding skips. Measured setup sums the time before the browser test step in
each browser job and explicit checkout, service, toolchain and cache setup
steps elsewhere. Installation inside combined build/test steps is included in
runner time and has no separate setup measurement. Revisions are the
checked-out merge SHA for PRs and the checkout SHA for dispatches. Cache warmth
refers to observed restores of Cargo dependencies, npm and Playwright where
selected. BuildKit is tracked separately.

| Actual run | Checkout / selection base | Selected checks and cache evidence | Wall | Runner-min | Queue initial / max job | Setup total |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| [Documentation PR](https://github.com/hbjoroy/sproyt/actions/runs/41991336688) | `12ff651` / `f1a6393` | Selector, provider contracts and browser gate. App caches unused. | 0:28 | 0.4 | 2 / 2 s | 5 s |
| [Shared CSS PR](https://github.com/hbjoroy/sproyt/actions/runs/41795884569) | `c81d5dd` / `55f8f94` | Frontend and layout, four browser jobs. Cargo, npm and both engines restored. | 5:45 | 13.5 | 3 / 2 s | 203 s |
| [Shared navigation PR](https://github.com/hbjoroy/sproyt/actions/runs/42001790199) | `f5f29f0` / `a8786c7` | Frontend and full browser, six jobs. Cargo, npm and both engines restored. | 11:45 | 45.2 | 2 / 3 s | 307 s |
| [Cold delivery](https://github.com/hbjoroy/sproyt/actions/runs/40768799498) | `982cc9a` / `73672da` | Full browser, audit and delivery. Browser caches missed and ARM64 dependencies compiled without reusable BuildKit layers. | 19:28 | 61.9 | 3 / 3 s | 433 s |
| [Scoped publication](https://github.com/hbjoroy/sproyt/actions/runs/42003483944) | `6147980` / `f1a6393` | Frontend, full browser, audit and delivery. Cargo, npm and browsers restored. BuildKit reused layers and rebuilt application code. | 12:21 | 50.2 | 5 / 4 s | 306 s |
| [UI probe, earlier policy](https://github.com/hbjoroy/sproyt/actions/runs/42037942044) | `8b2a36b` / `02fd556` | Frontend and full browser, six jobs. Cargo, npm and both engines restored. | 12:14 | 43.5 | 3 / 2 s | 280 s |
| [UI probe, policy at `861a903`](https://github.com/hbjoroy/sproyt/actions/runs/42037947947) | `5affa6c` / `861a903` | Frontend, invitation contracts and smoke in Chromium, touch smoke in WebKit. Cargo, npm and both engines restored. | 9:08 | 11.2 | 2 / 3 s | 468 s |
| [Backend weather probe](https://github.com/hbjoroy/sproyt/actions/runs/42037973984) | `ecdfc02` / `861a903` | Whole Rust and PostgreSQL passed. Cargo and npm restored in both jobs. | 5:58 | 8.7 | 3 / 6 s | 63 s |

The documentation sample met its 3 minute and 5 runner-minute targets. The
shared navigation PR illustrates the runner cost of full browser coverage.
The CSS and cold delivery runs serve as context for their respective paths.

The controlled UI comparison uses the same one-line comment in
`invitation-card.tsx` and identical application and test trees based on
`861a903`. The earlier-policy base restores the workflow and selector from
`c1fc39b`. The `861a903` policy uses #348 and #349. Both probes are PR
checks without publication.

Both UI runs passed their selected gates with the same cache keys restored.
Wall time fell by 3:06 (25%) and runner time by 32.4 minutes (74%). The
bounded sample met the 12 minute and 25 runner-minute targets. One pair
measures this invitation-card change. The median for bounded UI PRs needs
more samples. WebKit's install step took 367 seconds despite a browser cache
restore, and its selected test step took 18 seconds. The bounded selection
already uses one job per engine, with no extra per-engine shards to remove.
Measured setup accounted for 70% of this run's runner time. Retain both engine
checks and revisit dependency installation if this delay recurs.

The backend probe uses a comment-only change in `src/chatbot/weather.rs`.
The earlier selector evaluated locally against that path selects Rust,
PostgreSQL, codec and operator checks. The hosted run at `861a903` retained Rust and
PostgreSQL and explicitly excluded codec, operator, auth, recovery and browser
work. The earlier selection provides a gate comparison. The backend timing
sample covers the workflow at `861a903`.

## Delivery verification evidence

- **Publication and promotion.** The scoped publication above checked the
  complete 12-file interval at the exact merged revision and recorded immutable
  web and worker tags. PostgreSQL and recovery were explicitly excluded.
  [Canary #315](https://github.com/hbjoroy/rocket-applications/pull/315) and
  [production #316](https://github.com/hbjoroy/rocket-applications/pull/316)
  converged on the same chart pin and published web digest
  `sha256:bafe6e5da791a1dd840980f62e415404101f3c9d070f9675b9dafcc75820b80d`.
  Live Argo and public readiness/revision checks verified one Canary and two
  production web replicas. Canary's successful operation used an earlier
  revision with an identical Helm tree and converged on the expected pin.
  Both used the existing shared worker. The
  production GitOps change also advanced its separately verified migration pin
  for the existing shared schema.
- **Failure handling and baselines.** A
  [real publication attempt](https://github.com/hbjoroy/sproyt/actions/runs/41655794377)
  passed the browser shards but skipped publication after the worker image
  scan failed. The [next run](https://github.com/hbjoroy/sproyt/actions/runs/41659894821)
  passed after the [Expat fix](https://github.com/hbjoroy/sproyt/pull/328).
  Local evaluation of the browser gate shell and publication condition at `861a903`
  covered eight and 43 cases respectively, including selected failure,
  cancellation and unexpected skip. Local selector checks against actual Git
  history selected the bounded suite with a valid ancestor and full checks for
  missing, malformed, unavailable and non-ancestor baselines.
- **Weekly regression.** The
  [2 November scheduled run](https://github.com/hbjoroy/sproyt/actions/runs/41436299970)
  passed all six browser shards, Rust, PostgreSQL, recovery, codec, operations,
  dependency audit and ARM64 delivery. Both publish jobs skipped. This run
  predates #348 and #349. A later
  [broad publication](https://github.com/hbjoroy/sproyt/actions/runs/42036588378)
  ran the updated workflow for the 22-file interval from `6147980` to
  `861a903`. The selector chose all risk gates from the valid baseline.
  All gates and publication passed in 16:24 wall time and 62.7 runner-minutes.

The measurements above pin the policy before
[#367](https://github.com/hbjoroy/sproyt/pull/367), which subsequently separated
recovery selection from full application regression. Browser selection is
unchanged. At `97ec380`, missing or invalid baselines select full application
checks; recovery follows the independent decision described above.
Ten local Git/selector probes passed for this revision, including baseline
fallback, independent forced recovery and weekly recovery. The eight browser
gate cases and 43 publication-condition cases also passed: selected recovery
failure, cancellation or skip still blocks publication.
