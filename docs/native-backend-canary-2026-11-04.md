# Backend for Windows ARM64 Canary testing

This release extracts the already tested server changes from native PRs #330
and #334, on current main `e3ac63e9c751289c7f6d1b5f0c17ba13b5cfee5d`.
The desktop UI, its local Rust store and other native feature work remain in
their existing draft PR stack.

It adds native grant/account-fenced notification preferences and resumable
media uploads through the existing domain services. PDF, ZIP, UTF-8 text and
CSV are validated and served as download-only attachments; React and legacy
web recipients receive explicit download links. Existing image/video behavior
and browser authorization remain covered by the regression suite.

The intended rollout is Canary only. Native is already enabled there and
remains disabled in production. No OIDC client, provider grant sharing,
database migration, media quota or shared preview-worker change is required.
Documents never enqueue image preview jobs. Preserve the separately pinned
schema-0070 migration image and the shared production worker.

Publish the exact reviewed main revision through the canonical CI release gate,
then pin the resulting immutable application digest through Canary GitOps.
Record the previous application digest for rollback. If document data has been
created during testing, an older reader may not display it; retain that data
and prefer a forward fix for document compatibility.

Verify public revision/health/readiness, native discovery and unauthenticated
rejection of the new routes. These do not establish authenticated end-to-end
acceptance. The Windows test package uses an explicit separate local profile;
the user signs in through the system browser and can test in a private channel.
No automated messages are sent to real users by the release verification.

The local cluster API is currently unreachable. Internal Argo/replica checks
must be reported separately from GitOps CI and public runtime verification.
