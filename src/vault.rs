//! Encryption for everything xtermium stores.
//!
//! Only symmetric primitives are used, which keeps it post-quantum safe: quantum computers break
//! public-key schemes (RSA, elliptic curves), while 256-bit symmetric keys keep ~128-bit security
//! even against Grover's algorithm.
//! - Files: XChaCha20-Poly1305 under a random 256-bit data key, a fresh 192-bit nonce per write,
//!   and the file's name as associated data (a sessions file can't be swapped in for settings).
//! - Passphrases (recovery, exports): Argon2id, 64 MiB, 3 passes.
//!
//! Formats (all integers little-endian):
//! - sealed file: `XTM1` | nonce (24) | ciphertext+tag
//! - recovery key: TOML [`Recovery`], the data key sealed under a passphrase-derived key
//! - export: `XTMX` | salt (16) | m_cost KiB (u32) | t_cost (u32) | nonce (24) | ciphertext+tag

use anyhow::{Context, Result, anyhow, bail};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use serde::{Deserialize, Serialize};

pub type Key = [u8; 32];

const FILE_MAGIC: &[u8; 4] = b"XTM1";
const EXPORT_MAGIC: &[u8; 4] = b"XTMX";
const M_COST: u32 = 64 * 1024;
const T_COST: u32 = 3;

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("OS random number generator");
    b
}

fn seal_raw(key: &Key, aad: &[u8], plain: &[u8]) -> ([u8; 24], Vec<u8>) {
    let nonce = random::<24>();
    let ct = XChaCha20Poly1305::new(&(*key).into())
        .encrypt(&nonce.into(), Payload { msg: plain, aad })
        .expect("encryption cannot fail for in-memory data");
    (nonce, ct)
}

fn open_raw(key: &Key, aad: &[u8], nonce: &[u8], ct: &[u8]) -> Result<Vec<u8>> {
    let nonce: [u8; 24] = nonce.try_into().map_err(|_| anyhow!("truncated data"))?;
    XChaCha20Poly1305::new(&(*key).into())
        .decrypt(&nonce.into(), Payload { msg: ct, aad })
        .map_err(|_| anyhow!("wrong key or damaged data"))
}

/// Encrypts a file's contents; `name` binds it to its purpose.
pub fn seal(key: &Key, name: &str, plain: &[u8]) -> Vec<u8> {
    let (nonce, ct) = seal_raw(key, name.as_bytes(), plain);
    [FILE_MAGIC.as_slice(), &nonce, &ct].concat()
}

pub fn open(key: &Key, name: &str, data: &[u8]) -> Result<Vec<u8>> {
    let rest = data.strip_prefix(FILE_MAGIC).context("not an xtermium encrypted file")?;
    if rest.len() < 24 {
        bail!("truncated encrypted file");
    }
    open_raw(key, name.as_bytes(), &rest[..24], &rest[24..])
}

fn derive(passphrase: &str, salt: &[u8], m_cost: u32, t_cost: u32) -> Result<Key> {
    use argon2::{Algorithm, Argon2, Params, Version};
    // Bounds keep a hostile file from asking for absurd memory or time.
    if !(8 * 1024..=1024 * 1024).contains(&m_cost) || !(1..=10).contains(&t_cost) {
        bail!("unsupported key-derivation settings");
    }
    let params = Params::new(m_cost, t_cost, 1, Some(32)).map_err(|e| anyhow!("{e}"))?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("{e}"))?;
    Ok(key)
}

/// The data key, sealed under a key derived from the recovery passphrase. Safe to store and to
/// upload: without the passphrase it is useless.
#[derive(Serialize, Deserialize, Clone)]
pub struct Recovery {
    pub salt: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub nonce: String,
    pub sealed_key: String,
}

pub fn wrap(passphrase: &str, key: &Key) -> Result<Recovery> {
    let salt = random::<16>();
    let kek = derive(passphrase, &salt, M_COST, T_COST)?;
    let (nonce, ct) = seal_raw(&kek, b"recovery", key);
    Ok(Recovery { salt: hex(&salt), m_cost: M_COST, t_cost: T_COST, nonce: hex(&nonce), sealed_key: hex(&ct) })
}

