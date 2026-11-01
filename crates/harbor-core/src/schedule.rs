//! Scheduled backups as systemd user timers. Harbor writes a oneshot service that runs
//! `<binary> backup …` and a timer; nothing runs as root.
use crate::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub profile_id: String,
    pub profile_name: String,
    /// systemd OnCalendar expression, e.g. "daily" or "*-*-* 02:30".
    pub calendar: String,
    /// Number of backups of this profile kept in `dir`.
    pub keep: u32,
    /// Empty = all user databases.
    pub databases: Vec<String>,
    pub dir: PathBuf,
    /// File with the server password (mode 0600); None for socket authentication.
    pub password_file: Option<PathBuf>,
    /// File with the archive encryption password (mode 0600).
    pub encryption_file: Option<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledSchedule {
    /// Unit name without suffix ("harbor-backup-prod").
    pub name: String,
    pub description: String,
    pub calendar: String,
    /// `systemctl --user is-active` of the timer; None when it cannot be asked.
    pub active: Option<bool>,
}
pub fn unit_name(profile_name: &str) -> String {
    format!("harbor-backup-{}", backup::slug(profile_name))
}
/// `$XDG_CONFIG_HOME/systemd/user` or `~/.config/systemd/user`.
pub fn unit_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("systemd/user")
}
/// One ExecStart argument, quoted for systemd (specifiers and variables escaped).
fn systemd_arg(value: &str) -> Result<String> {
    if value.chars().any(char::is_control) {
        return Err(Error::Validation(
            "Hodnota pro plán nesmí obsahovat řídicí znaky.".into(),
        ));
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    ))
}
/// The service and timer unit files (text) for a schedule.
pub fn render_units(schedule: &Schedule, binary: &Path) -> Result<(String, String)> {
    if schedule.profile_id.is_empty() {
        return Err(Error::Validation(
            "Plán potřebuje uložený profil (uložte ho v Připojení).".into(),
        ));
    }
    let calendar = schedule.calendar.trim();
    if calendar.is_empty() || calendar.chars().any(|c| c.is_control()) {
        return Err(Error::Validation(
            "Zadejte čas spuštění (např. daily nebo *-*-* 02:30).".into(),
        ));
    }
    if schedule.keep == 0 {
        return Err(Error::Validation(
            "Počet uchovávaných záloh musí být alespoň 1.".into(),
        ));
    }
    let mut args = vec![
        binary.display().to_string(),
        "backup".into(),
        "--profile-id".into(),
        schedule.profile_id.clone(),
        "--dir".into(),
        schedule.dir.display().to_string(),
        "--keep".into(),
        schedule.keep.to_string(),
    ];
    if !schedule.databases.is_empty() {
        for name in &schedule.databases {
            identifier(name)?;
        }
        args.push("--databases".into());
        args.push(schedule.databases.join(","));
    }
    if let Some(file) = &schedule.password_file {
        args.push("--password-file".into());
        args.push(file.display().to_string());
    }
    if let Some(file) = &schedule.encryption_file {
        args.push("--encrypt-password-file".into());
        args.push(file.display().to_string());
    }
    let exec = args
        .iter()
        .map(|a| systemd_arg(a))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    let description = safe_text(&schedule.profile_name).replace('%', "%%");
    let service = format!(
        "# Vytvořeno Harborem; ručně neupravujte, změňte plán v Harboru.\n\
         [Unit]\n\
         Description=Harbor: záloha MariaDB ({description})\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={exec}\n\
         Nice=10\n\
         IOSchedulingClass=idle\n\
         UMask=0077\n"
    );
    let timer = format!(
        "# Vytvořeno Harborem; ručně neupravujte, změňte plán v Harboru.\n\
         [Unit]\n\
         Description=Harbor: plán zálohy ({description})\n\
         \n\
         [Timer]\n\
         OnCalendar={calendar}\n\
         Persistent=true\n\
         RandomizedDelaySec=5min\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    );
    Ok((service, timer))
}
/// Checks an OnCalendar expression with `systemd-analyze calendar`.
pub fn validate_calendar(calendar: &str) -> Result<()> {
    let output = Command::new("systemd-analyze")
        .args(["calendar", "--iterations=1", calendar.trim()])
        .output()
        .map_err(|_| {
            Error::Unsupported("systemd není k dispozici (chybí systemd-analyze).".into())
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Validation(format!(
            "Neplatný čas spuštění „{}“ (příklady: daily, weekly, *-*-* 02:30, Mon..Fri 22:00).",
            safe_text(calendar)
        )))
    }
}
/// Writes the unit files into `dir` (atomically) and returns their paths.
pub fn write_units(
    dir: &Path,
    name: &str,
    service: &str,
    timer: &str,
) -> Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(dir).map_err(|e| Error::File(format!("adresář systemd: {e}")))?;
    let write = |suffix: &str, text: &str| -> Result<PathBuf> {
        let path = dir.join(format!("{name}.{suffix}"));
        let temporary = dir.join(format!(".{name}.{suffix}.tmp"));
        fs::write(&temporary, text).map_err(|e| Error::File(format!("zápis {suffix}: {e}")))?;
        fs::rename(&temporary, &path).map_err(|e| Error::File(format!("zápis {suffix}: {e}")))?;
        Ok(path)
    };
    Ok((write("service", service)?, write("timer", timer)?))
}
fn systemctl(args: &[&str]) -> Result<String> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|_| Error::Unsupported("systemctl není k dispozici.".into()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(Error::Unsupported(format!(
            "systemctl --user {} selhal: {}",
            args.join(" "),
            safe_text(String::from_utf8_lossy(&output.stderr).trim())
        )))
    }
}
/// Installs and starts the timer. Returns a note for the user.
pub fn install(schedule: &Schedule, binary: &Path) -> Result<String> {
    validate_calendar(&schedule.calendar)?;
    for file in [&schedule.password_file, &schedule.encryption_file]
        .into_iter()
        .flatten()
    {
        cli::read_secret_file(file)?;
    }
    let (service, timer) = render_units(schedule, binary)?;
    let name = unit_name(&schedule.profile_name);
    write_units(&unit_dir(), &name, &service, &timer)?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", &format!("{name}.timer")])?;
    let mut note = format!("Plán {name} je aktivní ({}).", schedule.calendar.trim());
    if !linger_enabled() {
        note.push_str(" Zálohy poběží jen když jste přihlášeni; pro běh i bez přihlášení spusťte: loginctl enable-linger");
    }
    Ok(note)
}
/// Stops the timer and removes both unit files.
pub fn remove(name: &str) -> Result<()> {
    if !name.starts_with("harbor-backup-") || name.contains('/') {
        return Err(Error::Validation("Neplatný název plánu.".into()));
    }
    let _ = systemctl(&["disable", "--now", &format!("{name}.timer")]);
    let dir = unit_dir();
    for suffix in ["timer", "service"] {
        let path = dir.join(format!("{name}.{suffix}"));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| Error::File(format!("mazání {suffix}: {e}")))?;
        }
    }
    let _ = systemctl(&["daemon-reload"]);
    Ok(())
}
/// Harbor timers found in the user unit directory.
pub fn list() -> Vec<InstalledSchedule> {
    list_in(&unit_dir(), true)
}
pub(crate) fn list_in(dir: &Path, ask_systemd: bool) -> Vec<InstalledSchedule> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<InstalledSchedule> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(".timer")?.to_string();
            if !name.starts_with("harbor-backup-") {
                return None;
            }
            let text = fs::read_to_string(e.path()).ok()?;
            let value = |key: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(key))
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            Some(InstalledSchedule {
                active: ask_systemd.then(|| {
                    systemctl(&["is-active", &format!("{name}.timer")]).is_ok_and(|s| s == "active")
                }),
                description: value("Description="),
                calendar: value("OnCalendar="),
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
fn linger_enabled() -> bool {
    let uid = unsafe { libc::getuid() };
    Command::new("loginctl")
        .args(["show-user", &uid.to_string(), "--property=Linger"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "Linger=yes")
}
/// The CLI binary to put into the service: the running executable.
pub fn current_binary() -> Result<PathBuf> {
    std::env::current_exe().map_err(|e| Error::File(format!("cesta k programu: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn schedule() -> Schedule {
        Schedule {
            profile_id: "p1".into(),
            profile_name: "Produkce 100%".into(),
            calendar: "*-*-* 02:30".into(),
            keep: 7,
            databases: vec!["shop".into(), "crm".into()],
            dir: PathBuf::from("/home/a b/zálohy $x"),
            password_file: Some(PathBuf::from("/home/a/.harbor-pass")),
            encryption_file: None,
        }
    }
    #[test]
    fn units_quote_arguments_for_systemd() {
        let (service, timer) = render_units(&schedule(), Path::new("/usr/bin/harbor-tui")).unwrap();
        assert!(service.contains(
            "ExecStart=\"/usr/bin/harbor-tui\" \"backup\" \"--profile-id\" \"p1\" \"--dir\" \"/home/a b/zálohy $$x\" \"--keep\" \"7\" \"--databases\" \"shop,crm\" \"--password-file\" \"/home/a/.harbor-pass\""
        ), "{service}");
        assert!(service.contains("Description=Harbor: záloha MariaDB (Produkce 100%%)"));
        assert!(timer.contains("OnCalendar=*-*-* 02:30") && timer.contains("Persistent=true"));
        assert_eq!(unit_name("Produkce 100%"), "harbor-backup-produkce-100");
        let mut bad = schedule();
        bad.profile_id.clear();
        assert!(render_units(&bad, Path::new("/x")).is_err());
        let mut bad = schedule();
        bad.databases = vec!["a/b".into()];
        assert!(render_units(&bad, Path::new("/x")).is_err());
        let mut bad = schedule();
        bad.dir = PathBuf::from("/tmp/a\nExecStart=/bin/rm");
        assert!(render_units(&bad, Path::new("/x")).is_err());
    }
    #[test]
    fn written_units_are_listed() {
        let dir = std::env::temp_dir().join(format!("harbor-units-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (service, timer) = render_units(&schedule(), Path::new("/usr/bin/harbor-tui")).unwrap();
        write_units(&dir, "harbor-backup-produkce-100", &service, &timer).unwrap();
        fs::write(dir.join("other.timer"), "[Timer]\nOnCalendar=daily\n").unwrap();
        let listed = list_in(&dir, false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].calendar, "*-*-* 02:30");
        assert!(listed[0].description.contains("Produkce"));
        fs::remove_dir_all(dir).unwrap();
    }
}
#[cfg(test)]
mod systemd_check {
    use super::*;
    /// Writes units for `systemd-analyze verify` when HARBOR_UNIT_CHECK_DIR is set.
    #[test]
    fn write_units_for_external_verification() {
        let Some(dir) = std::env::var_os("HARBOR_UNIT_CHECK_DIR") else {
            return;
        };
        let binary = std::fs::canonicalize("../../target/debug/harbor-tui").unwrap();
        let plan = Schedule {
            profile_id: "p1".into(),
            profile_name: "Test 100% \"prod\"".into(),
            calendar: "Mon..Fri *-*-* 02:30".into(),
            keep: 3,
            databases: vec!["shop".into()],
            dir: PathBuf::from("/tmp/zálohy a $x"),
            password_file: None,
            encryption_file: None,
        };
        let (service, timer) = render_units(&plan, &binary).unwrap();
        write_units(
            Path::new(&dir),
            &unit_name(&plan.profile_name),
            &service,
            &timer,
        )
        .unwrap();
    }
}
