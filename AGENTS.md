# Teleaf release requirements

For every stable Teleaf release, push the fixes to `YoisakiKnd/teleaf` and
update the verified manifest in `YoisakiKnd/scoop-teleaf`, `bucket/teleaf.json`.

On 2026-10-04 the user explicitly replaced the previous two-bucket policy:
do not create or update release PRs in `Mythos-404/eimer` unless separately
requested. Existing eimer PRs are left unchanged.

Verify the release assets and use the actual version, Windows release URL,
SHA-256 and manifest metadata in the Scoop bucket. Record the main release and
Scoop synchronization results in the release report.

The project uses MIT, selected by the user. Set Scoop and Homebrew license
metadata to `MIT`. Preserve the separate licenses of bundled third-party
runtimes.