pub fn unwrap(passphrase: &str, r: &Recovery) -> Result<Key> {
    let kek = derive(passphrase, &unhex(&r.salt)?, r.m_cost, r.t_cost)?;
    let key = open_raw(&kek, b"recovery", &unhex(&r.nonce)?, &unhex(&r.sealed_key)?)
        .map_err(|_| anyhow!("wrong recovery passphrase"))?;
    key.try_into().map_err(|_| anyhow!("damaged recovery key"))
}

/// A password or PIN made into something to check it against later (the lock, lock.rs):
/// Argon2id with a salt of its own, as `salt:hash` in hex.
pub fn hash_secret(secret: &str) -> Result<String> {
    let salt = random::<16>();
    Ok(format!("{}:{}", hex(&salt), hex(&derive(secret, &salt, M_COST, T_COST)?)))
}

/// Whether `secret` is the one `hash_secret` made `stored` from, compared in constant time.
pub fn check_secret(secret: &str, stored: &str) -> bool {
    let Some((Ok(salt), Ok(hash))) = stored.split_once(':').map(|(s, h)| (unhex(s), unhex(h))) else { return false };
    derive(secret, &salt, M_COST, T_COST).is_ok_and(|k| hash.len() == k.len() && hash.iter().zip(k).fold(0, |d, (a, b)| d | (a ^ b)) == 0)
}

/// A self-contained file protected by a passphrase (folder exports).
pub fn export_seal(passphrase: &str, plain: &[u8]) -> Result<Vec<u8>> {
    let salt = random::<16>();
    let key = derive(passphrase, &salt, M_COST, T_COST)?;
    let (nonce, ct) = seal_raw(&key, EXPORT_MAGIC, plain);
    Ok([EXPORT_MAGIC.as_slice(), &salt, &M_COST.to_le_bytes(), &T_COST.to_le_bytes(), &nonce, &ct].concat())
}

pub fn export_open(passphrase: &str, data: &[u8]) -> Result<Vec<u8>> {
    let rest = data.strip_prefix(EXPORT_MAGIC).context("not an xtermium export")?;
    if rest.len() < 16 + 8 + 24 {
        bail!("truncated export");
    }
    let u32_at = |i: usize| u32::from_le_bytes(rest[i..i + 4].try_into().unwrap());
    let key = derive(passphrase, &rest[..16], u32_at(16), u32_at(20))?;
    open_raw(&key, EXPORT_MAGIC, &rest[24..48], &rest[48..]).map_err(|_| anyhow!("wrong passphrase"))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        bail!("bad hex");
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| anyhow!("bad hex"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_round_trip_and_reject_tampering() {
        let key = random::<32>();
        let sealed = seal(&key, "sessions", b"[[session]]");
        assert_eq!(open(&key, "sessions", &sealed).unwrap(), b"[[session]]");
        assert!(open(&random::<32>(), "sessions", &sealed).is_err(), "wrong key");
        assert!(open(&key, "settings", &sealed).is_err(), "wrong purpose");
        let mut bad = sealed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(open(&key, "sessions", &bad).is_err(), "tampered");
        assert_ne!(seal(&key, "sessions", b"x")[4..28], seal(&key, "sessions", b"x")[4..28], "fresh nonces");
    }

    #[test]
    fn passphrases_unlock_keys_and_exports() {
        let key = random::<32>();
        let r = wrap("correct horse", &key).unwrap();
        assert_eq!(unwrap("correct horse", &r).unwrap(), key);
        assert!(unwrap("wrong", &r).is_err());

        let file = export_seal("pw", b"folder").unwrap();
        assert_eq!(export_open("pw", &file).unwrap(), b"folder");
        assert!(export_open("nope", &file).is_err());
        assert!(export_open("pw", b"XTM1junk").is_err());
    }
}
