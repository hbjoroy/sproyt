# Shared capacity decision, 20 November 2026

Operations owns the shared budget and can defer a Sprøyt change. Chat core owns query and actor latency. Retain the shared pool, bounded actor and embedded workers. The [isolated traffic results](database-capacity-assessment-2026-11-10.md) cover hot and quiet channels, background topic work and reconnects. All six 160/s runs passed. At 320/s all six missed the targets. The production population remains the operating basis; revisit sustained queue wait, timeouts or materially different traffic before changing architecture.

## PostgreSQL allocations

The instance has 100 slots, including three superuser-only and zero other reserved slots. Ordinary operations need two additional slots for intervention under load. The following amounts govern Sprøyt admission. They give other services planning allocations, without changing their role limits or asserting their owners accepted a quota.

| Consumer and accountable function | Ordinary planning allocation | Implementation and overlap |
| --- | ---: | --- |
| API and embedded work, chat core | 13 | Two production pools of 4, one canary pool of 2, and one dedicated listener per pod. A production replacement can temporarily retain both old pods, costing up to 10 extra. Canary costs 3. |
| Media, media operations | 2 | One Recreate worker, no listener. |
| Heart, process operations | 6 | Three pools of 2. Recreate in both environments. Two maintenance slots fit the enforced role limit 8. [Isolated rollout and bootstrap proof](heart-rollout-budget.md). |
| Authentik, identity operations | 14 | The gate preserves unused headroom to 14. The actual role is unlimited. Requests beyond 14 are included at actual use. |
| Grafana, observability | 10 | Configured pool 10, Recreate. Unused seats are borrowable. |
| Remaining clients, shared database operations acting for the connected service owners | 40 pooled | Includes Isdalen, Minoa, Merchant, other Heart consumers, backup, admin and smaller clients. This is a planning line, with no per-tenant forced disconnect. |
| Ordinary intervention reserve | 2 | Deducted at every admission. The three PostgreSQL superuser seats stay separate. |
| Normal uncommitted after the above | 10 | Covers two old production API pods. Simultaneous canary replacement additionally requires three borrowable idle seats. |

The normal client allocation totals 85. The measured Nov 19 peak was 97 existing clients, with zero ordinary login headroom. A peak outside the plan leads to a hold on new work. It does not revoke established user sessions. Fifteen of the sixteen non-superuser login roles had no hard role limit. An ordinary slot is consumed by any established client, including a logged-in superuser.

