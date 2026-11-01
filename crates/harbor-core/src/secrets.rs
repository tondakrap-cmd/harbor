//! Optional password storage in the desktop's secret service (GNOME Keyring, KWallet)
//! through the freedesktop Secret Service API. Never a file: when no secret service is
//! running (servers, SSH sessions), Harbor says so and asks for the password again.
use crate::*;

const SERVICE: &str = "harbor";

fn entry(profile_id: &str) -> Result<keyring::Entry> {
    if profile_id.is_empty() {
        return Err(Error::Validation(
            "Heslo lze zapamatovat jen pro uložený profil.".into(),
        ));
    }
    keyring::Entry::new(SERVICE, &format!("profil {profile_id}")).map_err(unavailable)
}
fn unavailable(error: keyring::Error) -> Error {
    Error::Unsupported(format!(
        "systémové úložiště hesel není dostupné ({}); heslo zadejte ručně",
        safe_text(&error.to_string())
    ))
}
/// Stores (or replaces) the password of a saved profile.
pub fn store_password(profile_id: &str, password: &Secret) -> Result<()> {
    if password.0.is_empty() {
        return Ok(());
    }
    entry(profile_id)?
        .set_password(&password.0)
        .map_err(unavailable)
}
/// The stored password, or None when nothing is stored for the profile.
pub fn load_password(profile_id: &str) -> Result<Option<Secret>> {
    match entry(profile_id)?.get_password() {
        Ok(password) => Ok(Some(Secret::new(password))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(unavailable(error)),
    }
}
/// Removes a stored password; succeeds when there was none.
pub fn delete_password(profile_id: &str) -> Result<()> {
    match entry(profile_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(unavailable(error)),
    }
}
/// The password to use for a profile: the typed one, else the stored one when the
/// profile remembers it.
pub fn password_for(profile: &Profile, typed: Secret) -> Result<Secret> {
    if !typed.0.is_empty() || !profile.remember_password || profile.id.is_empty() {
        return Ok(typed);
    }
    Ok(load_password(&profile.id)?.unwrap_or(typed))
}
