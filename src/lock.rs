//! The lock (Settings › Security › Lock), for stepping away from the computer: every window hides
//! behind a lock screen until a password, a PIN, a passkey or Windows Hello opens it, while the
//! sessions keep running behind it. It locks when xtermium starts, on request (the `lock` action),
//! and after a while without input. Its settings are `lock.xtm`, on this device only: hashes and
//! the passkey's public key, which never go to the UI. Changing them takes an unlock first, unless
//! none is set yet.
//!
//! ponytail: it guards the windows, not the backend: automation and the `xtermium` command line
//! keep working while it's locked (scripts and agents run unattended). Gate `automation::call` on
//! `is_locked()` if that should change.

use crate::vault;
use anyhow::{Result, anyhow, bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an unlock lets the lock's settings change.
const CHANGES: Duration = Duration::from_secs(5 * 60);
/// How long a passkey has to answer its challenge.
const ANSWER: Duration = Duration::from_secs(3 * 60);

#[derive(Serialize, Deserialize, Default, Clone)]
struct Config {
    /// The password and the PIN as `vault::hash_secret` made them; "" for none.
    #[serde(default)]
    password: String,
    #[serde(default)]
    pin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    passkey: Option<Passkey>,
    /// Windows Hello: a face, a fingerprint, or Windows' own PIN.
    #[serde(default)]
    biometrics: bool,
    /// Minutes without input before it locks; 0 for never.
    #[serde(default)]
    idle: u32,
}

impl Config {
    fn any(&self) -> bool {
        !self.password.is_empty() || !self.pin.is_empty() || self.passkey.is_some() || self.biometrics
    }
}

/// A WebAuthn credential the window made (`navigator.credentials.create`).
#[derive(Serialize, Deserialize, Clone)]
struct Passkey {
    /// Its id, base64url.
    id: String,
    /// COSE algorithm: -7 ES256, -8 EdDSA, -257 RS256.
    alg: i32,
    /// The public key as a SubjectPublicKeyInfo, base64.
    key: String,
    /// The relying party id and origin it was made for: the window's.
    rp: String,
    origin: String,
}

#[derive(Default)]
struct State {
    config: Config,
    locked: bool,
    /// The last unlock: the lock's settings can change for a while after it.
    confirmed: Option<Instant>,
    /// The challenge a passkey is to sign, and when it was made.
    challenge: Option<(Vec<u8>, Instant)>,
    /// Wrong passwords and PINs in a row, and until when the next try waits.
    fails: u32,
    wait: Option<Instant>,
    /// The last input the window reported: idle time where the OS doesn't tell (`os_idle`).
    active: Option<Instant>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE.lock().unwrap().get_or_insert_with(Default::default))
}

/// The stored data can be read (xtermium started, or the recovery passphrase opened it): the
/// lock's settings load, and it locks if there is a way to unlock.
pub fn start() {
    let config: Config = crate::config::read("lock", &[], |_| Ok(String::new())).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
    *STATE.lock().unwrap() = Some(State { locked: config.any(), config, active: Some(Instant::now()), ..Default::default() });
}

/// What the UI shows: the ways to unlock that are set (never their secrets) and the state.
#[derive(Serialize)]
pub struct Info {
    password: bool,
    pin: bool,
    /// The passkey's credential id and relying party, for `navigator.credentials.get`.
    passkey: Option<(String, String)>,
    biometrics: bool,
    idle: u32,
    locked: bool,
    /// Unlocked lately: the lock's settings can change now.
    confirmed: bool,
    /// Windows Hello is set up on this computer.
    hello: bool,
}

pub fn info() -> Info {
    let hello = hello_available();
    with(|s| {
        let c = &s.config;
        Info {
            password: !c.password.is_empty(),
            pin: !c.pin.is_empty(),
            passkey: c.passkey.as_ref().map(|p| (p.id.clone(), p.rp.clone())),
            biometrics: c.biometrics,
            idle: c.idle,
            locked: s.locked,
            confirmed: !c.any() || s.confirmed.is_some_and(|t| t.elapsed() < CHANGES),
            hello,
        }
    })
}

pub fn is_locked() -> bool {
    with(|s| s.locked)
}

/// Locks, if there is a way to unlock; says whether it's locked.
pub fn lock() -> bool {
    with(|s| {
        s.locked = s.config.any();
        s.confirmed = None;
        s.locked
    })
}

/// The window saw input (keys, the pointer).
pub fn activity() {
    with(|s| s.active = Some(Instant::now()));
}

