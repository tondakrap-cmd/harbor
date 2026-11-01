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
    /// Detail of a database (tables, sizes, object counts).
    Tables(String),
    /// Columns, indexes, foreign keys and DDL of one table.
    Table {
        database: String,
        table: String,
    },
    /// Grants, roles and properties of an account or role.
    Grants(Account),
    Variables,
    Health,
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
    Table,
    Grants,
    Variables,
    Health,
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
            Self::Table { .. } => OperationKind::Table,
            Self::Grants(_) => OperationKind::Grants,
            Self::Variables => OperationKind::Variables,
            Self::Health => OperationKind::Health,
            Self::Apply(_) => OperationKind::Apply,
        }
    }
}
pub enum Response {
    ConnectionTested(Result<()>),
    Snapshot(Snapshot),
    Tables(DatabaseDetail),
    Table(TableDetail),
    Grants(AccountDetail),
    Variables(Vec<Variable>),
    Health(Vec<Finding>),
    /// Server messages of the change (maintenance output); usually empty.
    Changed(Vec<String>),
    Disconnected,
}
/// Reads must finish quickly; table maintenance (OPTIMIZE of a large table) may not.
fn timeout(kind: OperationKind, maintenance: bool) -> Duration {
    match kind {
        OperationKind::Apply if maintenance => Duration::from_secs(30 * 60),
        OperationKind::Health => Duration::from_secs(60),
        _ => Duration::from_secs(15),
    }
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
                    let limit = timeout(kind, matches!(request.operation, Operation::Apply(Change::Maintenance { .. })));
                    let result = tokio::select! {
                        biased;
                        _ = cancellation.changed() => { backend = None; continue; },
                        result = tokio::time::timeout(limit, execute(&mut backend, request.operation)) => result.unwrap_or(Err(Error::Timeout)),
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
                Operation::Tables(db) => Ok(Response::Tables(backend.database_detail(&db).await?)),
                Operation::Table { database, table } => Ok(Response::Table(
                    backend.table_detail(&database, &table).await?,
                )),
                Operation::Grants(account) => {
                    Ok(Response::Grants(backend.account_detail(&account).await?))
                }
                Operation::Variables => Ok(Response::Variables(backend.variables().await?)),
                Operation::Health => Ok(Response::Health(backend.health().await?)),
                Operation::Apply(change) => Ok(Response::Changed(backend.apply(change).await?)),
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
    /// Detail of `tables_for` (tables, sizes, object counts).
    pub database: Option<DatabaseDetail>,
    /// Database whose detail is in `database` (or is being loaded).
    pub tables_for: Option<String>,
    /// Detail of `table_for` (columns, indexes, foreign keys, DDL).
    pub table: Option<TableDetail>,
    pub table_for: Option<(String, String)>,
    /// Detail of `grants_for` (grants, roles, properties).
    pub account: Option<AccountDetail>,
    /// Account whose detail is in `account` (or is being loaded).
    pub grants_for: Option<Account>,
    pub variables: Option<Vec<Variable>>,
    /// Last health check result and when it ran.
    pub health: Option<(Instant, Vec<Finding>)>,
    /// Output of the last change (e.g. CHECK TABLE messages).
    pub change_output: Vec<String>,
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
    /// User-requested reads that arrived while the worker was busy (at most one per
    /// kind), tagged with the worker generation so none runs against another server.
    pending: VecDeque<(u64, Operation)>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            worker: Worker::new(),
            snapshot: None,
            database: None,
            tables_for: None,
            table: None,
            table_for: None,
            account: None,
            grants_for: None,
            variables: None,
            health: None,
            change_output: Vec::new(),
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
            pending: VecDeque::new(),
        }
    }
}
impl Session {
    fn clear_details(&mut self) {
        self.database = None;
        self.tables_for = None;
        self.table = None;
        self.table_for = None;
        self.account = None;
        self.grants_for = None;
        self.variables = None;
        self.health = None;
        self.change_output.clear();
    }
    /// Tables of the open database (empty when none is loaded).
    pub fn tables(&self) -> &[TableInfo] {
        self.database.as_ref().map_or(&[], |d| &d.tables)
    }
    /// SHOW GRANTS lines of the open account (empty when none is loaded).
    pub fn grants(&self) -> &[String] {
        self.account.as_ref().map_or(&[], |a| &a.grants)
    }
    /// When the UI should run `tick` again: soon while work is in flight, otherwise
    /// when the next periodic refresh is due (at most 1 s, so timers stay current).
    pub fn next_wakeup(&self, refresh_seconds: u64) -> Duration {
        if self.worker.busy || !self.pending.is_empty() {
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
        !self.pending.is_empty()
    }
    /// Queue a read for when the worker is free; a newer read of the same kind
    /// replaces an older one (the user changed their mind).
    fn enqueue(&mut self, operation: Operation) {
        let generation = self.worker.generation();
        self.pending
            .retain(|(g, queued)| *g == generation && queued.kind() != operation.kind());
        self.pending.push_back((generation, operation));
    }
    /// Entry point for user actions. Reads that arrive while the worker is busy are
    /// queued and sent as soon as it is free, instead of being dropped. Writes are
    /// never queued: returns false so the UI keeps its confirmation open.
    pub fn request(&mut self, operation: Operation) -> bool {
        if !self.error_is_connectivity {
            self.error = None;
        }
        let kind = operation.kind();
        match &operation {
            Operation::Tables(database) => {
                if self.tables_for.as_ref() != Some(database) {
                    self.table = None;
                    self.table_for = None;
                }
                self.tables_for = Some(database.clone());
            }
            Operation::Table { database, table } => {
                self.table_for = Some((database.clone(), table.clone()))
            }
            Operation::Grants(account) => self.grants_for = Some(account.clone()),
            _ => (),
        }
        let queueable = !matches!(
            kind,
            OperationKind::Apply | OperationKind::Connect | OperationKind::Disconnect
        );
        match self.worker.try_send(operation) {
            Ok(()) => true,
            Err(_) if !queueable => false,
            Err(operation) => {
                // A queued detail/test is followed by the periodic refresh anyway.
                if kind != OperationKind::Refresh || self.pending.is_empty() {
                    self.enqueue(*operation);
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
            self.pending.clear();
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
        self.pending.clear();
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
            OperationKind::Tables => self.database = None,
            OperationKind::Table => self.table = None,
            OperationKind::Grants => self.account = None,
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
                // A late answer for a detail the user already left is not shown.
                Ok(Response::Tables(detail)) => {
                    if self.tables_for.as_ref() == Some(&detail.name) {
                        self.database = Some(detail);
                    }
                }
                Ok(Response::Table(detail)) => {
                    if self.table_for.as_ref()
                        == Some(&(detail.database.clone(), detail.table.clone()))
                    {
                        self.table = Some(detail);
                    }
                }
                Ok(Response::Grants(detail)) => {
                    if detail.account.is_none() || detail.account == self.grants_for {
                        self.account = Some(detail);
                    }
                }
                Ok(Response::Variables(variables)) => self.variables = Some(variables),
                Ok(Response::Health(findings)) => self.health = Some((Instant::now(), findings)),
                Ok(Response::Changed(output)) => {
                    self.status = "Změna provedena".into();
                    self.error = None;
                    self.change_output = output;
                    self.worker.send(Operation::Refresh);
                    // Open details would otherwise keep showing the state before the change.
                    let reloads: Vec<Operation> = [
                        self.grants_for.clone().map(Operation::Grants),
                        self.tables_for.clone().map(Operation::Tables),
                        self.table_for
                            .clone()
                            .map(|(database, table)| Operation::Table { database, table }),
                        self.variables.is_some().then_some(Operation::Variables),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    for reload in reloads {
                        self.enqueue(reload);
                    }
                }
                Ok(Response::Disconnected) => self.status = "Odpojeno".into(),
                Err(error) => self.failed(kind, error),
            }
        }
        if !self.worker.busy {
            while let Some((generation, operation)) = self.pending.pop_front() {
                if generation != self.worker.generation() {
                    continue;
                }
                if let Err(operation) = self.worker.try_send(operation) {
                    self.pending.push_front((generation, *operation));
                }
                break;
            }
        }
        if self.snapshot.is_some()
            && self.last_refresh.elapsed() >= Duration::from_secs(refresh_seconds.clamp(1, 300))
        {
            self.worker.send(Operation::Refresh);
        }
    }
}
