#!/bin/bash
set -e

# Check if token is already initialized (match exact label field to avoid false positives)
if ! softhsm2-util --show-slots | grep -qE "^[[:space:]]*Label:[[:space:]]*${PKCS11_SLOT_TOKEN_LABEL}[[:space:]]*$"; then
  echo "Initializing SoftHSM token..."
  softhsm2-util --init-token --free --label "${PKCS11_SLOT_TOKEN_LABEL}" \
    --so-pin "${PKCS11_SO_PIN}" --pin "${PKCS11_USER_PIN}"
else
  echo "SoftHSM token already initialized, skipping..."
fi

# Execute the main command
exec "$@"
