#!/usr/bin/env bash
# Sign a release's SHA256SUMS.txt with CalendarCraft's ed25519 release key: writes
# SHA256SUMS.txt.sig beside it (standard base64 of a raw ed25519 signature over the file's
# exact bytes, plus a trailing newline). artcraftCloud verifies this the same way it verifies
# MailCraft's SHA256SUMS.txt.sig (docs/registry.md).
#
#   CALENDARCRAFT_RELEASE_SIGNING_KEY=… bash scripts/sign-sums.sh <folder with SHA256SUMS.txt>
#   bash scripts/sign-sums.sh --self-test
#
# The key is base64 PKCS#8 (as written under calendarcraft-release-keys) and comes only through
# the environment; it is never printed or written to a file. The signature is made by OpenSSL 3
# (`openssl pkeyutl -sign -rawin`), not by a program built from this repository.
set -euo pipefail

openssl3() {
  command -v openssl >/dev/null 2>&1 && openssl version 2>/dev/null | grep -Eq '^OpenSSL [3-9]\.'
}

# Hex on stdin, as bytes on stdout (printf is the shell's own: nothing goes on a command line).
unhex() {
  local escaped
  escaped="$(sed -e 's/../\\x&/g')"
  # shellcheck disable=SC2059 # the format is \xHH escapes of checked hex and nothing else
  printf "$escaped"
}

# The key's DER in hex, from the environment variable named by $1.
key_hex() {
  printf '%s' "${!1}" | tr -d '[:space:]' | base64 -d 2>/dev/null | od -An -v -tx1 | tr -d ' \n'
}

# The PKCS#8 v1 DER of the key whose seed is in KEY_SEED. OpenSSL 3.0 doesn't always read the
# v2 form (the seed plus the public half), so the key goes to OpenSSL as v1, through a pipe.
v1_der() {
  printf '%s' "302e020100300506032b657004220420$KEY_SEED" | unhex
}

