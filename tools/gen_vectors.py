"""用与 Rust 实现无关的参考库生成已知答案向量。

Argon2id 用 argon2-cffi（libargon2 C 参考实现），XChaCha20-Poly1305 用 PyNaCl（libsodium）。
用法：python tools/gen_vectors.py > crates/onepaste-core/tests/vectors.json
依赖：pip install argon2-cffi pynacl
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
PT = "你好，OnePaste".encode()
ct = aead(PT, AAD, NONCE, KEY)           # 密文 || 16 字节标签

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
