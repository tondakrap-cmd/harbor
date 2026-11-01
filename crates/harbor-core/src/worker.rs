use crate::*;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc},
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
/// Which request a response answers; decides whether a failure means lost connectivity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    TestConnection,
    Connect,
    Disconnect,
    Refresh,
    Tables,
    Grants,
    Apply,
}
impl Operation {
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::TestConnection { .. } => OperationKind::TestConnection,
            Self::Connect { .. } => OperationKind::Connect,
            Self::Disconnect => OperationKind::Disconnect,
            Self::Refresh => OperationKind::Refresh,
            Self::Tables(_) => OperationKind::Tables,
            Self::Grants(_) => OperationKind::Grants,
            Self::Apply(_) => OperationKind::Apply,
        }
    }
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
    tx: async_mpsc::UnboundedSender<Request>,
    rx: mpsc::Receiver<(u64, OperationKind, Result<Response>)>,
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
        // Unbounded: at most one read/write is in flight (busy flag), so the queue only
        // grows by user-initiated server switches. A bounded queue could refuse a switch
        // after its cancellation was already published.
        let (tx, mut requests) = async_mpsc::unbounded_channel::<Request>();
        let (results, rx) = mpsc::channel();
        let (cancel, mut cancellation) = watch::channel(0u64);
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => {
                    let _ = results.send((0, OperationKind::Connect, Err(Error::Network)));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut backend: Option<Backend> = None;
                while let Some(request) = requests.recv().await {
                    if request.generation != *cancellation.borrow_and_update() { continue; }
                    let kind = request.operation.kind();
                    let result = tokio::select! {
                        biased;
                        _ = cancellation.changed() => { backend = None; continue; },
                        result = tokio::time::timeout(Duration::from_secs(15), execute(&mut backend, request.operation)) => result.unwrap_or(Err(Error::Timeout)),
                    };
                    if results.send((request.generation, kind, result)).is_err() { break; }
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
        self.try_send(operation).is_ok()
    }
    /// Like `send`, but a refused operation is handed back so it can be queued.
    pub fn try_send(&mut self, operation: Operation) -> std::result::Result<(), Box<Operation>> {
        let switch = matches!(
            &operation,
            Operation::Connect { .. } | Operation::Disconnect
        );
        if switch && self.write_in_flight {
            return Err(Box::new(operation));
        }
        if self.busy && !switch {
            return Err(Box::new(operation));
        }
        if switch {
            self.generation += 1;
            self.cancel.send_replace(self.generation);
        }
        let writing = matches!(&operation, Operation::Apply(_));
        match self.tx.send(Request {
            generation: self.generation,
            operation,
        }) {
            Ok(()) => {
                self.busy = true;
                self.write_in_flight = writing;
                Ok(())
            }
            // Only when the worker thread is gone; nothing is in flight any more.
            Err(error) => {
                self.busy = false;
                self.write_in_flight = false;
                Err(Box::new(error.0.operation))
            }
        }
    }
    /// Changes whenever the target server changes (connect, demo, disconnect).
    /// UIs compare it to drop selections and drafts bound to the previous server.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn poll(&mut self) -> Option<(OperationKind, Result<Response>)> {
        loop {
            match self.rx.try_recv() {
                Ok((generation, kind, response)) => {
                    if generation == self.generation {
                        self.busy = false;
                        self.write_in_flight = false;
                        return Some((kind, response));
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                // The worker thread ended (e.g. a panic): report it once instead of
                // leaving the UI "busy" forever.
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.busy {
                        return None;
                    }
                    self.busy = false;
                    self.write_in_flight = false;
                    return Some((OperationKind::Connect, Err(Error::Worker)));
                }
            }
        }
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
    /// Shared, so UIs can hold it across a frame without copying all rows.
    pub snapshot: Option<Arc<Snapshot>>,
    pub tables: Vec<TableInfo>,
    /// Database whose tables are in `tables` (or are being loaded).
    pub tables_for: Option<String>,
    pub grants: Vec<String>,
    /// Account whose grants are in `grants` (or are being loaded).
    pub grants_for: Option<Account>,
    /// Read-only for UIs; set through the session so `error_at` stays in sync.
    pub error: Option<String>,
    pub error_at: Instant,
    pub test_result: Option<String>,
    pub test_result_at: Instant,
    pub status: String,
    pub stale: bool,
    pub history: VecDeque<f64>,
    pub qps: Option<f64>,
    pub profile: Option<Profile>,
    pub last_refresh: Instant,
    rate: Rate,
    /// Connectivity errors clear on the next good snapshot; operation errors (a refused
    /// write, missing privilege) stay until the user's next action.
    error_is_connectivity: bool,
    /// One user-requested read that arrived while the worker was busy, tagged with the
    /// worker generation so it never runs against a different server.
    pending: Option<(u64, Operation)>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            worker: Worker::new(),
            snapshot: None,
            tables: Vec::new(),
            tables_for: None,
            grants: Vec::new(),
            grants_for: None,
            error: None,
            error_at: Instant::now(),
            test_result: None,
            test_result_at: Instant::now(),
            status: "Odpojeno".into(),
            stale: false,
            history: VecDeque::new(),
            qps: None,
            profile: None,
            last_refresh: Instant::now(),
            rate: Rate::default(),
            error_is_connectivity: false,
            pending: None,
        }
    }
}
impl Session {
    fn clear_details(&mut self) {
        self.tables.clear();
        self.tables_for = None;
        self.grants.clear();
        self.grants_for = None;
    }
    /// When the UI should run `tick` again: soon while work is in flight, otherwise
    /// when the next periodic refresh is due (at most 1 s, so timers stay current).
    pub fn next_wakeup(&self, refresh_seconds: u64) -> Duration {
        if self.worker.busy || self.pending.is_some() {
            return Duration::from_millis(100);
        }
        if self.snapshot.is_none() {
            return Duration::from_secs(1);
        }
        Duration::from_secs(refresh_seconds.clamp(1, 300))
            .saturating_sub(self.last_refresh.elapsed())
            .clamp(Duration::from_millis(100), Duration::from_secs(1))
    }
    fn set_error(&mut self, message: String, connectivity: bool) {
        self.error = Some(message);
        self.error_at = Instant::now();
        self.error_is_connectivity = connectivity;
    }
    fn set_test_result(&mut self, message: String) {
        self.test_result = Some(message);
        self.test_result_at = Instant::now();
    }
    /// True while a user read waits for the running operation to finish.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
    /// Entry point for user actions. Reads (refresh, tables, grants, connection test)
    /// that arrive while the worker is busy are queued and sent as soon as it is free,
    /// instead of being dropped. Writes are never queued: returns false so the UI keeps
    /// its confirmation open.
    pub fn request(&mut self, operation: Operation) -> bool {
        if !self.error_is_connectivity {
            self.error = None;
        }
        let kind = operation.kind();
        match &operation {
            Operation::Tables(database) => self.tables_for = Some(database.clone()),
            Operation::Grants(account) => self.grants_for = Some(account.clone()),
            _ => (),
        }
        let queueable = matches!(
            kind,
            OperationKind::Refresh
                | OperationKind::Tables
                | OperationKind::Grants
                | OperationKind::TestConnection
        );
        match self.worker.try_send(operation) {
            Ok(()) => true,
            Err(_) if !queueable => false,
            Err(operation) => {
                // A queued detail/test is followed by the periodic refresh anyway.
                let keep_existing = kind == OperationKind::Refresh
                    && self.pending.as_ref().is_some_and(|(generation, queued)| {
                        *generation == self.worker.generation()
                            && queued.kind() != OperationKind::Refresh
                    });
                if !keep_existing {
                    self.pending = Some((self.worker.generation(), *operation));
                }
                true
            }
        }
    }
    pub fn connect(&mut self, profile: Profile, password: Secret, demo: bool) {
        if self.worker.write_in_flight {
            self.set_error(
                "Probíhá zápis. Vyčkejte na výsledek před změnou serveru.".into(),
                false,
            );
            return;
        }
        if self.worker.send(Operation::Connect {
            profile: profile.clone(),
            password,
            demo,
        }) {
            self.snapshot = None;
            self.history.clear();
            self.clear_details();
            self.rate = Rate::default();
            self.qps = None;
            self.profile = Some(profile);
            self.error = None;
            self.test_result = None;
            self.pending = None;
            self.status = "Připojuji…".into();
        } else {
            self.set_error(Error::Worker.to_string(), false);
        }
    }
    pub fn disconnect(&mut self) {
        if self.worker.write_in_flight {
            self.set_error(
                "Probíhá zápis. Vyčkejte na výsledek před odpojením.".into(),
                false,
            );
            return;
        }
        if !self.worker.send(Operation::Disconnect) {
            self.set_error(Error::Worker.to_string(), false);
            return;
        }
        self.pending = None;
        self.clear_details();
        self.snapshot = None;
        self.profile = None;
        self.history.clear();
        self.qps = None;
        self.status = "Odpojeno".into();
    }
    fn failed(&mut self, kind: OperationKind, error: Error) {
        let connectivity = matches!(
            kind,
            OperationKind::Connect | OperationKind::Refresh | OperationKind::Disconnect
        ) || matches!(error, Error::Network);
        match kind {
            // A worker-level timeout of a test is still just a test result.
            OperationKind::TestConnection => {
                self.set_test_result(format!("Test spojení selhal: {error}"));
                return;
            }
            OperationKind::Tables => self.tables.clear(),
            OperationKind::Grants => self.grants.clear(),
            _ => (),
        }
        let message = match kind {
            OperationKind::Apply => format!("Změna nebyla potvrzena: {error}"),
            _ => error.to_string(),
        };
        self.set_error(message, connectivity);
        if connectivity {
            self.stale = true;
            self.status = "CHYBA • data nejsou aktuální".into();
            self.last_refresh = Instant::now();
            self.rate = Rate::default();
            self.qps = None;
        }
    }
    pub fn tick(&mut self, refresh_seconds: u64) {
        while let Some((kind, result)) = self.worker.poll() {
            match result {
                Ok(Response::ConnectionTested(result)) => self.set_test_result(match result {
                    Ok(()) => "Test spojení uspěl.".into(),
                    Err(error) => format!("Test spojení selhal: {error}"),
                }),
                Ok(Response::Snapshot(snapshot)) => {
                    let now = Instant::now();
                    self.qps =
                        self.rate
                            .sample(snapshot.questions, snapshot.uptime, snapshot.source, now);
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
                    self.snapshot = Some(Arc::new(snapshot));
                    self.last_refresh = now;
                    self.stale = false;
                    if self.error_is_connectivity {
                        self.error = None;
                        self.error_is_connectivity = false;
                    }
                }
                Ok(Response::Tables(tables)) => self.tables = tables,
                Ok(Response::Grants(grants)) => self.grants = grants,
                Ok(Response::Changed) => {
                    self.status = "Změna provedena".into();
                    self.error = None;
                    self.worker.send(Operation::Refresh);
                    // The open detail (grants of an account, tables of a database) would
                    // otherwise keep showing the state before the change.
                    let reload = self
                        .grants_for
                        .clone()
                        .map(Operation::Grants)
                        .or_else(|| self.tables_for.clone().map(Operation::Tables));
                    if let Some(reload) = reload {
                        self.pending = Some((self.worker.generation(), reload));
                    }
                }
                Ok(Response::Disconnected) => self.status = "Odpojeno".into(),
                Err(error) => self.failed(kind, error),
            }
        }
        if !self.worker.busy {
            if let Some((generation, operation)) = self.pending.take() {
                if generation == self.worker.generation() {
                    if let Err(operation) = self.worker.try_send(operation) {
                        self.pending = Some((generation, *operation));
                    }
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
