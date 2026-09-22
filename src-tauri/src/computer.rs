//! Stable, application-specific computer identity. Host names and user names can change
//! or collide, so paths in a shared project store are keyed by the OS installation ID.

use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub(crate) fn id() -> Result<&'static str, String> {
    static ID: OnceLock<Result<String, String>> = OnceLock::new();
    ID.get_or_init(|| {
        let raw = os_id()
            .map_err(|error| format!("Cannot read this computer's OS identifier: {error}"))?;
        application_id(std::env::consts::OS, &raw)
    })
    .as_deref()
    .map_err(Clone::clone)
}

fn application_id(os: &str, raw: &str) -> Result<String, String> {
    let normalized: String = raw
        .trim()
        .trim_matches(['{', '}'])
        .chars()
        .filter(|ch| *ch != '-')
        .flat_map(char::to_lowercase)
        .collect();
    if normalized.len() != 32
        || !normalized.bytes().all(|ch| ch.is_ascii_hexdigit())
        || normalized.bytes().all(|ch| ch == b'0')
    {
        return Err("The OS computer identifier is missing or invalid".into());
    }
    // Do not put the raw, system-wide identifier into a repository shared through Git.
    let digest = Sha256::digest(format!("adashi:computer:v1:{os}:{normalized}"));
    Ok(format!("{os}:{digest:x}"))
}

#[cfg(windows)]
fn os_id() -> Result<String, std::io::Error> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        )?
        .get_value("MachineGuid")
}

#[cfg(target_os = "linux")]
fn os_id() -> Result<String, std::io::Error> {
    // Older non-systemd installations keep the same identity in the D-Bus location.
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(value) = std::fs::read_to_string(path) {
            if application_id("linux", &value).is_ok() {
                return Ok(value);
            }
        }
    }
    Err(std::io::Error::other("No valid machine-id was found"))
}

#[cfg(target_os = "macos")]
fn os_id() -> Result<String, std::io::Error> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()?;
    if output.status.success() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "\"IOPlatformUUID\"" {
                    return Ok(value.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    Err(std::io::Error::other("IOPlatformUUID was not available"))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn os_id() -> Result<String, std::io::Error> {
    Err(std::io::Error::other(
        "This OS has no supported computer identifier",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computer_identity_is_stable_namespaced_and_does_not_expose_the_os_id() {
        let raw = "AABBCCDD-1234-5678-90AB-112233445566";
        let id = application_id("windows", raw).unwrap();
        assert_eq!(
            id,
            application_id("windows", &format!("{{{raw}}}\n")).unwrap()
        );
        assert_eq!(id, application_id("windows", &raw.to_lowercase()).unwrap());
        assert_ne!(id, application_id("linux", raw).unwrap());
        assert!(!id.contains("aabbccdd"));
        assert!(application_id("linux", "uninitialized").is_err());
        assert!(application_id("linux", &"0".repeat(32)).is_err());
    }

    #[test]
    fn current_computer_has_a_stable_os_identifier() {
        assert_eq!(id().unwrap(), id().unwrap());
        assert!(id().unwrap().starts_with(std::env::consts::OS));
    }
}
