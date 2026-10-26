"""Generates known-answer vectors with reference libraries independent of the Rust implementation.

Argon2id uses argon2-cffi (the libargon2 C reference implementation); XChaCha20-Poly1305 uses PyNaCl (libsodium).
Usage: python tools/gen_vectors.py > crates/onepaste-core/tests/vectors.json
Requires: pip install argon2-cffi pynacl
"""
import base64, json
from argon2.low_level import hash_secret_raw, Type
from nacl.bindings import crypto_aead_xchacha20poly1305_ietf_encrypt as aead

def b64(b): return base64.b64encode(b).decode()

PASS = "correct horse battery staple"
SALT = bytes([0x42]) * 16
M_KIB, T, P = 8192, 3, 1
kek = hash_secret_raw(PASS.encode(), SALT, time_cost=T, memory_cost=M_KIB, parallelism=P,
                      hash_len=32, type=Type.ID, version=19)

KEY = bytes([0x07]) * 32
NONCE = bytes([0x09]) * 24
AAD = b"onepaste/item/v1:uid-1"
PT = "\u4f60\u597d\uff0cOnePaste".encode()
ct = aead(PT, AAD, NONCE, KEY)           # ciphertext || 16-byte tag

MKWRAP_AAD = b"onepaste/mkwrap/v1"
MK = bytes(range(32))
wrapped = aead(MK, MKWRAP_AAD, NONCE, kek)

print(json.dumps({
    "argon2id": {"password": PASS, "salt_b64": b64(SALT), "m_kib": M_KIB, "t": T, "p": P, "kek_b64": b64(kek)},
    "aead_item": {"key_b64": b64(KEY), "nonce_b64": b64(NONCE), "aad": AAD.decode(),
                  "plaintext_b64": b64(PT), "sealed_b64": b64(NONCE + ct)},
    "mk_wrap": {"kek_b64": b64(kek), "nonce_b64": b64(NONCE), "aad": MKWRAP_AAD.decode(),
                "mk_b64": b64(MK), "wrapped_b64": b64(NONCE + wrapped)},
}, indent=2))
