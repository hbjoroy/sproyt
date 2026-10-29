# Faster CI iterations

The previous release, [40751547076](https://github.com/hbjoroy/sproyt/actions/runs/40751547076),
spent 1,578 seconds in Chromium and 739 seconds in WebKit, sequentially in the
quality job. ARM64 compilation took 377 seconds and ran independently. Reducing
the release-image build alone could not remove the roughly 39-minute browser wait.

## Execution and isolation

- Build/check the frontend once in quality. Do not repeat `npm test` through
  `test:e2e`, or build the same frontend again in a PR-only Docker stage.
- Build one real browser-test server from the same checkout. Transfer that
  binary and the built frontend as an artifact within the same workflow run
  (worker/update tests also read the JavaScript bundle from disk). Every browser job migrates
  its own temporary SQLite database and starts its own server on a reserved port.
  Local test commands retain their Cargo fallback; CI never compiles per shard.
- Full regression uses four Chromium and two WebKit shards concurrently.
  Keep one worker and sequential tests within a file: many tests share server
  state and cannot safely be made fully parallel. Shards isolate that state.
- Install only the engine used by that job. Keep traces/reports on failures and
  machine-readable timing reports on every run, with the slowest cases in the
  job summary. Use these measurements before rewriting slow tests.
- Cache Rust dependency artifacts with a pinned rust-cache action, shared by
  quality, PostgreSQL and recovery jobs. Exclude workspace/incremental artifacts;
  only main writes, while PRs restore. Do not create a multi-GiB archive per commit.
- Keep separate ARM64 web/worker BuildKit indexes with max-mode intermediate
  layers. Import the previous `arm64` scope while the new indexes warm up.

## Selection

`tools/ci-scope.py` selects from the entire ancestor-to-head range. The release
baseline must be the last verified application revision. Missing or invalid
baselines, unknown paths, version tags, weekly runs and `force_full` keep full
regression. Deletions and both sides of renames remain visible to selection.

| Change | Browser checks | Rust/PostgreSQL and restore |
| --- | --- | --- |
| Documentation / UI skill bundle | None | None |
| Frontend unit tests only | Unit/type/build checks | None |
| Browser spec only | Exact changed specs + core smoke | None |
| CSS | Layout and key cross-browser interactions | None |
| Declared isolated frontend feature | Feature union + core smoke | None |
| Shared frontend, package/runtime glue | Full, sharded | Only if backend is also affected |
| Backend feature | Browser only for shared web/protocol contracts | Rust + PostgreSQL; restore for persistence/runtime changes |
| Unknown build/config/dependency change | Full, sharded | Full |

Suite files live in `tools/browser-suites.json`; the previous mobile WebKit list
is preserved there. Feature mappings are conservative exact paths, not guessed
import reachability. Changes to shared application ownership/state still use
full regression. Combining two features takes the union, never the smaller set.
Missing specs fail selection; an empty related selection cannot become an
implicit "run everything" or a successful "run nothing".

## Publication and cadence

PRs provide iteration feedback. A merge no longer starts a third, duplicate
automatic main-push workflow immediately before the explicit main publish run.
The canonical publish run still validates the exact merged revision; this change
does not reuse a PR's green status as release evidence. Production promotion
continues to use the verified Canary digest without rebuilding.

The aggregate browser gate requires success from every selected shard. A failed,
cancelled or unexpectedly skipped shard blocks publication. PostgreSQL and restore
may skip only on an explicit scope decision. ARM64 identity, Helm, SBOM and
security scanning remain publication gates. Scheduled full regression catches
interactions outside the bounded fast suites.

The expected gain is from concurrent and smaller suites, not weaker assertions,
automatic retries or larger timeouts. More concurrent runners trade some setup
minutes for lower wall time. Initial targets: a narrow UI iteration should be
limited mainly by server preparation; full browser regression should be limited
by its slowest shard, not the sum of both engines. Record actual CI timings below
before claiming a numerical speedup.

References: [Playwright sharding](https://playwright.dev/docs/test-sharding) and
[CI isolation guidance](https://playwright.dev/docs/ci).

## Cache incident, 29 October 2026

Release [40768799498](https://github.com/hbjoroy/sproyt/actions/runs/40768799498)
had no reusable ARM64 layers at its start. The dependency compile took 7m04s,
application compile 4m35s, and cache export 2m06s. Before the build, the API
listed only ordinary Actions caches; BuildKit indexes/blobs reappeared after export.
Two main Cargo archives alone held about 6.6 GiB (3,299 and 3,459 MiB), alongside
PR caches. This is consistent with eviction under shared storage pressure; the
API does not retain an eviction event proving the exact order. Per-revision
whole-target caching aggravated that pressure and is replaced here.

Keep the current ARM64 cache when removing obsolete whole-target Cargo archives.
Record restored cache keys, archive sizes and cached dependency steps from the
next real release. Do not claim a warm-build speedup solely from configuration.

References: [GitHub cache eviction](https://docs.github.com/en/actions/using-workflows/caching-dependencies-to-speed-up-workflows),
[Rust dependency cache cleanup](https://github.com/Swatinem/rust-cache#cache-details),
[BuildKit cache scopes](https://docs.docker.com/build/cache/backends/gha/#scope).
