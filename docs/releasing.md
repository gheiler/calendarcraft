# Releasing CalendarCraft

A push to `main` starts the *Tag and release* workflow. It reads the version in
`Cargo.toml`. If the annotated tag `v<version>` is not already on the repository, it
creates that tag on the commit that was pushed and publishes a GitHub release. If the
tag already exists, it is not moved; if the release already exists, nothing is
published again. So a later merge that forgot to bump the version will not overwrite
v0.3.0.

You can still push a version tag (`v0.3.0`) yourself from a commit whose `Cargo.toml`
version matches. That starts the same release jobs. Tags created by `GITHUB_TOKEN`
do not start other workflows, which is why *Tag and release* calls the release
workflow directly rather than waiting for the tag push.

The release build is `calendarcraft-web-<version>.zip`, plus `SHA256SUMS.txt` (one
`sha256sum` line for the zip), signed with CalendarCraft's ed25519 release key. The
GitHub release then holds the zip, the checksums, and `SHA256SUMS.txt.sig`.

The signature is a raw ed25519 signature over the exact bytes of `SHA256SUMS.txt`, written as
standard base64 plus a trailing newline — the same format artcraftCloud verifies for MailCraft
(see artcraftCloud's `docs/registry.md`). The private key lives in the repository secret
`CALENDARCRAFT_RELEASE_SIGNING_KEY` (base64 PKCS#8). It is never printed or committed.

After a new GitHub release, artcraftCloud still needs the version added to its app
registry (`registry add-version calendarcraft <version> --recommend`) in the
artcraft-cloud repository. That step is not done from this repository.

## Public keys

These are not secret. artcraftCloud should pin them under `keys.ed25519` for CalendarCraft.
The first key signs releases; the second is a spare for a later rotation.

- in use: `Hv8y6mNjnmRqTlAcnJl1v/jCiRyCVsHUi9/fT1T629E=`
- spare: `jF5sjrTxNhzH0nffbxO2SXoeITH3UsofUPTEu9IKPEk=`

## Signing an existing checksums file locally

```sh
CALENDARCRAFT_RELEASE_SIGNING_KEY=$(cat /path/to/calendarcraft-release-key-in-use.pkcs8.b64) \
  bash scripts/sign-sums.sh dist-release
```

`bash scripts/sign-sums.sh --self-test` checks the script with a throwaway key (used in CI).
