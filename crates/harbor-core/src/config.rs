use crate::{Error, Profile, Result};
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
        }
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
            Ok(data) => serde_json::from_slice(&data).map_err(|_| {
                Error::Config("settings.json má neplatný formát; nebyl přepsán.".into())
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(_) => Err(Error::Config("Nelze číst nastavení.".into())),
        }
    }
    // Lock + read/modify/write avoids one client overwriting another client's theme/profile changes.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings> {
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
        change(&mut settings);
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
        Ok(settings)
    }
}
fn config_io(_: std::io::Error) -> Error {
    Error::Config("Nelze bezpečně uložit konfiguraci.".into())
}
