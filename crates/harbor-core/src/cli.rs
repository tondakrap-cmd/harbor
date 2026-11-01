//! Command line for unattended work (cron, systemd timers, scripts). Shared by both
//! binaries: `harbor backup …` works like `harbor-tui backup …`.
use crate::*;
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

pub const USAGE: &str = "Příkazy bez uživatelského rozhraní:
  backup  --profile NÁZEV | --profile-id ID [--databases a,b] [--dir ADRESÁŘ] [--keep N]
          [--password-file SOUBOR] [--encrypt-password-file SOUBOR]
  restore SOUBOR.zip --profile NÁZEV | --profile-id ID --database DB [--target NÁZEV]
          [--replace] [--password-file SOUBOR] [--encrypt-password-file SOUBOR]
  verify  SOUBOR.zip [--encrypt-password-file SOUBOR]
  backups [--dir ADRESÁŘ]

Heslo se nikdy nepředává argumentem: --password-file (nebo proměnná HARBOR_PASSWORD_FILE)
ukazuje na soubor čitelný jen vlastníkem (chmod 600). Bez něj se použije připojení bez hesla
(např. root přes Unix socket). Výchozí adresář záloh: ~/.local/share/harbor/backups.
Návratový kód: 0 úspěch, 1 chyba, 2 špatné použití.";

