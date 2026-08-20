 # SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
 #
 # SPDX-License-Identifier: EUPL-1.2
import base64
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.exceptions import InvalidTag

def b64u(b):   return base64.urlsafe_b64encode(b).rstrip(b"=").decode("ascii")
def unb64u(s): return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))

# RFC 7516 A.1, transcribed from the RFC's JSON array notation.
plaintext = bytes([84,104,101,32,116,114,117,101,32,115,105,103,110,32,111,102,32,105,110,
 116,101,108,108,105,103,101,110,99,101,32,105,115,32,110,111,116,32,107,110,111,119,108,
 101,100,103,101,32,98,117,116,32,105,109,97,103,105,110,97,116,105,111,110,46])          # A.1
cek = bytes([177,161,244,128,84,143,225,115,63,180,3,255,107,154,212,246,138,7,110,91,112,
 46,34,105,47,130,203,46,122,234,64,252])                                                 # A.1.2
iv  = bytes([227,197,117,252,2,219,233,68,180,225,77,219])                                # A.1.4
rfc_ct  = bytes([229,236,166,241,53,191,115,196,174,43,73,109,39,122,233,96,140,206,120,52,
 51,237,48,11,190,219,186,80,111,104,50,142,47,167,59,61,181,127,196,21,40,82,242,32,123,
 143,168,226,73,216,176,144,138,247,106,60,16,205,160,109,64,63,192])                     # A.1.6
rfc_tag = bytes([92,80,104,49,133,25,161,215,173,101,219,211,136,91,210,145])             # A.1.6

aesgcm = AESGCM(cek)

# 0. Reproduce A.1 exactly. Catches any transcription slip above before it can propagate.
rsa_hdr = "eyJhbGciOiJSU0EtT0FFUCIsImVuYyI6IkEyNTZHQ00ifQ"                                # A.1.1
assert unb64u(rsa_hdr) == b'{"alg":"RSA-OAEP","enc":"A256GCM"}'
out = aesgcm.encrypt(iv, plaintext, rsa_hdr.encode("ascii"))
assert out[:-16] == rfc_ct and out[-16:] == rfc_tag       # ciphertext AND tag match the RFC

# 1. Same key, same IV, same plaintext — A.1's header with only the alg value swapped.
dir_hdr = b64u(b'{"alg":"dir","enc":"A256GCM"}')
out2 = aesgcm.encrypt(iv, plaintext, dir_hdr.encode("ascii"))
ct, tag = out2[:-16], out2[-16:]
assert ct == rfc_ct                     # unchanged: CTR keystream does not depend on AAD
assert tag != rfc_tag                   # changed:   GHASH does

jwe      = f"{dir_hdr}..{b64u(iv)}.{b64u(ct)}.{b64u(tag)}"
bad_tag  = tag[:-1] + bytes([tag[-1] ^ 1])
jwe_tag  = f"{dir_hdr}..{b64u(iv)}.{b64u(ct)}.{b64u(bad_tag)}"
kid_hdr  = b64u(b'{"alg":"dir","enc":"A256GCM","kid":"kat"}')
jwe_aad  = f"{kid_hdr}..{b64u(iv)}.{b64u(ct)}.{b64u(tag)}"   # same iv/ct/tag, altered AAD

assert aesgcm.decrypt(iv, ct + tag, dir_hdr.encode("ascii")) == plaintext
for label, (c, a) in {"tag": (ct + bad_tag, dir_hdr), "aad": (ct + tag, kid_hdr)}.items():
    try:
        aesgcm.decrypt(iv, c, a.encode("ascii")); print("BUG:", label, "accepted")
    except InvalidTag:
        print(label, "correctly rejected")

print("KAT_VALID_JWE        =", jwe)
print("KAT_TAMPERED_TAG_JWE  =", jwe_tag)
print("KAT_TAMPERED_AAD_JWE  =", jwe_aad)
