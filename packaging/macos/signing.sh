#!/usr/bin/env bash
# Developer ID signing material for the macOS release job (docs/releasing.md, "macOS signing"):
#
#   packaging/macos/signing.sh --setup     import the certificate into a temporary keychain and write
#                                          the notarization API key; export MACOS_SIGN_IDENTITY and
#                                          APPLE_API_* to later steps through $GITHUB_ENV
#   packaging/macos/signing.sh --cleanup   delete the keychain and the key
#
# --setup reads APPLE_CERTIFICATE_P12 (base64 .p12), APPLE_CERTIFICATE_PASSWORD, APPLE_API_KEY_P8,
# APPLE_API_KEY_ID and APPLE_API_ISSUER from the environment, and needs RUNNER_TEMP, GITHUB_ENV and
# GITHUB_STEP_SUMMARY. Without the certificate it does nothing, so forks and dry runs keep the
# ad-hoc signature that package.sh falls back to.
set -euo pipefail

KEYCHAIN="${RUNNER_TEMP:-}/signing.keychain-db"
API_KEY="${RUNNER_TEMP:-}/AuthKey.p8"

setup() {
  : "${RUNNER_TEMP:?}" "${GITHUB_ENV:?}" "${GITHUB_STEP_SUMMARY:?}"
  local p12="${APPLE_CERTIFICATE_P12:-}" password="${APPLE_CERTIFICATE_PASSWORD:-}"
  if [ -n "$p12$password" ] && { [ -z "$p12" ] || [ -z "$password" ]; }; then
    echo "::error::only one of APPLE_CERTIFICATE_P12 and APPLE_CERTIFICATE_PASSWORD is set"
    exit 1
  fi
  if [ -z "$p12" ]; then
    echo "The APPLE_CERTIFICATE_* secrets aren't set: the app is ad-hoc signed and not notarized" | tee -a "$GITHUB_STEP_SUMMARY"
    exit 0
  fi
  local keychain_password identity
  keychain_password="$(openssl rand -hex 24)"
  echo "::add-mask::$keychain_password"
  printf '%s' "$p12" | base64 --decode >"$RUNNER_TEMP/certificate.p12"
  security create-keychain -p "$keychain_password" "$KEYCHAIN"
  security set-keychain-settings -lut 21600 "$KEYCHAIN"
  security unlock-keychain -p "$keychain_password" "$KEYCHAIN"
  security import "$RUNNER_TEMP/certificate.p12" -P "$password" -A -t cert -f pkcs12 -k "$KEYCHAIN"
  security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$KEYCHAIN" >/dev/null
  # shellcheck disable=SC2046 # the keychain paths have no spaces on the runner
  security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')
  rm -f "$RUNNER_TEMP/certificate.p12"
  identity="$(security find-identity -v -p codesigning "$KEYCHAIN" | awk '/Developer ID Application/ { print $2; exit }')"
  if [ -z "$identity" ]; then
    echo "::error::the certificate has no Developer ID Application identity"
    exit 1
  fi
  echo "MACOS_SIGN_IDENTITY=$identity" >>"$GITHUB_ENV"
  if [ -n "${APPLE_API_KEY_P8:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
    printf '%s\n' "$APPLE_API_KEY_P8" >"$API_KEY"
    {
      echo "APPLE_API_KEY_PATH=$API_KEY"
      echo "APPLE_API_KEY_ID=$APPLE_API_KEY_ID"
      echo "APPLE_API_ISSUER=$APPLE_API_ISSUER"
    } >>"$GITHUB_ENV"
    echo "The macOS app is signed with the Developer ID and notarized" >>"$GITHUB_STEP_SUMMARY"
  else
    echo "The APPLE_API_* secrets aren't set: the macOS app is signed but not notarized" | tee -a "$GITHUB_STEP_SUMMARY"
  fi
}

cleanup() {
  : "${RUNNER_TEMP:?}"
  security delete-keychain "$KEYCHAIN" || true
  rm -f "$API_KEY" "$RUNNER_TEMP/certificate.p12"
}

case "${1:-}" in
  --setup) setup ;;
  --cleanup) cleanup ;;
  *) echo "usage: $0 --setup | --cleanup" >&2; exit 2 ;;
esac