/// Runs a command-line subcommand. None when `args` is not one (start the UI instead).
pub fn run(args: &[String]) -> Option<i32> {
    let command = args.first()?.as_str();
    if !["backup", "restore", "verify", "backups"].contains(&command) {
        return None;
    }
    let options = match Options::parse(&args[1..]) {
        Ok(options) => options,
        Err(Error::Validation(message)) => {
            eprintln!("{message}\n\n{USAGE}");
            return Some(2);
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return Some(2);
        }
    };
    let result = match command {
        "backup" => backup(&options),
        "restore" => restore(&options),
        "verify" => verify(&options),
        _ => list(&options),
    };
    Some(match result {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(Error::Validation(message)) if message.starts_with("Použití:") => {
            eprintln!("{message}\n\n{USAGE}");
            2
        }
        Err(error) => {
            eprintln!("Chyba: {error}");
            1
        }
    })
}
#[derive(Default, Debug)]
struct Options {
    positional: Vec<String>,
    profile: Option<String>,
    profile_id: Option<String>,
    databases: Vec<String>,
    dir: Option<PathBuf>,
    keep: Option<usize>,
    password_file: Option<PathBuf>,
    encryption_file: Option<PathBuf>,
    database: Option<String>,
    target: Option<String>,
    replace: bool,
}
fn usage(message: &str) -> Error {
    Error::Validation(format!("Použití: {message}"))
}
impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let mut value = |name: &str| {
                iter.next()
                    .cloned()
                    .ok_or_else(|| usage(&format!("{name} vyžaduje hodnotu.")))
            };
            match arg.as_str() {
                "--profile" => options.profile = Some(value(arg)?),
                "--profile-id" => options.profile_id = Some(value(arg)?),
                "--databases" => {
                    options.databases = value(arg)?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                }
                "--dir" => options.dir = Some(PathBuf::from(value(arg)?)),
                "--keep" => {
                    options.keep = Some(
                        value(arg)?
                            .parse()
                            .ok()
                            .filter(|n| *n > 0)
                            .ok_or_else(|| usage("--keep musí být kladné číslo."))?,
                    )
                }
                "--password-file" => options.password_file = Some(PathBuf::from(value(arg)?)),
                "--encrypt-password-file" => {
                    options.encryption_file = Some(PathBuf::from(value(arg)?))
                }
                "--database" => options.database = Some(value(arg)?),
                "--target" => options.target = Some(value(arg)?),
                "--replace" => options.replace = true,
                "--password" => {
                    return Err(usage(
                        "heslo nezadávejte na příkazové řádce (je vidět v seznamu procesů); použijte --password-file.",
                    ))
                }
                other if other.starts_with("--") => {
                    return Err(usage(&format!("neznámá volba {other}.")))
                }
                other => options.positional.push(other.to_string()),
            }
        }
        if options.password_file.is_none() {
            options.password_file = std::env::var_os("HARBOR_PASSWORD_FILE").map(PathBuf::from);
        }
        Ok(options)
    }
    fn dir(&self) -> PathBuf {
        self.dir.clone().unwrap_or_else(default_backup_dir)
    }
    fn profile(&self) -> Result<Profile> {
        let settings = ConfigStore::standard()?.load()?;
        let found = match (&self.profile_id, &self.profile) {
            (Some(id), _) => settings.profile(id).cloned(),
            (None, Some(name)) => settings
                .profiles
                .iter()
                .find(|p| p.name.trim().eq_ignore_ascii_case(name.trim()))
                .cloned(),
            (None, None) => return Err(usage("zadejte --profile nebo --profile-id.")),
        };
        found.ok_or_else(|| {
            Error::Validation(
                "Profil neexistuje. Uložte ho v Harboru (Připojení / Profily).".into(),
            )
        })
    }
    fn password(&self) -> Result<Secret> {
        match &self.password_file {
            Some(path) => read_secret_file(path),
            None => Ok(Secret::default()),
        }
    }
    fn encryption(&self) -> Result<Option<Secret>> {
        self.encryption_file
            .as_deref()
            .map(read_secret_file)
            .transpose()
    }
}
/// Reads a password file; refuses files readable by others or owned by someone else.
pub fn read_secret_file(path: &Path) -> Result<Secret> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)
        .map_err(|e| Error::File(format!("soubor s heslem {}: {e}", path.display())))?;
    if metadata.mode() & 0o077 != 0 {
        return Err(Error::Validation(format!(
            "Soubor s heslem {} smí číst jen vlastník (chmod 600).",
            path.display()
        )));
    }
    if metadata.uid() != unsafe { libc::getuid() } {
        return Err(Error::Validation(format!(
            "Soubor s heslem {} patří jinému uživateli.",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::File(format!("soubor s heslem {}: {e}", path.display())))?;
    let secret = Secret::new(text.trim_end_matches(['\n', '\r']).to_string());
    if secret.0.is_empty() {
        return Err(Error::Validation(format!(
            "Soubor s heslem {} je prázdný.",
            path.display()
        )));
    }
    Ok(secret)
}
/// Waits for a job; on a terminal shows a progress line, in logs only phase changes.
fn wait(job: &Job) -> Result<String> {
    let tty = std::io::stderr().is_terminal();
    let mut last_phase = String::new();
    let mut last_print = Instant::now() - Duration::from_secs(10);
    let result = job.wait(|progress| {
        if progress.phase != last_phase && !tty {
            eprintln!("{}", progress.phase);
        }
        if tty && last_print.elapsed() >= Duration::from_millis(500) {
            eprint!(
                "\r{:<60} {:>3} %",
                progress.phase.chars().take(60).collect::<String>(),
                (progress.fraction() * 100.0) as u32
            );
            let _ = std::io::stderr().flush();
            last_print = Instant::now();
        }
        last_phase.clone_from(&progress.phase);
    });
    if tty {
        eprintln!();
    }
    result
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Worker)
}
fn backup(options: &Options) -> Result<String> {
    let profile = options.profile()?;
    let password = options.password()?;
    let databases = if options.databases.is_empty() {
        // All user databases the account can see.
        let snapshot = runtime()?.block_on(async {
            let mut backend = Backend::connect(&profile, password.clone()).await?;
            backend.snapshot().await
        })?;
        snapshot
            .databases
            .into_iter()
            .filter(|d| !d.system())
            .map(|d| d.name)
            .collect()
    } else {
        options.databases.clone()
    };
    if databases.is_empty() {
        return Err(Error::Validation(
            "Účet nevidí žádnou uživatelskou databázi; zadejte --databases.".into(),
        ));
    }
    let dir = options.dir();
    let path = dir.join(backup_file_name(&profile.name, SystemTime::now()));
    let job = start_job(
        JobSpec::Backup(BackupRequest {
            profile: profile.name.clone(),
            databases,
            path,
            encryption: options.encryption()?,
        }),
        Some((&profile, password)),
    )?;
    let mut message = wait(&job)?;
    if let Some(keep) = options.keep {
        let removed = prune_backups(&dir, &profile.name, keep)?;
        if !removed.is_empty() {
            message.push_str(&format!(" Smazáno starých záloh: {}.", removed.len()));
        }
    }
    Ok(message)
}
fn archive(options: &Options) -> Result<PathBuf> {
    match options.positional.as_slice() {
        [path] => Ok(PathBuf::from(path)),
        _ => Err(usage("zadejte právě jeden soubor zálohy.")),
    }
}
fn restore(options: &Options) -> Result<String> {
    let path = archive(options)?;
    let database = options
        .database
        .clone()
        .ok_or_else(|| usage("zadejte --database (databázi ze zálohy)."))?;
    let profile = options.profile()?;
    let job = start_job(
        JobSpec::Restore(RestoreRequest {
            path,
            encryption: options.encryption()?,
            target: options.target.clone().unwrap_or_else(|| database.clone()),
            database,
            replace: options.replace,
        }),
        Some((&profile, options.password()?)),
    )?;
    wait(&job)
}
fn verify(options: &Options) -> Result<String> {
    let job = start_job(
        JobSpec::Verify {
            path: archive(options)?,
            encryption: options.encryption()?,
        },
        None,
    )?;
    wait(&job)
}
fn list(options: &Options) -> Result<String> {
    let dir = options.dir();
    let files = list_backups(&dir);
    if files.is_empty() {
        return Ok(format!("V {} nejsou žádné zálohy.", dir.display()));
    }
    let mut lines = Vec::new();
    for file in files {
        lines.push(match &file.manifest {
            Ok(m) => format!(
                "{}  {}  {}  {}{}",
                file.name(),
                human_bytes(file.bytes),
                m.created_utc,
                m.databases
                    .iter()
                    .map(|d| d.name.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                if m.encrypted { "  [šifrováno]" } else { "" }
            ),
            Err(error) => format!(
                "{}  {}  nečitelná: {error}",
                file.name(),
                human_bytes(file.bytes)
            ),
        });
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn subcommands_are_recognized_and_passwords_never_come_from_arguments() {
        assert_eq!(run(&args(&["--demo"])), None);
        assert_eq!(run(&[]), None);
        assert_eq!(run(&args(&["backup", "--password", "x"])), Some(2));
        assert_eq!(run(&args(&["backup", "--keep", "0"])), Some(2));
        assert_eq!(run(&args(&["verify"])), Some(2));
        let parsed =
            Options::parse(&args(&["--databases", "a, b,", "--keep", "3", "x.zip"])).unwrap();
        assert_eq!(parsed.databases, ["a", "b"]);
        assert_eq!(parsed.keep, Some(3));
        assert_eq!(parsed.positional, ["x.zip"]);
    }
    #[test]
    fn password_files_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("harbor-secret-{}", std::process::id()));
        std::fs::write(&path, "tajné\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_secret_file(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_secret_file(&path).unwrap().0.as_str(), "tajné");
        std::fs::write(&path, "\n").unwrap();
        assert!(read_secret_file(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