At 05:05 / 05:08 UTC on Nov 20 the operator counted 76 / 74 clients. The respective groups were API/media 14 / 13, Heart 6 / 6, identity 12 / 12, Grafana 4 / 4 and other 40 / 39. In a separate read-only ownership sample Isdalen used 15, Minoa 11, Merchant 3, its Heart 3, the superuser 3, and smaller roles 1–2 each. These are point counts, not contractual maxima. The previous [normal canary and production promotion](https://github.com/hbjoroy/rocket-applications/pull/352) retained both production API replicas throughout.

## Release admission

In the enabled Sprøyt Argo applications a PreSync job samples three times, five seconds apart, before backup and migrations (wave −15), and again after those hooks but before workload changes (wave −1). Use full sync for promotion and rollback. A selective sync skips Argo hooks and is outside this operating procedure.

The gate uses the minimum of the configured ordinary slots and 97, subtracts existing clients other than itself, then reserves unused API/media capacity to 15, Heart to 6 and identity to 14. With an ordinary login PostgreSQL hides other roles’ backend type. The gate counts database-attached processes and ambiguous role-backed processes conservatively. As a DBA the separate [operator query](../tools/capacity-postgres-budget.sql) distinguishes client backends. In the live comparison an ordinary-role filter on backend type alone would have seen 14 clients when the DBA saw 75. The production-role query with the correct visibility reported 19 adjusted slots at its follow-up sample.

| Phase | Increment above normal, before the two-seat reserve | Why |
| --- | ---: | --- |
| Backup, migration, bootstrap or healthy unchanged web | 2 | One database client per release and up to two releases. Backup dumps are serial, restore is local. The unchanged checkpoint path verifies local checksums. Heart init and main bootstrap containers run serially. Migration replacement waits for its predecessor to finish. |
| Changed or unfinished canary web | 4 | Three web seats plus a maintenance seat in the other environment. |
| Changed or unfinished production web | 13 | Two terminating production API pods and one canary replacement. A canary change alone cannot start two production replacements. |

The web decision uses the desired Deployment spec fingerprint, Ready/updated/available counts and image from a name-scoped Kubernetes read. A missing Deployment or unrecognised projection costs full overlap. A failed API/DB read, timeout or insufficient sample fails the hook. Recheck the last held phase, established clients and concurrent work before retry. Manual drift repair or emergency rollback also requires the operator’s live preflight; use an unchanged migration image when rolling back web. The readings govern admission at that instant. Other applications can grow after approval. A sustained breach goes to database and identity operations to reduce idle demand or reschedule changes. Do not raise the pool or disconnect another tenant to satisfy a gate.

## Compute and storage

Three nodes each offer four CPU and 8255904 KiB allocatable memory (roughly 24,187 MiB total). The following is the operating envelope shared with the platform. CPU is in cores, memory in MiB.

| Group | CPU | Memory | Basis and owner |
| --- | ---: | ---: | --- |
| Sprøyt runtime | 3.5 | 2816 | Three API pods at 0.5/512, three Heart at 0.5/256, one media at 0.5/512. Enforced per-pod limits. |
| Shared PostgreSQL | 0.5 | 1536 | Enforced, infrastructure. |
| Grafana | 0.75 | 768 | Enforced, observability. |
| Transient release work | 2 | 2048 | Two backup jobs at 1/768 each or a production web overlap at 1/1024 and a canary backup at 1/768. Keep heavy maintenance serial by default. Includes the smaller admission jobs (0.1/64 per job) in their own phase. |
| Node/incident reserve | 1.5 | 3072 | Preserve 0.5 core and 1 GiB per node, also during a single-site release. |
| Identity | 1.25 | 1536 | Planning allocation; Authentik has no observed pod limits. |
| Remaining cluster work | 2.5 | about 12,411 | Planning remainder for infrastructure and other tenant workloads, not a Kubernetes quota. |

Before authorising a normal release run `python tools/verify-capacity-resources.py --context <approved-context>`. It reads node requests, recent working-set/CPU samples, node conditions, node filesystem and the PostgreSQL PVC. Each schedulable node must cover the larger of observed usage and requests, plus one site's worst web/backup phase and node reserve. Hold maintenance when another heavy job can land on the same node. CPU limits across all pods are overcommitted. On 20 November, node CPU requests were 1395/1660/1910m and memory requests 1478/1838/2808 MiB. The check showed 2065–2605m CPU, 3504–5407 MiB memory and 163–184 GiB node filesystem available. The enforced scheduler requests for an API pod are 50m/64 MiB; these alone cannot guarantee peak capacity.

| Storage owner | Budget and action |
| --- | --- |
| Database operations | 8 GiB PostgreSQL PVC; preserve at least 2 GiB filesystem free for WAL and maintenance. Observed 3.82 GiB free, 192 MiB currently in WAL and 2750 MiB Sprøyt DB size. An extra 1 GiB of WAL between checks, a downward trend projected to cross the floor before the next maintenance, or a failed PVC sample triggers investigation. Restrict further high-I/O work until cleared. |
| Release and backup operations | 32 GiB production and canary release claims plus an 8 GiB canary claim. For each new checkpoint reserve at least the existing hook's 2 GiB floor and the projected new dump size plus 1 GiB. Verify actual mounted free space before a fresh dump; detached volume allocation is not filesystem use. The hook still enforces its own free-space and checksum gates. Run one serial dump stream per environment and avoid overlapping fresh checkpoints with the database backup at 03:00. Its CronJob forbids self-overlap; last success was Nov 20 02:06 UTC. |
| Node storage and Longhorn | Allow 12 GiB temporary restore plus 2 GiB node reserve; media scratch has its own 2 GiB ceiling. Longhorn requires 25% physical free and permits 110% logical scheduling. On Nov 20 logical margins after configured reservations were about 0.60/4.84/16.84 GiB per node. Validate placement for **every replica** before any new PVC or expansion. Physical free alone cannot admit new replicas. |
| Infrastructure backup | The shared database cron writes a separate host-mounted target. Its free-space and retention policy remain with infrastructure. Sprøyt's release budget does not spend that target; a change affecting its retention, IO concurrency or capacity requires a new infrastructure measurement first. |

Review the envelope each quarter and whenever replica counts, pool limits, shared consumers, WAL or backup growth, storage replication, or traffic changes. Architecture operations owns the decision and holds new Sprøyt work when inputs are stale. No new broker, split or increase to shared database settings is required for the measured workload.
