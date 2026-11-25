# Releasing CalendarCraft

Push a version tag (`v0.2.1`) from a commit whose `Cargo.toml` version matches. The release
workflow builds `calendarcraft-web-<version>.zip`, writes `SHA256SUMS.txt` (one `sha256sum`
line for the zip), and signs that file with CalendarCraft's ed25519 release key. The GitHub
release then holds the zip, the checksums, and `SHA256SUMS.txt.sig`.

The signature is a raw ed25519 signature over the exact bytes of `SHA256SUMS.txt`, written as
standard base64 plus a trailing newline — the same format artcraftCloud verifies for MailCraft
(see artcraftCloud's `docs/registry.md`). The private key lives in the repository secret
`CALENDARCRAFT_RELEASE_SIGNING_KEY` (base64 PKCS#8). It is never printed or committed.

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
