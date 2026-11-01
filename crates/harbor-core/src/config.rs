use crate::{new_profile_id, Error, Profile, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub profiles: Vec<Profile>,
    pub desktop_theme: String,
    pub tui_theme: String,
    pub refresh_seconds: u64,
    pub terminal_colors: String,
    pub ascii: bool,
    /// Keys written by a newer Harbor version (desktop and TUI packages can differ);
    /// kept on read-modify-write instead of being dropped.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            desktop_theme: "original".into(),
            tui_theme: "turbo_classic".into(),
            refresh_seconds: 5,
            terminal_colors: "auto".into(),
            ascii: false,
            extra: serde_json::Map::new(),
        }
    }
}
impl Settings {
    /// Profiles saved before ids existed get a deterministic id, so repeated loads agree
    /// until the next write persists it.
    fn normalize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for profile in &mut self.profiles {
            if profile.id.is_empty() || seen.contains(&profile.id) {
                let base = format!("legacy:{}", profile.name);
                let mut candidate = base.clone();
                let mut n = 2;
                while seen.contains(&candidate) {
                    candidate = format!("{base}#{n}");
                    n += 1;
                }
                profile.id = candidate;
            }
            seen.insert(profile.id.clone());
        }
    }
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }
    /// Inserts a new profile (empty id) or replaces the one with the same id.
    /// A name used by a different profile is rejected instead of silently replacing it.
    pub fn upsert_profile(&mut self, mut profile: Profile) -> Result<String> {
        profile.validate()?;
        let name = profile.name.trim().to_lowercase();
        if self
            .profiles
            .iter()
            .any(|p| p.id != profile.id && p.name.trim().to_lowercase() == name)
        {
            return Err(Error::Validation(
                "Profil s tímto názvem už existuje; zvolte jiný název.".into(),
            ));
        }
        if profile.id.is_empty() {
            profile.id = new_profile_id();
            while self.profile(&profile.id).is_some() {
                profile.id = new_profile_id();
            }
        }
        let id = profile.id.clone();
        match self.profiles.iter_mut().find(|p| p.id == id) {
            Some(existing) => {
                // Forms only know the fields of this version; keep newer unknown ones.
                for (key, value) in &existing.extra {
                    profile
                        .extra
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
                *existing = profile;
            }
            None => self.profiles.push(profile),
        }
        Ok(id)
    }
    pub fn remove_profile(&mut self, id: &str) -> Result<Profile> {
        let index = self
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| Error::Validation("Profil už v konfiguraci neexistuje.".into()))?;
        Ok(self.profiles.remove(index))
    }
}
#[derive(Clone)]
pub struct ConfigStore {
    directory: PathBuf,
}
impl ConfigStore {
    pub fn standard() -> Result<Self> {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| Error::Config("HOME ani XDG_CONFIG_HOME nejsou nastavené.".into()))?;
        Ok(Self {
            directory: root.join("harbor"),
        })
    }
    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }
    pub fn load(&self) -> Result<Settings> {
        match fs::read(self.directory.join("settings.json")) {
            Ok(data) => serde_json::from_slice::<Settings>(&data)
                .map(|mut settings| {
                    settings.normalize();
                    settings
                })
                .map_err(|_| {
                    Error::Config("settings.json má neplatný formát; nebyl přepsán.".into())
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(_) => Err(Error::Config("Nelze číst nastavení.".into())),
        }
    }
    // Lock + read/modify/write avoids one client overwriting another client's theme/profile changes.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings> {
        self.try_update(|settings| {
            change(settings);
            Ok(())
        })
        .map(|(settings, ())| settings)
    }
    /// Like `update`, but a failed change leaves the file untouched.
    pub fn try_update<T>(
        &self,
        change: impl FnOnce(&mut Settings) -> Result<T>,
    ) -> Result<(Settings, T)> {
        fs::create_dir_all(&self.directory).map_err(config_io)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("settings.lock"))
            .map_err(config_io)?;
        lock.lock_exclusive().map_err(config_io)?;
        let mut settings = self.load()?;
        let value = change(&mut settings)?;
        let path = self.directory.join("settings.json.tmp");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(config_io)?;
        file.write_all(
            &serde_json::to_vec_pretty(&settings)
                .map_err(|_| Error::Config("Serializace nastavení selhala.".into()))?,
        )
        .map_err(config_io)?;
        file.sync_all().map_err(config_io)?;
        fs::rename(path, self.directory.join("settings.json")).map_err(config_io)?;
        std::fs::File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(config_io)?;
        Ok((settings, value))
    }
}
fn config_io(_: std::io::Error) -> Error {
    Error::Config("Nelze bezpečně uložit konfiguraci.".into())
}
