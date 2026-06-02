#!/bin/bash

# SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
#
# SPDX-License-Identifier: EUPL-1.2

set -e

# The SoftHSM token directory is mounted from a ReadWriteMany PVC shared by all
# StatefulSet replicas, so multiple pods can race on first-start init (token
# creation + wrap-key generation). Serialize that with a POSIX file lock on a
# file in the shared directory — flock blocks the loser until the winner is
# done, after which the winner has already initialized everything and the
# loser's existence checks skip both steps. CephFS supports flock.
LOCK_FILE=/myuser/softhsm/tokens/.init.lock

(
  flock -x 200

  # Check if token is already initialized (match exact label field to avoid false positives)
  if ! softhsm2-util --show-slots | grep -qE "^[[:space:]]*Label:[[:space:]]*${PKCS11_SLOT_TOKEN_LABEL}[[:space:]]*$"; then
    echo "Initializing SoftHSM token..."
    softhsm2-util --init-token --free --label "${PKCS11_SLOT_TOKEN_LABEL}" \
      --so-pin "${PKCS11_SO_PIN}" --pin "${PKCS11_USER_PIN}"
  else
    echo "SoftHSM token already initialized, skipping..."
  fi

  # Create the AES wrapping key if not already present
  echo "Ensuring AES wrapping key '${PKCS11_WRAP_KEY_ALIAS}' exists..."
  /usr/local/bin/digg-hsm-keytool \
    --hsm-lib "${PKCS11_LIB}" \
    --slot-token-label "${PKCS11_SLOT_TOKEN_LABEL}" \
    --pin "${PKCS11_USER_PIN}" \
    create-wrapping-key --label "${PKCS11_WRAP_KEY_ALIAS}"
) 200>"$LOCK_FILE"

# Execute the main command
exec "$@"