/// Locks once the minutes set have gone by without input; true when this locked it.
pub fn idle_check() -> bool {
    let idle = os_idle().or_else(|| with(|s| s.active.map(|a| a.elapsed())));
    with(|s| {
        let after = Duration::from_secs(u64::from(s.config.idle) * 60);
        let due = s.config.any() && !s.locked && s.config.idle > 0 && idle.is_some_and(|i| i >= after);
        if due {
            (s.locked, s.confirmed) = (true, None);
        }
        due
    })
}

/// A new challenge for a passkey to sign (an unlock) or to be made with, base64url.
pub fn challenge() -> String {
    let c = vault::random::<32>().to_vec();
    let text = URL_SAFE_NO_PAD.encode(&c);
    with(|s| s.challenge = Some((c, Instant::now())));
    text
}

fn taken_challenge() -> Result<Vec<u8>> {
    with(|s| s.challenge.take()).filter(|(_, at)| at.elapsed() < ANSWER).map(|(c, _)| c).ok_or_else(|| anyhow!("The passkey took too long. Try again."))
}

/// How the UI proves it's you.
#[derive(Deserialize)]
#[serde(tag = "with", rename_all = "lowercase")]
pub enum Proof {
    Password { secret: String },
    Pin { secret: String },
    /// A passkey's answer to `challenge()`: authenticator data, client data and signature, base64.
    Passkey { auth: String, client: String, sig: String },
    Biometrics,
}

/// Checks `proof`, then unlocks; the lock's settings can change for a while after. `hwnd`: the
/// window Windows Hello's question belongs to. Wrong passwords and PINs make the next try wait,
/// more each time from the fifth.
pub fn unlock(proof: Proof, hwnd: isize) -> Result<()> {
    let config = with(|s| match s.wait.filter(|w| *w > Instant::now()) {
        Some(w) => Err(anyhow!("Too many tries. Try again in {} s.", (w - Instant::now()).as_secs() + 1)),
        None => Ok(s.config.clone()),
    })?;
    let (ok, wrong) = match &proof {
        Proof::Password { secret } => (!config.password.is_empty() && vault::check_secret(secret, &config.password), "Wrong password."),
        Proof::Pin { secret } => (!config.pin.is_empty() && vault::check_secret(secret, &config.pin), "Wrong PIN."),
        Proof::Passkey { auth, client, sig } => {
            let p = config.passkey.as_ref().ok_or_else(|| anyhow!("No passkey is set."))?;
            let key = STANDARD.decode(&p.key)?;
            assertion(p.alg, &key, &p.rp, &p.origin, &taken_challenge()?, &STANDARD.decode(auth)?, &STANDARD.decode(client)?, &STANDARD.decode(sig)?)?;
            (true, "")
        }
        Proof::Biometrics => {
            ensure!(config.biometrics, "Windows Hello isn't on.");
            (hello(hwnd)?, "Windows Hello didn't confirm it's you.")
        }
    };
    let counts = matches!(proof, Proof::Password { .. } | Proof::Pin { .. });
    with(|s| {
        if ok {
            (s.locked, s.confirmed, s.fails, s.wait) = (false, Some(Instant::now()), 0, None);
        } else if counts {
            s.fails += 1;
            if s.fails >= 5 {
                s.wait = Some(Instant::now() + Duration::from_secs(30 << (s.fails - 5).min(5)));
            }
        }
    });
    ensure!(ok, "{wrong}");
    Ok(())
}

/// A passkey just made in the window: its id (base64url), algorithm, public key and client
/// data (base64).
#[derive(Deserialize)]
pub struct NewPasskey {
    id: String,
    alg: i32,
    key: String,
    client: String,
}

/// A change to how it locks. An empty secret removes that way to unlock.
#[derive(Deserialize)]
#[serde(tag = "set", rename_all = "lowercase")]
pub enum Change {
    Password { secret: String },
    Pin { secret: String },
    Passkey { passkey: Option<NewPasskey> },
    Biometrics { on: bool },
    Idle { minutes: u32 },
}

