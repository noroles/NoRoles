//! Signed answers. Each person has an ed25519 key; the private half stays on their computer,
//! encrypted with a passphrase, so an agent running on the same machine cannot sign for them.
//! The files are the same as the JavaScript version writes: encrypted PKCS#8 PEM and SPKI public keys.
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use std::fs;
use std::path::PathBuf;

pub fn key_dir() -> PathBuf {
    match std::env::var("NOROLES_KEYS") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".noroles").join("keys"),
    }
}
pub fn key_path(id: &str) -> PathBuf { key_dir().join(format!("{id}.pem")) }

pub fn keygen(id: &str, passphrase: &str) -> Result<String, String> {
    if passphrase.chars().count() < 8 { return Err("use a passphrase of at least 8 characters".into()); }
    let file = key_path(id);
    if file.exists() { return Err(format!("{} already exists", file.display())); }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    let pem = key.to_pkcs8_encrypted_pem(passphrase, pkcs8::LineEnding::LF).map_err(|e| e.to_string())?;
    let dir = key_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)); }
    write_private(&file, pem.as_bytes())?;
    let spki = key.verifying_key().to_public_key_der().map_err(|e| e.to_string())?;
    Ok(format!("ed25519:{}", B64.encode(spki.as_bytes())))
}

fn write_private(file: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    o.open(file).and_then(|mut f| f.write_all(bytes)).map_err(|e| e.to_string())
}

/// The public key of a key already on this computer (needs its passphrase).
pub fn public_of(id: &str, passphrase: &str) -> Result<String, String> {
    let pem = fs::read_to_string(key_path(id)).map_err(|_| format!("no signing key for {id} on this computer"))?;
    let key = SigningKey::from_pkcs8_encrypted_pem(&pem, passphrase).map_err(|_| "wrong passphrase".to_string())?;
    let spki = key.verifying_key().to_public_key_der().map_err(|e| e.to_string())?;
    Ok(format!("ed25519:{}", B64.encode(spki.as_bytes())))
}

/// What a person signs: the request, the exact action it approves, and the answer.
pub fn statement(id: &str, action_hash: &str, answer: &str, at: &str) -> String {
    format!("noroles/1\n{id}\n{action_hash}\n{answer}\n{at}")
}

pub fn sign(id: &str, passphrase: &str, text: &str) -> Result<String, String> {
    let pem = fs::read_to_string(key_path(id)).map_err(|_| format!("no signing key for {id} on this computer"))?;
    let key = SigningKey::from_pkcs8_encrypted_pem(&pem, passphrase).map_err(|_| "wrong passphrase: nothing was signed".to_string())?;
    Ok(B64.encode(key.sign(text.as_bytes()).to_bytes()))
}

pub fn verify(public: &str, text: &str, sig: &str) -> bool {
    let Some(b) = public.strip_prefix("ed25519:") else { return false };
    let (Ok(der), Ok(s)) = (B64.decode(b), B64.decode(sig)) else { return false };
    let Ok(key) = VerifyingKey::from_public_key_der(&der) else { return false };
    let Ok(s) = Signature::from_slice(&s) else { return false };
    key.verify(text.as_bytes(), &s).is_ok()
}
