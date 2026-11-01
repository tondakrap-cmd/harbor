//! Comparison of two servers: variables, accounts with privileges, and schemas
//! (databases, tables, columns, indexes). Runs in the background with its own
//! connections; the pure diffing is separate and unit-tested.
use crate::*;
use sqlx::mysql::MySqlConnectOptions;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Area {
    Variables,
    Accounts,
    Schema,
}
impl Area {
    pub fn label(self) -> &'static str {
        match self {
            Self::Variables => "Proměnné",
            Self::Accounts => "Účty",
            Self::Schema => "Schéma",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    OnlyLeft,
    OnlyRight,
    Different,
}
impl DiffKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OnlyLeft => "jen vlevo",
            Self::OnlyRight => "jen vpravo",
            Self::Different => "liší se",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub area: Area,
    pub object: String,
    pub kind: DiffKind,
    pub left: String,
    pub right: String,
}
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    pub left: String,
    pub right: String,
    pub differences: Vec<Difference>,
    /// How many objects of each area were compared.
    pub compared: Vec<(Area, usize)>,
    /// Variables skipped because they differ on every server (paths, ids, positions).
    pub ignored: usize,
}
impl Comparison {
    /// CSV of the differences (oblast, objekt, rozdíl, vlevo, vpravo).
    pub fn csv(&self) -> String {
        let result = QueryResult {
            columns: vec![
                "oblast".into(),
                "objekt".into(),
                "rozdil".into(),
                format!("vlevo: {}", self.left),
                format!("vpravo: {}", self.right),
            ],
            rows: self
                .differences
                .iter()
                .map(|d| {
                    vec![
                        Some(d.area.label().to_string()),
                        Some(d.object.clone()),
                        Some(d.kind.label().to_string()),
                        Some(d.left.clone()),
                        Some(d.right.clone()),
                    ]
                })
                .collect(),
            ..Default::default()
        };
        result.csv()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    pub variables: bool,
    pub accounts: bool,
    pub schema: bool,
}
impl Default for Areas {
    fn default() -> Self {
        Self {
            variables: true,
            accounts: true,
            schema: true,
        }
    }
}
/// Variables that differ between any two servers by design.
pub const EXPECTED_DIFFERENT: &[&str] = &[
    "hostname",
    "server_id",
    "server_uuid",
    "pid_file",
    "socket",
    "port",
    "log_bin_basename",
    "log_bin_index",
    "relay_log",
    "relay_log_basename",
    "relay_log_index",
    "relay_log_info_file",
    "general_log_file",
    "slow_query_log_file",
    "log_error",
    "gtid_binlog_pos",
    "gtid_binlog_state",
    "gtid_current_pos",
    "gtid_slave_pos",
    "gtid_io_pos",
    "wsrep_node_address",
    "wsrep_node_incoming_address",
    "wsrep_node_name",
    "wsrep_sst_receive_address",
    "report_host",
    "report_port",
    "timestamp",
    "rand_seed1",
    "rand_seed2",
    "pseudo_thread_id",
    "last_insert_id",
    "insert_id",
    "identity",
    "error_count",
    "warning_count",
    "system_time_zone",
    "version_source_revision",
    "innodb_buffer_pool_load_at_startup",
    "large_page_size",
    "thread_pool_size",
];
fn add(
    out: &mut Vec<Difference>,
    area: Area,
    object: String,
    left: Option<&String>,
    right: Option<&String>,
) {
    let (kind, l, r) = match (left, right) {
        (Some(l), Some(r)) if l == r => return,
        (Some(l), Some(r)) => (DiffKind::Different, l.clone(), r.clone()),
        (Some(l), None) => (DiffKind::OnlyLeft, l.clone(), String::new()),
        (None, Some(r)) => (DiffKind::OnlyRight, String::new(), r.clone()),
        (None, None) => return,
    };
    out.push(Difference {
        area,
        object,
        kind,
        left: l,
        right: r,
    });
}
/// Compares two keyed maps (missing keys are only-left / only-right).
pub fn diff_maps(
    area: Area,
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
) -> Vec<Difference> {
    let mut out = Vec::new();
    let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
    for key in keys {
        add(&mut out, area, key.clone(), left.get(key), right.get(key));
    }
    out
}
/// Global variable values worth comparing (expected differences skipped).
pub fn variable_map(variables: &[Variable]) -> (BTreeMap<String, String>, usize) {
    let mut ignored = 0;
    let map = variables
        .iter()
        .filter(|v| v.scope != "SESSION ONLY")
        .filter(|v| {
            let skip = EXPECTED_DIFFERENT.contains(&v.name.as_str());
            ignored += usize::from(skip);
            !skip
        })
        .map(|v| (v.name.clone(), v.value.clone()))
        .collect();
    (map, ignored)
}
/// One entry per account facet: "účet • oprávnění db.*", "účet • autentizace"…
pub fn account_map(details: &[AccountDetail]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for detail in details {
        let Some(account) = &detail.account else {
            continue;
        };
        let name = account.label();
        map.insert(
            format!("{name} • existuje"),
            if account.is_role() { "role" } else { "účet" }.to_string(),
        );
        if let Some(p) = &detail.properties {
            map.insert(format!("{name} • autentizace"), p.plugin.clone());
            map.insert(
                format!("{name} • stav"),
                format!(
                    "{}{}",
                    if p.locked { "zamčen" } else { "odemčen" },
                    if p.password_expired {
                        ", heslo vypršelo"
                    } else {
                        ""
                    }
                ),
            );
            map.insert(format!("{name} • TLS"), p.tls.label().to_string());
            let l = p.limits;
            map.insert(
                format!("{name} • limity"),
                format!(
                    "{}/{}/{}/{}",
                    l.queries_per_hour,
                    l.updates_per_hour,
                    l.connections_per_hour,
                    l.user_connections
                ),
            );
        }
        if !detail.roles.is_empty() {
            map.insert(format!("{name} • role"), detail.roles.join(", "));
        }
        if let Some(role) = &detail.default_role {
            map.insert(format!("{name} • výchozí role"), role.clone());
        }
        for set in &detail.privileges {
            let mut privileges: Vec<String> = set.privileges.iter().cloned().collect();
            if set.grantable {
                privileges.push(GRANT_OPTION.into());
            }
            map.insert(
                format!("{name} • oprávnění {}", set.scope.label()),
                if privileges.is_empty() {
                    "USAGE".into()
                } else {
                    privileges.join(", ")
                },
            );
        }
        for column in &detail.columns {
            map.insert(format!("{name} • sloupec {column}"), "ano".into());
        }
    }
    map
}
/// Schema objects of one server: "db", "db.table", "db.table.column", "db.table [index]".
#[derive(Clone, Debug, Default)]
pub struct SchemaOverview {
    pub objects: BTreeMap<String, String>,
}
/// Current phase and, once finished, the result.
type CompareState = (String, Option<std::result::Result<Comparison, String>>);
#[derive(Clone)]
pub struct CompareJob {
    pub started: Instant,
    pub right_name: String,
    state: Arc<Mutex<CompareState>>,
}
impl CompareJob {
    pub fn phase(&self) -> String {
        self.state.lock().map(|s| s.0.clone()).unwrap_or_default()
    }
    pub fn result(&self) -> Option<std::result::Result<Comparison, String>> {
        self.state.lock().ok().and_then(|s| s.1.clone())
    }
    pub fn running(&self) -> bool {
        self.result().is_none()
    }
}
/// Starts a comparison. `left` is the connected server (its connection settings, or
/// None in demo mode); the right one is a saved profile with its password.
pub(crate) fn spawn(
    left: Option<MySqlConnectOptions>,
    left_name: String,
    right: Profile,
    password: Secret,
    areas: Areas,
) -> CompareJob {
    let job = CompareJob {
        started: Instant::now(),
        right_name: right.name.clone(),
        state: Arc::new(Mutex::new(("Připojuji…".into(), None))),
    };
    let state = Arc::clone(&job.state);
    std::thread::spawn(move || {
        let phase = |text: &str| {
            if let Ok(mut s) = state.lock() {
                s.0 = text.to_string();
            }
        };
        let result = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(async {
                let (mut a, mut b) = match left {
                    Some(options) => {
                        let password = secrets::password_for(&right, password)?;
                        (
                            Backend::open(options).await?,
                            Backend::connect(&right, password).await?,
                        )
                    }
                    None => (Backend::demo(), Backend::demo_other()),
                };
                compare(&mut a, &mut b, left_name, right.name.clone(), areas, &phase).await
            }),
            Err(_) => Err(Error::Worker),
        };
        if let Ok(mut s) = state.lock() {
            s.0 = "Hotovo".into();
            s.1 = Some(result.map_err(|e| e.to_string()));
        }
    });
    job
}
async fn compare(
    a: &mut Backend,
    b: &mut Backend,
    left: String,
    right: String,
    areas: Areas,
    phase: &dyn Fn(&str),
) -> Result<Comparison> {
    let (snap_a, snap_b) = (a.snapshot().await?, b.snapshot().await?);
    let mut result = Comparison {
        left: format!("{left} ({})", snap_a.version),
        right: format!("{right} ({})", snap_b.version),
        ..Default::default()
    };
    if areas.variables {
        phase("Porovnávám proměnné…");
        let (va, ia) = variable_map(&a.variables().await?);
        let (vb, ib) = variable_map(&b.variables().await?);
        result.ignored = ia.max(ib);
        result
            .compared
            .push((Area::Variables, va.len().max(vb.len())));
        result
            .differences
            .extend(diff_maps(Area::Variables, &va, &vb));
    }
    if areas.accounts {
        phase("Porovnávám účty a oprávnění…");
        let mut details = (Vec::new(), Vec::new());
        for account in &snap_a.accounts {
            details.0.push(a.account_detail(account).await?);
        }
        for account in &snap_b.accounts {
            details.1.push(b.account_detail(account).await?);
        }
        let (ma, mb) = (account_map(&details.0), account_map(&details.1));
        result.compared.push((
            Area::Accounts,
            snap_a.accounts.len().max(snap_b.accounts.len()),
        ));
        result
            .differences
            .extend(diff_maps(Area::Accounts, &ma, &mb));
    }
    if areas.schema {
        phase("Porovnávám schémata…");
        let (sa, sb) = (a.schema_overview().await?, b.schema_overview().await?);
        result
            .compared
            .push((Area::Schema, sa.objects.len().max(sb.objects.len())));
        result
            .differences
            .extend(diff_maps(Area::Schema, &sa.objects, &sb.objects));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    #[test]
    fn maps_diff_into_only_left_right_and_different() {
        let diff = diff_maps(
            Area::Variables,
            &map(&[("a", "1"), ("b", "2"), ("c", "3")]),
            &map(&[("b", "2"), ("c", "4"), ("d", "5")]),
        );
        assert_eq!(
            diff.iter()
                .map(|d| (d.object.as_str(), d.kind))
                .collect::<Vec<_>>(),
            [
                ("a", DiffKind::OnlyLeft),
                ("c", DiffKind::Different),
                ("d", DiffKind::OnlyRight)
            ]
        );
        assert_eq!((diff[1].left.as_str(), diff[1].right.as_str()), ("3", "4"));
    }
    #[test]
    fn expected_differences_and_session_variables_are_skipped() {
        let variable = |name: &str, scope: &str| Variable {
            name: name.into(),
            value: "x".into(),
            scope: scope.into(),
            ..Default::default()
        };
        let (map, ignored) = variable_map(&[
            variable("server_id", "GLOBAL"),
            variable("max_connections", "GLOBAL"),
            variable("pseudo_slave_mode", "SESSION ONLY"),
        ]);
        assert_eq!(map.keys().collect::<Vec<_>>(), ["max_connections"]);
        assert_eq!(ignored, 1);
    }
    #[test]
    fn accounts_compare_facet_by_facet() {
        let detail = |privileges: &[&str], locked: bool| AccountDetail {
            account: Some(Account {
                user: "app".into(),
                host: "%".into(),
                plugin: String::new(),
            }),
            privileges: vec![PrivilegeSet {
                scope: GrantScope::Database("shop".into()),
                privileges: privileges.iter().map(|p| p.to_string()).collect(),
                grantable: false,
            }],
            properties: Some(AccountProperties {
                plugin: "mysql_native_password".into(),
                locked,
                ..Default::default()
            }),
            ..Default::default()
        };
        let diff = diff_maps(
            Area::Accounts,
            &account_map(&[detail(&["SELECT"], false)]),
            &account_map(&[detail(&["SELECT", "INSERT"], true)]),
        );
        let objects: Vec<&str> = diff.iter().map(|d| d.object.as_str()).collect();
        assert_eq!(objects, ["app@% • oprávnění shop.*", "app@% • stav"]);
        assert_eq!(diff[0].right, "INSERT, SELECT");
    }
    #[test]
    fn csv_lists_every_difference() {
        let comparison = Comparison {
            left: "A".into(),
            right: "B".into(),
            differences: vec![Difference {
                area: Area::Schema,
                object: "shop.orders.total".into(),
                kind: DiffKind::Different,
                left: "decimal(10,2)".into(),
                right: "decimal(12,2)".into(),
            }],
            ..Default::default()
        };
        assert!(comparison
            .csv()
            .contains("Schéma,shop.orders.total,liší se,\"decimal(10,2)\",\"decimal(12,2)\""));
    }
}
