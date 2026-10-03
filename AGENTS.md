# Teleaf release requirements

For every stable Teleaf release, submit the verified Scoop manifest to both:

1. `YoisakiKnd/scoop-teleaf`, path `bucket/teleaf.json`.
2. `Mythos-404/eimer`, path `bucket/teleaf.json`, through a pull request.

The user explicitly requested this ongoing release policy. Creating or updating
the corresponding eimer manifest PR is authorized as part of a release task.
Check for an existing open Teleaf PR before creating another; update it when
appropriate. Do not merge a third-party PR without separate authorization.

Use the same version, Windows release URL, SHA-256 and manifest metadata in both
buckets. Verify the release assets before submission. The eimer submission is
complete when the current manifest is in a submitted PR; acceptance and merging
remain with its maintainers. Record the PR URL in the release report. Never
describe an unmerged manifest as already available from eimer.

The project uses MIT, selected by the user. Set Scoop and Homebrew license
metadata to `MIT`. Preserve the separate licenses of bundled third-party
runtimes.
