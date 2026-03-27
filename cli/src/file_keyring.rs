//! Encrypted-file fallback for OS keyring.
//!
//! When the OS keyring (gnome-keyring, kwallet, etc.) is unavailable — common
//! on headless Linux, Docker, and minimal distros — credentials are stored in
//! `~/.onchainos/keyring.enc` encrypted with AES-256-GCM.
//!
//! Key derivation: `scrypt(machine_id + username, random_salt) -> 32-byte key`
//!
//! Machine ID priority:
//!   1. /etc/machine-id
//!   2. /var/lib/dbus/machine-id
//!   3. hostname

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::home::onchainos_home;

const KEYRING_FILE: &str = "keyring.enc";
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SCRYPT_LOG_N: u8 = 15;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

// --------------- identity helpers ---------------

fn machine_identity() -> String {
    let machine_id = read_machine_id();
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "onchainos-user".to_string());
    format!("{machine_id}:{username}")
}

fn read_machine_id() -> String {
    if let Ok(id) = fs::read_to_string("/etc/machine-id") {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    if let Ok(id) = fs::read_to_string("/var/lib/dbus/machine-id") {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    hostname()
}

fn hostname() -> String {
    if let Ok(h) = fs::read_to_string("/proc/sys/kernel/hostname") {
        let trimmed = h.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: buf is a valid mutable [u8; 256] on the stack.
        // gethostname writes at most buf.len() bytes including the NUL terminator.
        let ret = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if ret == 0 {
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    "unknown-host".to_string()
}

// --------------- key derivation ---------------

fn derive_key(identity: &str, salt: &[u8]) -> Zeroizing<Vec<u8>> {
    let params =
        scrypt::Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P, 32).expect("valid scrypt params");
    let mut key = Zeroizing::new(vec![0u8; 32]);
    scrypt::scrypt(identity.as_bytes(), salt, &params, &mut key)
        .expect("scrypt output length is valid");
    key
}

// --------------- permissions ---------------

#[cfg(unix)]
fn ensure_dir_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        fs::create_dir_all(path).context("failed to create directory")?;
    }
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if mode != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_dir_permissions(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_file_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        return Ok(());
    }
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if mode != 0o600 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_file_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn check_and_fix_permissions(path: &std::path::Path) -> Result<()> {
    ensure_file_permissions(path)
}

// --------------- public API ---------------

pub fn read_blob() -> Result<HashMap<String, String>> {
    let path = keyring_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    check_and_fix_permissions(&path)?;

    let data = fs::read(&path).context("failed to read keyring.enc")?;
    if data.len() < SALT_LEN + NONCE_LEN + 1 {
        anyhow::bail!("keyring.enc is corrupted (too short)");
    }

    let (salt, rest) = data.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);

    let identity = machine_identity();
    let key = derive_key(&identity, salt);
    let cipher = Aes256Gcm::new_from_slice(&key).context("failed to create AES cipher")?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("failed to decrypt keyring.enc"))?,
    );

    let map: HashMap<String, String> = serde_json::from_slice(&plaintext)?;
    Ok(map)
}

pub fn write_blob(map: &HashMap<String, String>) -> Result<()> {
    let home = onchainos_home()?;
    ensure_dir_permissions(&home)?;

    let path = home.join(KEYRING_FILE);
    let json = Zeroizing::new(serde_json::to_string(map)?);

    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let identity = machine_identity();
    let key = derive_key(&identity, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, json.as_bytes())
        .map_err(|_| anyhow::anyhow!("failed to encrypt keyring blob"))?;

    let mut out = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);

    let tmp = path.with_extension("enc.tmp");
    fs::write(&tmp, &out)?;
    ensure_file_permissions(&tmp)?;
    fs::rename(&tmp, &path)?;

    Ok(())
}

pub fn clear_all() -> Result<()> {
    let path = keyring_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

fn keyring_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join(KEYRING_FILE))
}

// --------------- tests ---------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_identity_contains_separator() {
        let id = machine_identity();
        assert!(id.contains(':'), "expected colon separator in identity");
    }

    #[test]
    fn machine_identity_not_empty() {
        let id = machine_identity();
        assert!(!id.is_empty());
    }

    #[test]
    fn derive_key_produces_32_bytes() {
        let key = derive_key("test-identity", b"test-salt-32-bytes-long-padding!");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn derive_key_deterministic() {
        let salt = b"deterministic-salt-for-testing!!";
        let k1 = derive_key("identity", salt);
        let k2 = derive_key("identity", salt);
        assert_eq!(*k1, *k2);
    }

    #[test]
    fn derive_key_different_identity_produces_different_key() {
        let salt = b"shared-salt-value-32-bytes-long!";
        let k1 = derive_key("alice", salt);
        let k2 = derive_key("bob", salt);
        assert_ne!(*k1, *k2);
    }

    #[test]
    fn derive_key_different_salt_produces_different_key() {
        let k1 = derive_key("identity", b"salt-aaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let k2 = derive_key("identity", b"salt-bbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_ne!(*k1, *k2);
    }

    #[test]
    fn read_machine_id_returns_nonempty() {
        let id = read_machine_id();
        assert!(!id.is_empty());
    }

    #[test]
    fn hostname_returns_nonempty() {
        let h = hostname();
        assert!(!h.is_empty());
    }

    #[test]
    fn roundtrip_write_read_clear() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", dir.path().to_str().unwrap());

        let mut map = HashMap::new();
        map.insert("key1".to_string(), "value1".to_string());
        map.insert("key2".to_string(), "value2".to_string());

        write_blob(&map).unwrap();
        let loaded = read_blob().unwrap();
        assert_eq!(loaded, map);

        clear_all().unwrap();
        let empty = read_blob().unwrap();
        assert!(empty.is_empty());

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn read_blob_returns_empty_when_no_file() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", dir.path().to_str().unwrap());

        let map = read_blob().unwrap();
        assert!(map.is_empty());

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn write_blob_creates_directory() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        std::env::set_var("ONCHAINOS_HOME", sub.to_str().unwrap());

        let mut map = HashMap::new();
        map.insert("k".to_string(), "v".to_string());
        write_blob(&map).unwrap();
        assert!(sub.exists());

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn corrupted_file_returns_error() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", dir.path().to_str().unwrap());

        // Write a file that's too short
        let path = dir.path().join(KEYRING_FILE);
        fs::write(&path, b"short").unwrap();

        let result = read_blob();
        assert!(result.is_err());

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn wrong_identity_cannot_decrypt() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", dir.path().to_str().unwrap());

        let mut map = HashMap::new();
        map.insert("secret".to_string(), "data".to_string());
        write_blob(&map).unwrap();

        // Tamper: overwrite the first byte of salt to change derived key
        let path = dir.path().join(KEYRING_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 0xFF;
        fs::write(&path, &data).unwrap();

        let result = read_blob();
        assert!(result.is_err());

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[cfg(unix)]
    #[test]
    fn file_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", dir.path().to_str().unwrap());

        let mut map = HashMap::new();
        map.insert("k".to_string(), "v".to_string());
        write_blob(&map).unwrap();

        let path = dir.path().join(KEYRING_FILE);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[cfg(unix)]
    #[test]
    fn dir_permissions_are_0700() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("newdir");
        std::env::set_var("ONCHAINOS_HOME", sub.to_str().unwrap());

        let mut map = HashMap::new();
        map.insert("k".to_string(), "v".to_string());
        write_blob(&map).unwrap();

        let mode = fs::metadata(&sub).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);

        std::env::remove_var("ONCHAINOS_HOME");
    }
}
