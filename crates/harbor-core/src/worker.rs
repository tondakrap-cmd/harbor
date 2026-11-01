use crate::*;
use std::{
    collections::VecDeque,
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc as async_mpsc, watch};

pub enum Operation {
    TestConnection {
        profile: Profile,
        password: Secret,
    },
    Connect {
        profile: Profile,
        password: Secret,
        demo: bool,
    },
    Disconnect,
    Refresh,
    Tables(String),
    Grants(Account),
    Apply(Change),
}
pub enum Response {
    ConnectionTested(Result<()>),
    Snapshot(Snapshot),
    Tables(Vec<TableInfo>),
    Grants(Vec<String>),
    Changed,
    Disconnected,
}
struct Request {
    generation: u64,
    operation: Operation,
}
pub struct Worker {
    tx: async_mpsc::Sender<Request>,
    rx: mpsc::Receiver<(u64, Result<Response>)>,
    cancel: watch::Sender<u64>,
    generation: u64,
    pub busy: bool,
    write_in_flight: bool,
}
impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}
impl Worker {
    pub fn new() -> Self {
        let (tx, mut requests) = async_mpsc::channel::<Request>(2);
        let (results, rx) = mpsc::channel();
        let (cancel, mut cancellation) = watch::channel(0u64);
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => {
                    let _ = results.send((0, Err(Error::Network)));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut backend: Option<Backend> = None;
                while let Some(request) = requests.recv().await {
                    if request.generation != *cancellation.borrow_and_update() { continue; }
                    let result = tokio::select! {
                        biased;
                        _ = cancellation.changed() => { backend = None; continue; },
                        result = tokio::time::timeout(Duration::from_secs(15), execute(&mut backend, request.operation)) => result.unwrap_or(Err(Error::Timeout)),
                    };
                    if results.send((request.generation, result)).is_err() { break; }
                }
            });
        });
        Self {
            tx,
            rx,
            cancel,
            generation: 0,
            busy: false,
            write_in_flight: false,
        }
    }
    pub fn send(&mut self, operation: Operation) -> bool {
        let switch = matches!(
            &operation,
            Operation::Connect { .. } | Operation::Disconnect
        );
        if switch && self.write_in_flight {
            return false;
        }
        if self.busy && !switch {
            return false;
        }
        if switch {
            self.generation += 1;
            self.cancel.send_replace(self.generation);
        }
        let writing = matches!(&operation, Operation::Apply(_));
        if self
            .tx
            .try_send(Request {
                generation: self.generation,
                operation,
            })
            .is_ok()
        {
            self.busy = true;
            self.write_in_flight = writing;
            true
        } else {
            false
        }
    }
    pub fn poll(&mut self) -> Option<Result<Response>> {
        while let Ok((generation, response)) = self.rx.try_recv() {
            if generation == self.generation {
                self.busy = false;
                self.write_in_flight = false;
                return Some(response);
            }
        }
        None
    }
}
async fn execute(backend: &mut Option<Backend>, operation: Operation) -> Result<Response> {
    match operation {
        Operation::TestConnection { profile, password } => Ok(Response::ConnectionTested(
            Backend::connect(&profile, password).await.map(|_| ()),
        )),
        Operation::Connect {
            profile,
            password,
            demo,
        } => {
            *backend = None;
            *backend = Some(if demo {
                Backend::demo()
            } else {
                Backend::connect(&profile, password).await?
            });
            Ok(Response::Snapshot(
                backend.as_mut().expect("connected").snapshot().await?,
            ))
        }
        Operation::Disconnect => {
            *backend = None;
            Ok(Response::Disconnected)
        }
        operation => {
            let backend = backend.as_mut().ok_or_else(|| {
                Error::Validation("Nejprve připojte server nebo otevřete demo.".into())
            })?;
            match operation {
                Operation::Refresh => Ok(Response::Snapshot(backend.snapshot().await?)),
                Operation::Tables(db) => Ok(Response::Tables(backend.tables(&db).await?)),
                Operation::Grants(account) => Ok(Response::Grants(backend.grants(&account).await?)),
                Operation::Apply(change) => {
                    backend.apply(change).await?;
                    Ok(Response::Changed)
                }
                _ => unreachable!(),
            }
        }
    }
}
// Shared live data; navigation, forms and theme selection remain in each UI.
pub struct Session {
    pub worker: Worker,
    pub snapshot: Option<Snapshot>,
    pub tables: Vec<TableInfo>,
    pub grants: Vec<String>,
    pub error: Option<String>,
    pub test_result: Option<String>,
    pub status: String,
    pub stale: bool,
    pub history: VecDeque<f64>,
    pub qps: Option<f64>,
    pub profile: Option<Profile>,
    pub last_refresh: Instant,
    rate: Rate,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            worker: Worker::new(),
            snapshot: None,
            tables: Vec::new(),
            grants: Vec::new(),
            error: None,
            test_result: None,
            status: "Odpojeno".into(),
            stale: false,
            history: VecDeque::new(),
            qps: None,
            profile: None,
            last_refresh: Instant::now(),
            rate: Rate::default(),
        }
    }
}
impl Session {
    pub fn connect(&mut self, profile: Profile, password: Secret, demo: bool) {
        if self.worker.write_in_flight {
            self.error = Some("Probíhá zápis. Vyčkejte na výsledek před změnou serveru.".into());
            return;
        }
        if self.worker.send(Operation::Connect {
            profile: profile.clone(),
            password,
            demo,
        }) {
            self.snapshot = None;
            self.history.clear();
            self.tables.clear();
            self.grants.clear();
            self.rate = Rate::default();
            self.qps = None;
            self.profile = Some(profile);
            self.error = None;
            self.test_result = None;
            self.status = "Připojuji…".into();
        }
    }
    pub fn disconnect(&mut self) {
        if !self.worker.send(Operation::Disconnect) {
            self.error = Some("Probíhá zápis. Vyčkejte na výsledek před odpojením.".into());
            return;
        }
        self.snapshot = None;
        self.profile = None;
        self.history.clear();
        self.qps = None;
        self.status = "Odpojeno".into();
    }
    pub fn tick(&mut self, refresh_seconds: u64) {
        while let Some(result) = self.worker.poll() {
            match result {
                Ok(Response::ConnectionTested(result)) => {
                    self.test_result = Some(match result {
                        Ok(()) => "Test spojení uspěl.".into(),
                        Err(error) => format!("Test spojení selhal: {error}"),
                    })
                }
                Ok(Response::Snapshot(snapshot)) => {
                    let now = Instant::now();
                    self.qps = self.rate.sample(snapshot.questions, snapshot.uptime, now);
                    if let Some(qps) = self.qps {
                        if self.history.len() == 120 {
                            self.history.pop_front();
                        }
                        self.history.push_back(qps);
                    } else {
                        self.history.clear();
                    }
                    self.status = if snapshot.demo {
                        "DEMO • ukázková data"
                    } else {
                        "Připojeno"
                    }
                    .into();
                    self.snapshot = Some(snapshot);
                    self.last_refresh = now;
                    self.stale = false;
                    self.error = None;
                }
                Ok(Response::Tables(tables)) => self.tables = tables,
                Ok(Response::Grants(grants)) => self.grants = grants,
                Ok(Response::Changed) => {
                    self.status = "Změna provedena".into();
                    self.worker.send(Operation::Refresh);
                }
                Ok(Response::Disconnected) => self.status = "Odpojeno".into(),
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.stale = true;
                    self.status = "CHYBA • data nejsou aktuální".into();
                    self.last_refresh = Instant::now();
                    self.rate = Rate::default();
                    self.qps = None;
                }
            }
        }
        if self.snapshot.is_some()
            && self.last_refresh.elapsed() >= Duration::from_secs(refresh_seconds.clamp(1, 300))
        {
            self.worker.send(Operation::Refresh);
        }
    }
}
