# Teleaf release requirements

For every stable Teleaf release, push the fixes to `YoisakiKnd/teleaf` and
update the verified manifest in `YoisakiKnd/scoop-bucket`, `bucket/teleaf.json`.
The bucket was renamed from `YoisakiKnd/scoop-teleaf`; the old URL redirects.

Since 2026-10-10, Homebrew distribution uses `YoisakiKnd/homebrew-tap`,
`Formula/teleaf.rb` (tap name `YoisakiKnd/tap`). For every stable release,
also sync and verify that formula against the actual macOS/Linux release
assets. The main repository only publishes `teleaf.rb` as a Release asset;
do not recreate a live `Formula/teleaf.rb` here. Keep `tap_migrations.json`
so existing users of the old tap can migrate. Record Homebrew synchronization
alongside the main release and Scoop results.

On 2026-10-04 the user explicitly replaced the previous two-bucket policy:
do not create or update release PRs in `Mythos-404/eimer` unless separately
requested. Existing eimer PRs are left unchanged.

Verify the release assets and use the actual version, Windows release URL,
SHA-256 and manifest metadata in the Scoop bucket. Record the main release and
Scoop synchronization results in the release report.

The project uses MIT, selected by the user. Set Scoop and Homebrew license
metadata to `MIT`. Preserve the separate licenses of bundled third-party
runtimes.
