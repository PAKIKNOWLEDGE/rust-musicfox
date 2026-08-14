//! NetEase Cloud Music "weapi" request encryption.
//!
//! Every privileged API call must send form fields `params` and `encSecKey`
//! produced by double AES-128-CBC encryption plus a raw (unpadded) RSA
//! encryption of a random 16-char session key. The algorithm mirrors the
//! reference implementation in go-musicfox's `netease-music` SDK
//! (`vendor/.../netease-music/util/cryto.go`).

use aes::Aes128;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockEncryptMut, KeyIvInit};
use rand::RngCore;
use rsa::BigUint;

const NONCE: &[u8] = b"0CoJUm6Qyw8W8jud";
const IV: &[u8] = b"0102030405060708";
const PUBKEY_E: &str = "010001";
const PUBKEY_N: &str = "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";
const STD_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

type Aes128CbcEnc = cbc::Encryptor<Aes128>;

fn aes_encrypt(data: &str, key: &[u8]) -> String {
    let mut buf = data.as_bytes().to_vec();
    let len = buf.len();
    buf.resize(len + 16, 0);
    let ct = Aes128CbcEnc::new(key.into(), IV.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, len)
        .expect("aes encrypt failed");
    B64.encode(ct)
}

/// Raw (unpadded) RSA: m = 112 zero bytes + 16-byte key, then m^e mod n.
/// NetEase uses no padding, so the 16-byte session key is right-aligned in
/// a 128-byte block. Output is hex (leading zeros trimmed, as in Go's
/// `big.Int.Exp().Bytes()`).
fn rsa_encrypt(sec_key: &[u8]) -> String {
    let n = BigUint::parse_bytes(PUBKEY_N.as_bytes(), 16).expect("valid modulus");
    let e = BigUint::parse_bytes(PUBKEY_E.as_bytes(), 16).expect("valid exponent");
    let mut m = vec![0u8; 128 - sec_key.len()];
    m.extend_from_slice(sec_key);
    let m = BigUint::from_bytes_be(&m);
    let c = m.modpow(&e, &n);
    hex::encode(c.to_bytes_be())
}

/// Generate a random 16-char session key from the base62 alphabet.
fn create_secret_key() -> Vec<u8> {
    let mut key = [0u8; 16];
    let mut rng = rand::thread_rng();
    for b in key.iter_mut() {
        *b = STD_CHARS[(rng.next_u32() % 62) as usize];
    }
    key.to_vec()
}

/// Wrap `data` into weapi `{ params, encSecKey }` form fields.
pub fn weapi_params(data: &serde_json::Value) -> (String, String) {
    let text = serde_json::to_string(data).expect("serialize payload");
    let sec_key = create_secret_key();
    let params = aes_encrypt(&aes_encrypt(&text, NONCE), &sec_key);
    let enc_sec_key = rsa_encrypt(&sec_key);
    (params, enc_sec_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_roundtrip_shape() {
        let json = serde_json::json!({"csrf_token": ""});
        let (params, _) = weapi_params(&json);
        assert!(!params.is_empty());
        assert!(B64.decode(params).is_ok());
    }

    #[test]
    fn secret_key_is_16_base62_chars() {
        let key = create_secret_key();
        assert_eq!(key.len(), 16);
        assert!(key.iter().all(|b| STD_CHARS.contains(b)));
    }

    #[test]
    fn rsa_output_is_hex() {
        let key = create_secret_key();
        let enc = rsa_encrypt(&key);
        // 128-byte ciphertext -> up to 256 hex chars (leading zeros trimmed)
        assert!(enc.len() >= 254 && enc.len() <= 256, "len={}", enc.len());
        assert!(hex::decode(&enc).is_ok());
    }
}