pub fn set(change: Change, hwnd: isize) -> Result<()> {
    let mut config = with(|s| {
        let free = !s.config.any() || s.confirmed.is_some_and(|t| t.elapsed() < CHANGES);
        free.then(|| s.config.clone()).ok_or_else(|| anyhow!("Confirm it's you first."))
    })?;
    match change {
        Change::Password { secret } if secret.is_empty() => config.password.clear(),
        Change::Password { secret } => {
            ensure!(secret.chars().count() >= 8, "Use a password of at least 8 characters.");
            config.password = vault::hash_secret(&secret)?;
        }
        Change::Pin { secret } if secret.is_empty() => config.pin.clear(),
        Change::Pin { secret } => {
            ensure!((4..=12).contains(&secret.len()) && secret.bytes().all(|b| b.is_ascii_digit()), "A PIN is 4 to 12 digits.");
            config.pin = vault::hash_secret(&secret)?;
        }
        Change::Passkey { passkey: None } => config.passkey = None,
        Change::Passkey { passkey: Some(p) } => {
            let client: serde_json::Value = serde_json::from_slice(&STANDARD.decode(&p.client)?)?;
            let asked = URL_SAFE_NO_PAD.encode(taken_challenge()?);
            ensure!(client["type"] == "webauthn.create" && client["challenge"] == asked.as_str(), "The passkey answered another request.");
            let origin = client["origin"].as_str().unwrap_or_default().to_string();
            let rp = reqwest::Url::parse(&origin).ok().and_then(|u| u.host_str().map(String::from)).ok_or_else(|| anyhow!("The passkey came from no address."))?;
            ensure!([-7, -8, -257].contains(&p.alg) && spki_key(&STANDARD.decode(&p.key)?).is_some(), "This kind of passkey isn't supported.");
            config.passkey = Some(Passkey { id: p.id, alg: p.alg, key: p.key, rp, origin });
        }
        Change::Biometrics { on: true } => {
            ensure!(hello(hwnd)?, "Windows Hello didn't confirm it's you.");
            config.biometrics = true;
        }
        Change::Biometrics { on: false } => config.biometrics = false,
        Change::Idle { minutes } => config.idle = minutes.min(24 * 60),
    }
    crate::config::write("lock", &toml::to_string(&config)?)?;
    with(|s| {
        // The first way to unlock: the next changes need no unlock first either, for a while.
        if !s.config.any() {
            s.confirmed = Some(Instant::now());
        }
        s.config = config;
    });
    Ok(())
}

/// Checks a passkey's answer (WebAuthn assertion): the client data asked for this challenge
/// from this origin, the authenticator data is for this relying party with the user present and
/// verified, and the signature over both checks out with the public key.
#[allow(clippy::too_many_arguments)]
fn assertion(alg: i32, spki: &[u8], rp: &str, origin: &str, challenge: &[u8], auth: &[u8], client: &[u8], sig: &[u8]) -> Result<()> {
    let c: serde_json::Value = serde_json::from_slice(client)?;
    ensure!(c["type"] == "webauthn.get" && c["challenge"] == URL_SAFE_NO_PAD.encode(challenge).as_str(), "The passkey answered another request.");
    ensure!(c["origin"] == origin, "The passkey answered another window.");
    ensure!(auth.len() >= 37 && auth[..32] == Sha256::digest(rp.as_bytes())[..], "The passkey is for another place.");
    // Flags: user present (1) and verified (4), as `userVerification: "required"` asked.
    ensure!(auth[32] & 0x05 == 0x05, "The passkey didn't check it's you.");
    use ring::signature::{ECDSA_P256_SHA256_ASN1, ED25519, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey, VerificationAlgorithm};
    let how: &'static dyn VerificationAlgorithm = match alg {
        -7 => &ECDSA_P256_SHA256_ASN1,
        -8 => &ED25519,
        -257 => &RSA_PKCS1_2048_8192_SHA256,
        _ => bail!("This kind of passkey isn't supported."),
    };
    let key = spki_key(spki).ok_or_else(|| anyhow!("The passkey's public key is damaged."))?;
    let signed = [auth, &Sha256::digest(client)[..]].concat();
    UnparsedPublicKey::new(how, key).verify(&signed, sig).map_err(|_| anyhow!("The passkey's signature doesn't check out."))
}

/// The key inside a SubjectPublicKeyInfo (DER): SEQUENCE { AlgorithmIdentifier, BIT STRING },
/// the bit string's bytes: an EC point, an Ed25519 key, or an RSAPublicKey, as ring takes them.
fn spki_key(der: &[u8]) -> Option<&[u8]> {
    /// One DER element: its tag, its contents, and what follows it.
    fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        let (&tag, b) = b.split_first()?;
        let (&first, mut b) = b.split_first()?;
        let len = if first < 0x80 {
            usize::from(first)
        } else {
            let n = usize::from(first & 0x7f);
            if n > 2 || b.len() < n {
                return None;
            }
            let len = b[..n].iter().fold(0, |a, x| a << 8 | usize::from(*x));
            b = &b[n..];
            len
        };
        (b.len() >= len).then(|| (tag, &b[..len], &b[len..]))
    }
    let (0x30, seq, _) = tlv(der)? else { return None };
    let (0x30, _, rest) = tlv(seq)? else { return None };
    let (0x03, bits, _) = tlv(rest)? else { return None };
    bits.strip_prefix(&[0])
}

/// How long since the last key press or pointer move anywhere on this computer.
#[cfg(windows)]
fn os_idle() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut last = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe { GetLastInputInfo(&mut last) }.as_bool().then(|| Duration::from_millis(u64::from(unsafe { GetTickCount() }.wrapping_sub(last.dwTime))))
}

