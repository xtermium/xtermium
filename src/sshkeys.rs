//! Settings › SSH keys: the key pairs in `~/.ssh` (where SSH sessions look for keys), their public
//! halves to copy onto servers, and new pairs made here.

use anyhow::{Context, Result, bail};
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, EcdsaCurve, HashAlg, PrivateKey, PublicKey};
use serde::Serialize;
use std::path::PathBuf;

/// A key pair as Settings shows it.
#[derive(Serialize)]
pub struct Key {
    /// The private key's file name in `~/.ssh` (`id_ed25519`).
    pub name: String,
    /// "ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"…
    pub kind: String,
    pub comment: String,
    /// SHA256:…, as `ssh-keygen -l` prints it.
    pub fingerprint: String,
    /// The public key's line, for a server's `authorized_keys`.
    pub public: String,
    /// The private key needs its passphrase.
    pub encrypted: bool,
}

/// What Settings › SSH keys lists: the folder, the keys, and the comment a new key gets.
#[derive(Serialize)]
pub struct Keys {
    pub dir: String,
    pub keys: Vec<Key>,
    pub comment: String,
}

fn dir() -> Result<PathBuf> {
    Ok(directories::UserDirs::new().context("no home folder")?.home_dir().join(".ssh"))
}

/// `user@computer`, as `ssh-keygen` comments a new key.
fn whoami() -> String {
    let var = |names: &[&str]| names.iter().find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty())).unwrap_or_default();
    format!("{}@{}", var(&["USERNAME", "USER"]), var(&["COMPUTERNAME", "HOSTNAME"]).to_lowercase())
}

/// `comment`: the public key's own, else the private key's (an encrypted one hides it).
fn info(name: String, key: &PublicKey, comment: &str, encrypted: bool) -> Key {
    let own: &str = key.comment().as_ref();
    let comment = if own.is_empty() { comment.trim() } else { own }.to_string();
    let mut key = key.clone();
    key.set_comment(comment.as_str());
    let (kind, fingerprint) = (key.algorithm().to_string(), key.fingerprint(HashAlg::Sha256).to_string());
    Key { name, kind, comment, fingerprint, public: key.to_openssh().unwrap_or_default(), encrypted }
}

/// A file name that stays in `~/.ssh`: letters, digits, `.`, `_` and `-`, not starting with a dot.
fn valid(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !ok {
        bail!("Name the key with letters, digits, dots, dashes and underscores only.");
    }
    Ok(())
}

/// The key pairs in `~/.ssh`: each private key with the `.pub` beside it, or without one, the
/// public half inside it (OpenSSH's format keeps it readable even when encrypted). By name.
pub fn list() -> Result<Keys> {
    let dir = dir()?;
    let mut keys = vec![];
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let small = e.metadata().is_ok_and(|m| m.is_file() && m.len() < 64 << 10);
        if !small || name.ends_with(".pub") || valid(&name).is_err() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        if !text.contains("PRIVATE KEY") {
            continue; // known_hosts, config, authorized_keys
        }
        let private = PrivateKey::from_openssh(&text).ok();
        let encrypted = private.as_ref().map_or(text.contains("ENCRYPTED"), |k| k.is_encrypted());
        let public = std::fs::read_to_string(dir.join(format!("{name}.pub"))).ok().and_then(|l| PublicKey::from_openssh(l.trim()).ok());
        let comment = private.as_ref().map(|k| AsRef::<str>::as_ref(k.comment()).to_string()).unwrap_or_default();
        if let Some(p) = public.as_ref().or(private.as_ref().map(|k| k.public_key())) {
            keys.push(info(name, p, &comment, encrypted));
        }
    }
    keys.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Keys { dir: dir.display().to_string(), keys, comment: whoami() })
}

/// Makes a key pair `~/.ssh/<name>` and `<name>.pub`: `kind` "ed25519" (the default), "ecdsa"
/// (P-256) or "rsa" (4096 bits); encrypted with `passphrase` unless it's empty. Never overwrites.
pub fn create(name: &str, kind: &str, comment: &str, passphrase: &str) -> Result<Key> {
    valid(name)?;
    let dir = dir()?;
    let (private, public) = (dir.join(name), dir.join(format!("{name}.pub")));
    if private.exists() || public.exists() {
        bail!("There is a key named {name} already.");
    }
    let algorithm = match kind {
        "ecdsa" => Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 },
        "rsa" => Algorithm::Rsa { hash: None },
        _ => Algorithm::Ed25519,
    };
    let mut rng = rand::rng();
    let mut key = PrivateKey::random(&mut rng, algorithm)?;
    key.set_comment(comment.trim());
    let made = info(name.into(), key.public_key(), comment, !passphrase.is_empty());
    let key = if passphrase.is_empty() { key } else { key.encrypt(&mut rng, passphrase)? };
    let text = key.to_openssh(LineEnding::LF)?;
    std::fs::create_dir_all(&dir)?;
    write_new(&private, text.as_bytes(), true)?;
    write_new(&public, format!("{}\n", made.public).as_bytes(), false)?;
    Ok(made)
}

/// Writes a file that mustn't exist yet; `secret`: readable by its owner only (Unix; on Windows
/// the profile folder's permissions already are).
fn write_new(path: &std::path::Path, data: &[u8], secret: bool) -> Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(if secret { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = secret;
    o.open(path).with_context(|| format!("writing {}", path.display()))?.write_all(data)?;
    Ok(())
}

/// Deletes the key pair `name` (its `.pub` too).
pub fn delete(name: &str) -> Result<()> {
    valid(name)?;
    let dir = dir()?;
    std::fs::remove_file(dir.join(name)).with_context(|| format!("deleting {name}"))?;
    let _ = std::fs::remove_file(dir.join(format!("{name}.pub")));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new key reads back as it was made: its public line, encrypted or not, and names that
    /// would leave `~/.ssh` are refused.
    #[test]
    fn keys_made_read_back() {
        for (kind, prefix) in [("ed25519", "ssh-ed25519 "), ("ecdsa", "ecdsa-sha2-nistp256 ")] {
            for pass in ["", "pw"] {
                let mut key = PrivateKey::random(&mut rand::rng(), if kind == "ecdsa" { Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 } } else { Algorithm::Ed25519 }).unwrap();
                key.set_comment("me@box");
                let key = if pass.is_empty() { key } else { key.encrypt(&mut rand::rng(), pass).unwrap() };
                let back = PrivateKey::from_openssh(key.to_openssh(LineEnding::LF).unwrap()).unwrap();
                let k = info("id".into(), back.public_key(), " me@box ", back.is_encrypted());
                assert!(k.public.starts_with(prefix) && k.public.ends_with(" me@box"), "{}", k.public);
                assert!(k.fingerprint.starts_with("SHA256:") && k.encrypted == !pass.is_empty());
                if !pass.is_empty() {
                    assert!(back.decrypt(pass).is_ok());
                }
            }
        }
        for bad in ["", "../x", "a/b", ".hidden", "a b", "a\\b"] {
            assert!(valid(bad).is_err(), "{bad}");
        }
        assert!(valid("id_ed25519.work-2").is_ok());
    }
}