# sign <file> <variable holding the key>: <file>.sig, and the public key said.
sign_with_openssl() {
  local file="$1" var="$2" hex embedded="" public spki bad
  bad="$var isn't an ed25519 private key in base64 PKCS#8"
  hex="$(key_hex "$var" || true)"
  KEY_SEED=""
  # Two layouts and no other: v2 (83 bytes: version 1, the seed, then the public half as
  # [1] 33 bytes), and v1 (48 bytes: version 0 and the seed).
  if [[ ${#hex} == 166 ]] && [[ ${hex:0:32} == 3051020101300506032b657004220420 ]] && [[ ${hex:96:6} == 812100 ]]; then
    KEY_SEED="${hex:32:64}"
    embedded="${hex:102:64}"
  elif [[ ${#hex} == 96 ]] && [[ ${hex:0:32} == 302e020100300506032b657004220420 ]]; then
    KEY_SEED="${hex:32:64}"
  fi
  hex=""
  case "$KEY_SEED" in
    "" | *[!0-9a-f]*)
      KEY_SEED=""
      echo "$bad" >&2
      exit 1
      ;;
  esac
  spki="$(v1_der | openssl pkey -inform DER -pubout -outform DER 2>/dev/null | od -An -v -tx1 | tr -d ' \n' || true)"
  if [[ ${#spki} != 88 ]] || [[ ${spki:0:24} != 302a300506032b6570032100 ]]; then
    KEY_SEED=""
    echo "$bad" >&2
    exit 1
  fi
  if [[ -n "$embedded" ]] && [[ ${spki:24:64} != "$embedded" ]]; then
    KEY_SEED=""
    echo "$var carries a public key that isn't its own" >&2
    exit 1
  fi
  openssl pkeyutl -sign -rawin -keyform DER -inkey <(v1_der) -in "$file" | base64 | tr -d '\n' >"$file.sig.part"
  KEY_SEED=""
  echo >>"$file.sig.part"
  mv "$file.sig.part" "$file.sig"
  public="$(printf '%s' "${spki:24:64}" | unhex | base64)"
  echo "signed $file with the release key $public: $file.sig"
}

self_test() {
  local scratch key out public
  scratch="$(mktemp -d)"
  openssl genpkey -algorithm Ed25519 -outform DER -out "$scratch/key.der" >/dev/null 2>&1
  key="$(base64 -w0 < "$scratch/key.der" 2>/dev/null || base64 < "$scratch/key.der" | tr -d '\n')"
  rm -f "$scratch/key.der"
  printf 'd1593248151f220671836e69b8c38dfb04214823a05d33c78dc6d15bab2b6f08  calendarcraft-web-0.2.0.zip\n' >"$scratch/SHA256SUMS.txt"
  out="$(CALENDARCRAFT_RELEASE_SIGNING_KEY="$key" sign_with_openssl "$scratch/SHA256SUMS.txt" CALENDARCRAFT_RELEASE_SIGNING_KEY)"
  key=""
  echo "$out"
  public="$(printf '%s\n' "$out" | sed -n 's/^signed .* with the release key \(.*\): .*$/\1/p')"
  bytes=$(wc -c < "$scratch/SHA256SUMS.txt.sig")
  if [[ "$bytes" -ne 89 ]]; then
    echo "self-test: SHA256SUMS.txt.sig must be 88 base64 characters and a newline (got $bytes bytes)" >&2
    rm -rf "$scratch"
    exit 1
  fi
  tr -d "\n" < "$scratch/SHA256SUMS.txt.sig" | base64 -d >"$scratch/sig.bin"
  if [[ "$(wc -c < "$scratch/sig.bin")" -ne 64 ]]; then
    echo "self-test: signature is not 64 bytes" >&2
    rm -rf "$scratch"
    exit 1
  fi
  printf '%s' "$public" | base64 -d >"$scratch/pub.raw"
  if [[ "$(wc -c < "$scratch/pub.raw")" -ne 32 ]]; then
    echo "self-test: public key is not 32 bytes" >&2
    rm -rf "$scratch"
    exit 1
  fi
  printf '%s' "302a300506032b6570032100$(od -An -v -tx1 <"$scratch/pub.raw" | tr -d ' \n')" | unhex >"$scratch/pub.der"
  openssl pkey -inform DER -pubin -in "$scratch/pub.der" -out "$scratch/pub.pem"
  openssl pkeyutl -verify -rawin -pubin -inkey "$scratch/pub.pem" -in "$scratch/SHA256SUMS.txt" -sigfile "$scratch/sig.bin" >/dev/null
  if openssl pkeyutl -verify -rawin -pubin -inkey "$scratch/pub.pem" -in <(printf 'tampered') -sigfile "$scratch/sig.bin" >/dev/null 2>&1; then
    echo "self-test: tampered message verified" >&2
    rm -rf "$scratch"
    exit 1
  fi
  echo "self-test: signature verifies, and not over other bytes"
  rm -rf "$scratch"
  echo "self-test passed"
}

if [[ "${1:-}" == "--self-test" ]]; then
  if ! openssl3; then
    echo "OpenSSL 3 is needed to sign checksums" >&2
    exit 1
  fi
  self_test
  exit 0
fi

DIR="${1:?usage: scripts/sign-sums.sh <folder with SHA256SUMS.txt>}"
SUMS="$(cd "$DIR" && pwd)/SHA256SUMS.txt"

if [[ ! -f "$SUMS" ]]; then
  echo "$SUMS isn't there" >&2
  exit 1
fi
rm -f "$SUMS.sig"
if [[ -z "${CALENDARCRAFT_RELEASE_SIGNING_KEY:-}" ]]; then
  echo "CALENDARCRAFT_RELEASE_SIGNING_KEY isn't set; refusing to publish an unsigned release" >&2
  exit 1
fi
if ! openssl3; then
  echo "OpenSSL 3 is needed to sign checksums" >&2
  exit 1
fi
sign_with_openssl "$SUMS" CALENDARCRAFT_RELEASE_SIGNING_KEY
