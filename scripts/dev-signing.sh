#!/usr/bin/env bash
# Stable local code signature for target/bundle/adoken.app.
#
# Why: the Keychain remembers "Always Allow" per code-signing requirement. An
# ad hoc signature's requirement is the binary's hash, so every rebuild looks
# like a new app and macOS asks for the Keychain password again. Signing with
# one self-signed certificate keeps the requirement stable across rebuilds.
#
# What it does (idempotent; bundle.sh runs it automatically):
#   - a private keychain ~/Library/Keychains/adoken-dev.keychain-db whose
#     password lives in ~/.config/adoken/dev-keychain-password (0600), so it
#     unlocks without prompts;
#   - a self-signed "adoken dev" code-signing certificate inside it;
#   - trust for code signing: the one step macOS guards with your password,
#     asked once, the first time only.
#
# Usage: scripts/dev-signing.sh            set up (no-op when ready)
#        scripts/dev-signing.sh --keychain print the keychain path (for codesign)
#        scripts/dev-signing.sh --remove   delete the keychain, trust and password
#
# Local development only. Distribution uses Developer ID (docs/release.md).

set -euo pipefail

NAME="adoken dev"
KC="$HOME/Library/Keychains/adoken-dev.keychain-db"
PASS_FILE="$HOME/.config/adoken/dev-keychain-password"

unlock() {
  security unlock-keychain -p "$(cat "$PASS_FILE")" "$KC"
  # Stay unlocked; codesign must never prompt.
  security set-keychain-settings "$KC"
}

ready() {
  [[ -f "$KC" && -f "$PASS_FILE" ]] || return 1
  unlock
  security find-identity -v -p codesigning "$KC" | grep -q "\"$NAME\""
}

case "${1:-}" in
  --keychain)
    ready || { echo "adoken dev identity not set up: run scripts/dev-signing.sh" >&2; exit 1; }
    echo "$KC"
    exit 0
    ;;
  --remove)
    if [[ -f "$KC" ]]; then
      cert="$(mktemp)"
      security find-certificate -c "$NAME" -p "$KC" >"$cert" 2>/dev/null &&
        security remove-trusted-cert "$cert" 2>/dev/null || true
      rm -f "$cert"
      security delete-keychain "$KC"
    fi
    rm -f "$PASS_FILE"
    echo "removed the adoken dev signing identity"
    exit 0
    ;;
  "") ;;
  *)
    sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac

if ready; then
  exit 0
fi

command -v openssl >/dev/null || { echo "openssl is required" >&2; exit 1; }

# Start clean if a previous run stopped halfway.
[[ -f "$KC" ]] && security delete-keychain "$KC"
mkdir -p "$(dirname "$PASS_FILE")"
umask 077
openssl rand -hex 24 >"$PASS_FILE"
security create-keychain -p "$(cat "$PASS_FILE")" "$KC"
unlock
# Add it to the search list (keeping the existing ones) so Keychain Access shows it.
existing=()
while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"
  existing+=("${line//\"/}")
done < <(security list-keychains -d user)
security list-keychains -d user -s "${existing[@]}" "$KC"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$NAME" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  -addext "basicConstraints=critical,CA:false" \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
legacy=()
openssl version | grep -q '^OpenSSL 3' && legacy=(-legacy)
openssl pkcs12 -export "${legacy[@]}" -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
  -name "$NAME" -out "$tmp/id.p12" -passout pass:adoken
security import "$tmp/id.p12" -k "$KC" -P adoken -T /usr/bin/codesign >/dev/null
# Let codesign use the key without a GUI prompt (we know this keychain's password).
security set-key-partition-list -S apple-tool:,apple:,codesign: -s \
  -k "$(cat "$PASS_FILE")" "$KC" >/dev/null

echo "Trusting the \"$NAME\" certificate for code signing."
echo "macOS will ask for your password once (this is the only prompt, ever)."
security add-trusted-cert -r trustRoot -p codeSign -k "$KC" "$tmp/cert.pem"

ready || { echo "setup finished but the identity isn't valid for code signing" >&2; exit 1; }
echo "adoken dev signing identity ready. Rebuilds keep the same signature;"
echo "click \"Always Allow\" the next time adoken asks for its Keychain item."