#[cfg(not(windows))]
fn os_idle() -> Option<Duration> {
    None
}

/// Asks Windows Hello (face, fingerprint or Windows' PIN) over window `hwnd`; true if it's you.
#[cfg(windows)]
fn hello(hwnd: isize) -> Result<bool> {
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier};
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    let interop = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
    let ask: windows_future::IAsyncOperation<UserConsentVerificationResult> =
        unsafe { interop.RequestVerificationForWindowAsync(windows::Win32::Foundation::HWND(hwnd as _), &"Unlock xtermium".into())? };
    Ok(ask.join()? == UserConsentVerificationResult::Verified)
}

#[cfg(not(windows))]
fn hello(_: isize) -> Result<bool> {
    bail!("Windows Hello is only on Windows.")
}

/// Windows Hello is set up here. Asked once: it takes a moment.
fn hello_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        #[cfg(windows)]
        {
            use windows::Security::Credentials::UI::{UserConsentVerifier, UserConsentVerifierAvailability};
            UserConsentVerifier::CheckAvailabilityAsync().and_then(|a| a.join()).is_ok_and(|a| a == UserConsentVerifierAvailability::Available)
        }
        #[cfg(not(windows))]
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};

    /// A passkey's answer passes with the right key, challenge, origin and flags, and fails when
    /// any of them is off; SubjectPublicKeyInfo comes apart into the key ring checks with.
    #[test]
    fn passkey_answers_are_checked() {
        let rng = SystemRandom::new();
        let (rp, origin, challenge) = ("tauri.localhost", "http://tauri.localhost", [7u8; 32]);
        let client = |kind: &str, ch: &[u8], from: &str| {
            serde_json::to_vec(&serde_json::json!({ "type": kind, "challenge": URL_SAFE_NO_PAD.encode(ch), "origin": from })).unwrap()
        };
        let auth = |rp: &str, flags: u8| [&Sha256::digest(rp.as_bytes())[..], &[flags, 0, 0, 0, 1]].concat();
        let signed = |a: &[u8], c: &[u8]| [a, &Sha256::digest(c)[..]].concat();

        let ec = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap().as_ref(), &rng).unwrap();
        let ec_spki = [&vault::unhex("3059301306072a8648ce3d020106082a8648ce3d030107034200").unwrap()[..], ec.public_key().as_ref()].concat();
        let ed = Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref()).unwrap();
        let ed_spki = [&vault::unhex("302a300506032b6570032100").unwrap()[..], ed.public_key().as_ref()].concat();
        assert_eq!(spki_key(&ec_spki), Some(ec.public_key().as_ref()));
        assert_eq!(spki_key(&ed_spki), Some(ed.public_key().as_ref()));
        assert!(spki_key(&ec_spki[..40]).is_none() && spki_key(b"").is_none());

        let (a, c) = (auth(rp, 0x05), client("webauthn.get", &challenge, origin));
        let ec_sig = ec.sign(&rng, &signed(&a, &c)).unwrap();
        let ed_sig = ed.sign(&signed(&a, &c));
        assert!(assertion(-7, &ec_spki, rp, origin, &challenge, &a, &c, ec_sig.as_ref()).is_ok());
        assert!(assertion(-8, &ed_spki, rp, origin, &challenge, &a, &c, ed_sig.as_ref()).is_ok());
        assert!(assertion(-8, &ed_spki, rp, origin, &[8; 32], &a, &c, ed_sig.as_ref()).is_err(), "another challenge");
        assert!(assertion(-8, &ed_spki, rp, "http://evil.localhost", &challenge, &a, &c, ed_sig.as_ref()).is_err(), "another origin");
        assert!(assertion(-8, &ed_spki, "evil.localhost", origin, &challenge, &a, &c, ed_sig.as_ref()).is_err(), "another relying party");
        assert!(assertion(-7, &ed_spki, rp, origin, &challenge, &a, &c, ed_sig.as_ref()).is_err(), "another algorithm");
        let mut bad = ed_sig.as_ref().to_vec();
        bad[0] ^= 1;
        assert!(assertion(-8, &ed_spki, rp, origin, &challenge, &a, &c, &bad).is_err(), "a changed signature");
        let unverified = auth(rp, 0x01);
        let sig = ed.sign(&signed(&unverified, &c));
        assert!(assertion(-8, &ed_spki, rp, origin, &challenge, &unverified, &c, sig.as_ref()).is_err(), "user not verified");

        let h = vault::hash_secret("1234").unwrap();
        assert!(vault::check_secret("1234", &h) && !vault::check_secret("1235", &h) && !vault::check_secret("1234", "junk"));
    }
}
