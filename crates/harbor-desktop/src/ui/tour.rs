//! Debug builds only: walks through the screens and saves screenshots (PNG) into
//! `HARBOR_TOUR_DIR`. Used to review layouts on large servers under Xvfb.
use super::*;
use std::path::PathBuf;

/// Screenshot name and the state change that prepares it.
type Step = (String, Box<dyn Fn(&mut Harbor)>);
pub struct Tour {
    pub app: Harbor,
    dir: PathBuf,
    steps: Vec<Step>,
    step: usize,
    frames: u32,
    requested: bool,
}
impl Tour {
    pub fn new(ctx: &egui::Context, dir: PathBuf, socket: Option<String>) -> Self {
        let mut app = Harbor::new(ctx, socket.is_none());
        app.store = None;
        if let Ok(port) = std::env::var("HARBOR_TOUR_HOST_DB_PORT") {
            app.session.connect(
                Profile {
                    name: "AlmaLinux 9 přes SSH".into(),
                    user: "harbor".into(),
                    transport: Transport::Tcp {
                        host: "127.0.0.1".into(),
                        port: port.parse().unwrap_or(3306),
                    },
                    tls: TlsMode::InsecureTestOnly,
                    host: HostSettings {
                        access: HostAccess::Ssh {
                            target: "harbor@127.0.0.1".into(),
                            port: std::env::var("HARBOR_TOUR_SSH_PORT")
                                .ok()
                                .and_then(|p| p.parse().ok())
                                .unwrap_or(22),
                        },
                        elevation: Elevation::Sudo,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Secret::new(std::env::var("HARBOR_TOUR_PASSWORD").unwrap_or_default()),
                false,
            );
        } else if let Some(path) = socket {
            app.session.connect(
                Profile {
                    name: "Velký testovací server".into(),
                    user: "root".into(),
                    transport: Transport::Unix { path },
                    ..Default::default()
                },
                Secret::default(),
                false,
            );
        }
        let mut steps: Vec<Step> = Vec::new();
        let only = std::env::var("HARBOR_TOUR_THEMES").unwrap_or_else(|_| "original".into());
        for theme in ThemeId::DESKTOP
            .into_iter()
            .filter(|t| only.contains(t.key()))
        {
            let t = theme;
            let set = move |a: &mut Harbor, page: usize| {
                a.theme = t;
                a.page = page;
            };
            steps.push((
                format!("{}-0-prehled", t.key()),
                Box::new(move |a| set(a, OVERVIEW)),
            ));
            steps.push((
                format!("{}-1-databaze", t.key()),
                Box::new(move |a| {
                    set(a, DATABASES);
                    let first = a.session.snapshot.as_ref().and_then(|s| {
                        s.databases
                            .iter()
                            .find(|d| !d.system())
                            .map(|d| d.name.clone())
                    });
                    if let Some(db) = first {
                        if a.selected_db.as_ref() != Some(&db) {
                            a.session.request(Operation::Tables(db.clone()));
                            a.selected_db = Some(db);
                        }
                    }
                }),
            ));
            steps.push((
                format!("{}-1b-tabulka", t.key()),
                Box::new(move |a| {
                    set(a, DATABASES);
                    let table =
                        a.session.database.as_ref().and_then(|d| {
                            d.tables.first().map(|t| (d.name.clone(), t.name.clone()))
                        });
                    if let Some((db, table)) = table {
                        if a.selected_table.as_ref() != Some(&table) {
                            a.session.request(Operation::Table {
                                database: db,
                                table: table.clone(),
                            });
                            a.selected_table = Some(table);
                        }
                    }
                }),
            ));
            steps.push((
                format!("{}-2-ucty", t.key()),
                Box::new(move |a| {
                    set(a, ACCOUNTS);
                    let account = a.session.snapshot.as_ref().and_then(|s| {
                        s.accounts
                            .iter()
                            .find(|x| x.user == "uzivatel_07")
                            .or(s.accounts.first())
                            .cloned()
                    });
                    if let Some(account) = account {
                        if a.selected_account.as_ref() != Some(&account) {
                            a.session.request(Operation::Grants(account.clone()));
                            a.selected_account = Some(account);
                        }
                    }
                }),
            ));
            steps.push((
                format!("{}-2b-opravneni", t.key()),
                Box::new(move |a| {
                    set(a, ACCOUNTS);
                    if a.privilege_editor.is_none() {
                        if let (Some(account), Some(scope)) = (
                            a.selected_account.clone(),
                            a.session
                                .account
                                .as_ref()
                                .and_then(|d| d.privileges.first().map(|p| p.scope.clone())),
                        ) {
                            a.privilege_editor = Some(PrivilegeEditor {
                                account,
                                fixed: Some(scope),
                                scope_text: String::new(),
                                loaded_for: None,
                                wanted: BTreeSet::new(),
                            });
                        }
                    }
                }),
            ));
            steps.push((
                format!("{}-3-spojeni", t.key()),
                Box::new(move |a| {
                    a.privilege_editor = None;
                    set(a, PROCESSES)
                }),
            ));
            steps.push((
                format!("{}-4-promenne", t.key()),
                Box::new(move |a| {
                    set(a, VARIABLES);
                    a.selected_variable = Some("max_connections".into());
                }),
            ));
            steps.push((
                format!("{}-5-doktor", t.key()),
                Box::new(move |a| set(a, HEALTH)),
            ));
            steps.push((
                format!("{}-3b-zamky", t.key()),
                Box::new(move |a| {
                    set(a, PROCESSES);
                    a.locks_view = true;
                }),
            ));
            steps.push((
                format!("{}-9-vykon", t.key()),
                Box::new(move |a| {
                    a.locks_view = false;
                    set(a, PERFORMANCE);
                    if a.selected_stat.is_none() {
                        a.selected_stat = a
                            .session
                            .performance
                            .as_ref()
                            .and_then(|p| p.statements.first().map(|s| s.query.clone()));
                    }
                }),
            ));
            steps.push((
                format!("{}-9b-replikace", t.key()),
                Box::new(move |a| set(a, REPLICATION)),
            ));
            steps.push((
                format!("{}-9c-konzole", t.key()),
                Box::new(move |a| {
                    set(a, CONSOLE);
                    if a.session.console.is_none() && a.session.snapshot.is_some() && !a.session.worker.busy {
                        a.console_sql = "SELECT TABLE_SCHEMA, TABLE_NAME, ENGINE, TABLE_ROWS FROM information_schema.TABLES ORDER BY TABLE_ROWS DESC".into();
                        let _ = a.session.console("", &a.console_sql.clone(), false);
                    }
                }),
            ));
            steps.push((
                format!("{}-5b-sluzba", t.key()),
                Box::new(move |a| set(a, SERVICE)),
            ));
            steps.push((
                format!("{}-5c-profil", t.key()),
                Box::new(move |a| {
                    if a.page != CONNECTION {
                        if let Some(profile) = a.session.profile.clone() {
                            a.load_profile(profile);
                        }
                    }
                    set(a, CONNECTION)
                }),
            ));
            steps.push((
                format!("{}-6-zalohy", t.key()),
                Box::new(move |a| {
                    set(a, BACKUPS);
                    if a.selected_backup.is_none() {
                        a.selected_backup = a.backups.first().map(|b| b.path.clone());
                    }
                }),
            ));
            steps.push((
                format!("{}-6b-nova-zaloha", t.key()),
                Box::new(move |a| {
                    set(a, BACKUPS);
                    a.backup_form_open = true;
                }),
            ));
            steps.push((
                format!("{}-6c-plan", t.key()),
                Box::new(move |a| {
                    set(a, BACKUPS);
                    a.backup_form_open = false;
                    a.schedule_open = true;
                }),
            ));
            steps.push((
                format!("{}-7-pripojeni", t.key()),
                Box::new(move |a| {
                    a.schedule_open = false;
                    set(a, CONNECTION)
                }),
            ));
            steps.push((
                format!("{}-8-nastaveni", t.key()),
                Box::new(move |a| set(a, SETTINGS)),
            ));
        }
        Self {
            app,
            dir,
            steps,
            step: 0,
            frames: 0,
            requested: false,
        }
    }
}
impl eframe::App for Tour {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let name = &self.steps[self.step].0;
            let path = self.dir.join(format!("{name}.png"));
            std::fs::write(&path, png(&image)).expect("write screenshot");
            eprintln!("snímek {}", path.display());
            self.step += 1;
            self.frames = 0;
            self.requested = false;
            if self.step == self.steps.len() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.step < self.steps.len() {
            (self.steps[self.step].1)(&mut self.app);
        }
        self.app.render(ctx);
        ctx.request_repaint();
        let idle = self.app.session.snapshot.is_some()
            && !self.app.session.worker.busy
            && !self.app.session.has_pending()
            && (self.app.page != HEALTH || self.app.session.health.is_some())
            && (self.app.page != VARIABLES || self.app.session.variables.is_some())
            && (self.app.page != PERFORMANCE || self.app.session.performance.is_some())
            && (self.app.page != REPLICATION || self.app.session.replication.is_some())
            && (self.app.page != CONSOLE
                || self
                    .app
                    .session
                    .console
                    .as_ref()
                    .is_some_and(|c| !c.running()))
            && (self.app.page != PROCESSES
                || !self.app.locks_view
                || self.app.session.locks.is_some())
            && (self.app.page != SERVICE
                || self.app.session.host.is_none()
                || (!self.app.session.host_busy() && self.app.session.host_logs.is_some())
                || self.app.session.host_error.is_some());
        self.frames += 1;
        if idle && self.frames > 12 && !self.requested && self.step < self.steps.len() {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}
/// Minimal PNG encoder (stored deflate blocks); no extra dependency for a debug tool.
fn png(image: &egui::ColorImage) -> Vec<u8> {
    let [w, h] = image.size;
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for p in &image.pixels[y * w..(y + 1) * w] {
            raw.extend_from_slice(&p.to_array());
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(last as u8);
        let len = chunk.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + *byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for byte in data {
            c ^= *byte as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    };
    let mut header = Vec::new();
    header.extend_from_slice(&(w as u32).to_be_bytes());
    header.extend_from_slice(&(h as u32).to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
