# GitHub issue export: setup and implementation

The owner selected **hbjoroy/sproyt** for the initial manual export pilot,
instead of a separate test repository. GitHub reports repository ID
`1272543078`, issues enabled and owner administration access. The repository
is **public**. This is an explicit destination choice, not authorization to
publish arbitrary channel history or private attachments.

The application record `sproyt` belongs to Sprøyt - apptilbakemeldingar.
Testkanal is enabled for work-item review 1.1.0. GitHub export is not implemented
or enabled yet. The existing `github_repository_id` and `can_export` fields
are a foundation, not a working connection.

## Owner's GitHub steps

Register an app at <https://github.com/settings/apps/new> under the hbjoroy
account, with these reviewed settings:

| Setting | Value |
| --- | --- |
| App name | `Sproyt Issues hbjoroy` (GitHub requires a globally unique name) |
| Description | `Manual export of reviewed Sprøyt work items to approved GitHub repositories.` |
| Homepage | `https://sproyt.bjoroy.me` |
| User authorization / OAuth / device flow | Not requested for this installation-token MVP |
| Callback and setup URLs | Blank for the initial operator-assisted setup |
| Webhook Active | Off; the first export is one-way |
| Repository permissions: Issues | Read and write |
| Repository permissions: Metadata | Read-only, as required by GitHub |
| Other repository / organization / account permissions | No access |
| Where can this app be installed? | Only on this account |

The initial recommendation was **Install App → hbjoroy → Only select
repositories → hbjoroy/sproyt**. The owner explicitly chose **All repositories**
on the hbjoroy account to support future applications. Record
the public App ID and Installation ID. Generate a private key from the app's
settings and keep the downloaded PEM file outside the repository. Do not
paste its contents into chat, Git, issue descriptions or configuration files.
The owner performs credential creation and repository permission approval.

On 2026-10-02 the owner registered `sproyt-issues-hbjoroy`, App ID
`5160764`, generated its first private key and installed the app on hbjoroy,
Installation ID `167150170`. The installed settings were verified in GitHub:
all repositories, Issues read/write and Metadata read-only. The wider installation does not authorize arbitrary
Sprøyt applications to export into those repositories; each application needs
its own approved binding. Installation alone does not enable export.

At 12:33 Europe/Bucharest on 2026-10-02 the supplied RSA private key was
validated and imported into `sproyt-canary/sproyt-github-app` on Kubernetes
context `default`. A read-back comparison verified the exact local key and
App ID without printing credential values. No existing Secret was replaced,
no application rollout occurred and export remains disabled.

An authentication probe verified the App and hbjoroy installation, then obtained
a token restricted to repository ID `1272543078`, Issues write and Metadata
read. Reading `hbjoroy/sproyt` confirmed the exact repository and enabled issues.
No issue was created; the probe token was explicitly revoked afterwards.
This verifies credential and declared permission configuration, not yet the
end-to-end export feature.

The dedicated Secret is manually managed and is not yet mounted by a workload.
Its setup rollback is removal of that unused Secret; local key recovery is
available from the owner-held PEM, or by generating a replacement in GitHub.
Neither operation needs a database restore. Do not remove it after deployment
plumbing starts without checking its consumers. Unused GitHub keys can be
revoked by the owner once the retained key's fingerprint is identified.

Official references:
- [Registration](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app)
- [Installation](https://docs.github.com/en/apps/using-github-apps/installing-your-own-github-app)
- [Private keys](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps)
- [Installation tokens](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app)

## Store the existing credential

After the owner provides the App ID and local PEM path, use
`tools/Add-SproytGitHubAppSecret.ps1`. Start with `-WhatIf`, then import to
the dedicated `sproyt-github-app` Secret in `sproyt-canary` when authorized.
The helper validates the RSA private key locally, uses explicit Kubernetes
context/timeout and server-side apply, and never prints or writes the key
or credential-bearing manifest. It does not change the application's main
Secret, deploy an image, mount the key, or enable export.

No database migration is needed to import this independent credential. The
export implementation below requires its own additive migrations, CI/release
gates and canary delivery. Do not set `can_export=true` prematurely.

## Next implementation slices (step 4)

1. **GitHub client and deployment plumbing.** Load App ID/key from a dedicated
   Secret, sign short-lived JWTs, acquire installation tokens and renew before
   expiry. Restrict all requests to the GitHub API with explicit timeouts and
   no credential-bearing redirect. Do not log keys, tokens or issue bodies.
   Verify installation access and Issues write permission without creating an
   issue. Mock authentication/expiry/revocation/error paths in tests.
2. **Authorized repo binding.** Persist installation/repository IDs and a
   verified display name. For the initial operator-assisted setup, approve
   only application `sproyt` → `hbjoroy/sproyt`; installation credentials
   alone must not let another circle owner bind their application to this
   repo. Add an owner-only UI showing the approved connection and processor
   export permissions. Reject stale, revoked and cross-circle configuration.
   Future self-service installations need a state-bound ownership proof.
3. **Durable manual export.** Add a persisted export receipt with approved
   title/body, exact repository, actor, expected revision, request key, stable
   marker, outcome and returned issue ID/URL. Recheck current review/export
   permissions, channel access, application binding and eligible case status.
   Keep the internal case if GitHub is unavailable. On an uncertain create
   response, reconcile the marker before another POST; stop blind retries
   when outcome cannot be established. Never attach private media or export
   internal notes/channel history automatically.
4. **Compact review-card UI.** Provide an explicit **Send til GitHub** action
   on an eligible reviewed case, show the public destination and editable
   final text before submission, preserve drafts on error and show queued,
   sent/link, failed or uncertain status. Historical completed review cards
   must remain usable for the two cases already registered. Others see only
   what their current access/export policy permits. Use the Sprøyt UI skill.
5. **Acceptance and delivery.** SQLite/PostgreSQL receipt/authorization tests,
   mock GitHub accepted-response-loss and duplicate protection, compact
   Chromium/WebKit plus both themes. Full CI/CD before canary. Enable only
   the feedback application/reviewer after a read-only connection probe,
   then let the owner review and submit one deliberate real issue. Keep
   GitHub-to-Sprøyt status synchronization and automatic development off.

Disabling export blocks new submissions while preserving cases, accepted
requests and GitHub links for reconciliation. Application rollback keeps
forward migrations; never restore over later chat messages as a routine
rollback. Rotate/revoke a compromised App key through GitHub and update the
dedicated Secret; do not embed credentials in the release image.
