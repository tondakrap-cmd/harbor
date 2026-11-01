//! SQL text for every change. Pure functions: the same code builds the statement that
//! runs and the preview shown before confirmation (with password hashes hidden).
use crate::*;
use zeroize::Zeroizing;

/// The exact statement(s) a change will run, with password hashes replaced by a
/// placeholder. Shown in confirmation dialogs.
pub fn preview(change: &Change) -> Result<String> {
    Ok(statement(change, false)?.to_string())
}
/// `reveal = false` replaces password hashes with a placeholder (preview only).
pub(crate) fn statement(change: &Change, reveal: bool) -> Result<Zeroizing<String>> {
    let password = |password: &Secret| -> Result<String> {
        validate_password(password)?;
        Ok(if reveal {
            quoted(&native_password_hash(password))
        } else {
            "'<hash hesla>'".into()
        })
    };
    let sql = match change {
        Change::CreateDatabase { name, collation } => format!(
            "CREATE DATABASE {} COLLATE {}",
            identifier(name)?,
            identifier(collation)?
        ),
        Change::DropDatabase { name } => {
            if system_database(name) {
                return Err(Error::Validation(
                    "Systémovou databázi Harbor nesmaže.".into(),
                ));
            }
            format!("DROP DATABASE {}", identifier(name)?)
        }
        Change::CreateAccount {
            user,
            host,
            password: secret,
        } => {
            validate_account(user, host)?;
            format!(
                "CREATE USER {}@{} IDENTIFIED VIA mysql_native_password USING {}",
                quoted(user),
                quoted(host),
                password(secret)?
            )
        }
        Change::DropAccount { account } => {
            if account.user == "mariadb.sys" {
                return Err(Error::Validation(
                    "Systémový účet mariadb.sys Harbor nesmaže.".into(),
                ));
            }
            if account.is_role() {
                format!("DROP ROLE {}", account_sql(account)?)
            } else {
                format!("DROP USER {}", account_sql(account)?)
            }
        }
        Change::ChangePassword {
            account,
            password: secret,
        } => {
            if account.is_role() {
                return Err(Error::Validation("Role nemá heslo.".into()));
            }
            // A literal (not PASSWORD()) is stored as the hash; the plugin is verified
            // as mysql_native_password before this runs.
            format!(
                "SET PASSWORD FOR {} = {}",
                account_sql(account)?,
                password(secret)?
            )
        }
        Change::Privileges {
            account,
            scope,
            grant,
            revoke,
        } => privileges_sql(account, scope, grant, revoke)?,
        Change::CreateRole { name } => {
            validate_role(name)?;
            format!("CREATE ROLE {}", role_identifier(name))
        }
        Change::RoleMembership {
            role,
            account,
            grant,
        } => {
            validate_role(role)?;
            if *grant {
                format!(
                    "GRANT {} TO {}",
                    role_identifier(role),
                    account_sql(account)?
                )
            } else {
                format!(
                    "REVOKE {} FROM {}",
                    role_identifier(role),
                    account_sql(account)?
                )
            }
        }
        Change::DefaultRole { account, role } => {
            if account.is_role() {
                return Err(Error::Validation(
                    "Výchozí roli lze nastavit jen účtu.".into(),
                ));
            }
            let role = match role {
                Some(role) => {
                    validate_role(role)?;
                    role_identifier(role)
                }
                None => "NONE".into(),
            };
            format!("SET DEFAULT ROLE {role} FOR {}", account_sql(account)?)
        }
        Change::AlterAccount {
            account,
            locked,
            expire_password,
            tls,
            limits,
        } => {
            if account.is_role() {
                return Err(Error::Validation(
                    "Vlastnosti přihlášení má jen účet, ne role.".into(),
                ));
            }
            let hourly = 4_294_967_295;
            if limits.queries_per_hour > hourly
                || limits.updates_per_hour > hourly
                || limits.connections_per_hour > hourly
                || limits.user_connections > i32::MAX as u64
            {
                return Err(Error::Validation("Limit je mimo povolený rozsah.".into()));
            }
            let require = match tls {
                None | Some(TlsRequirement::Specified) => String::new(),
                Some(TlsRequirement::None) => " REQUIRE NONE".into(),
                Some(TlsRequirement::Ssl) => " REQUIRE SSL".into(),
                Some(TlsRequirement::X509) => " REQUIRE X509".into(),
            };
            format!(
                "ALTER USER {}{require} WITH MAX_QUERIES_PER_HOUR {} MAX_UPDATES_PER_HOUR {} MAX_CONNECTIONS_PER_HOUR {} MAX_USER_CONNECTIONS {} ACCOUNT {}{}",
                account_sql(account)?,
                limits.queries_per_hour,
                limits.updates_per_hour,
                limits.connections_per_hour,
                limits.user_connections,
                if *locked { "LOCK" } else { "UNLOCK" },
                if *expire_password { " PASSWORD EXPIRE" } else { "" }
            )
        }
        Change::SetGlobal { variable, value } => {
            if variable.name.is_empty()
                || !variable
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                return Err(Error::Validation("Neplatný název proměnné.".into()));
            }
            if !variable.settable() {
                return Err(Error::Validation(format!(
                    "{} nelze měnit za běhu; patří do konfiguračního souboru.",
                    variable.name
                )));
            }
            format!(
                "SET GLOBAL {} = {}",
                variable.name,
                variable.literal(value)?
            )
        }
        Change::Maintenance {
            database,
            table,
            action,
        } => format!(
            "{} {}.{}",
            action.sql(),
            identifier(database)?,
            identifier(table)?
        ),
        Change::KillQuery { target } => {
            if target.query_id == 0 {
                return Err(Error::Validation("Dotaz už neběží.".into()));
            }
            // QUERY_ID, not the thread id: if the statement ended in the meantime,
            // the next statement on that connection is not hit.
            format!("KILL QUERY ID {}", target.query_id)
        }
    };
    Ok(Zeroizing::new(sql))
}
fn privileges_sql(
    account: &Account,
    scope: &GrantScope,
    grant: &[String],
    revoke: &[String],
) -> Result<String> {
    let allowed = scope.privileges();
    for privilege in grant.iter().chain(revoke) {
        if privilege != GRANT_OPTION && !allowed.contains(&privilege.as_str()) {
            return Err(Error::Validation(format!(
                "Oprávnění {} nelze udělit na úrovni {}.",
                safe_text(privilege),
                scope.label()
            )));
        }
    }
    if grant.iter().any(|g| revoke.contains(g)) {
        return Err(Error::Validation(
            "Stejné oprávnění nelze zároveň přidat i odebrat.".into(),
        ));
    }
    if grant.is_empty() && revoke.is_empty() {
        return Err(Error::Validation("Žádná změna oprávnění.".into()));
    }
    let target = match scope {
        GrantScope::Global => "*.*".to_string(),
        // GRANT treats % and _ in a database name as patterns: escape them.
        GrantScope::Database(db) => format!("{}.*", grant_identifier(db)?),
        GrantScope::DatabasePattern(pattern) => format!("{}.*", identifier(pattern)?),
        GrantScope::Table(db, table) => format!("{}.{}", identifier(db)?, identifier(table)?),
    };
    let who = account_sql(account)?;
    let mut statements = Vec::new();
    if !grant.is_empty() {
        let option = grant.iter().any(|g| g == GRANT_OPTION);
        let list: Vec<&str> = grant
            .iter()
            .filter(|g| *g != GRANT_OPTION)
            .map(String::as_str)
            .collect();
        statements.push(format!(
            "GRANT {} ON {target} TO {who}{}",
            if list.is_empty() {
                "USAGE".into()
            } else {
                list.join(", ")
            },
            if option { " WITH GRANT OPTION" } else { "" }
        ));
    }
    if !revoke.is_empty() {
        statements.push(format!(
            "REVOKE {} ON {target} FROM {who}",
            revoke.join(", ")
        ));
    }
    Ok(statements.join(";\n"))
}
pub(crate) fn system_database(name: &str) -> bool {
    ["mysql", "information_schema", "performance_schema", "sys"].contains(&name)
}
pub(crate) fn validate_password(password: &Secret) -> Result<()> {
    if password.0.is_empty() {
        Err(Error::Validation("Nové heslo nesmí být prázdné.".into()))
    } else {
        Ok(())
    }
}
pub(crate) fn validate_account(user: &str, host: &str) -> Result<()> {
    if user.is_empty()
        || user.len() > 80
        || host.is_empty()
        || host.len() > 255
        || user.chars().chain(host.chars()).any(char::is_control)
    {
        Err(Error::Validation("Neplatný uživatel nebo hostitel.".into()))
    } else {
        Ok(())
    }
}
fn validate_role(name: &str) -> Result<()> {
    if name.is_empty()
        || name.chars().count() > 128
        || name.chars().any(char::is_control)
        || ["NONE", "PUBLIC"].contains(&name.to_ascii_uppercase().as_str())
    {
        Err(Error::Validation(
            "Název role musí mít 1–128 znaků a nesmí být NONE ani PUBLIC.".into(),
        ))
    } else {
        Ok(())
    }
}
fn role_identifier(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}
/// mysql_native_password stored form: '*' + upper hex of SHA1(SHA1(password)).
/// Only this hash is sent, so the plaintext never appears in SQL text, PROCESSLIST,
/// server logs or on an explicitly unencrypted test connection.
pub(crate) fn native_password_hash(password: &Secret) -> Zeroizing<String> {
    use sha1::{Digest, Sha1};
    let stage1 = Zeroizing::new(<[u8; 20]>::from(Sha1::digest(password.0.as_bytes())));
    let stage2 = Sha1::digest(stage1.as_slice());
    let mut hash = Zeroizing::new(String::with_capacity(41));
    hash.push('*');
    for byte in stage2 {
        hash.push_str(&format!("{byte:02X}"));
    }
    hash
}
pub(crate) fn grant_identifier(value: &str) -> Result<String> {
    identifier(value)?;
    Ok(format!(
        "`{}`",
        value
            .replace('`', "``")
            .replace('_', "\\_")
            .replace('%', "\\%")
    ))
}
/// 'user'@'host' for accounts, `name` for roles.
pub(crate) fn account_sql(account: &Account) -> Result<String> {
    if account.is_role() {
        validate_role(&account.user)?;
        return Ok(role_identifier(&account.user));
    }
    validate_account(&account.user, &account.host)?;
    Ok(format!(
        "{}@{}",
        quoted(&account.user),
        quoted(&account.host)
    ))
}
pub(crate) fn redact_grant(grant: String) -> String {
    let upper = grant.to_ascii_uppercase();
    match upper.find(" IDENTIFIED ") {
        Some(at) => format!("{} [autentizační údaje skryty]", &grant[..at]),
        None => grant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn account(user: &str, host: &str) -> Account {
        Account {
            user: user.into(),
            host: host.into(),
            plugin: "mysql_native_password".into(),
        }
    }
    #[test]
    fn literals_and_grants_do_not_expose_or_escape_secrets() {
        assert_eq!(quoted("a'b\\c"), "'a''b\\c'");
        assert_eq!(grant_identifier("db_%`x").unwrap(), "`db\\_\\%``x`");
        assert!(!redact_grant(
            "GRANT USAGE ON *.* TO 'u'@'h' IDENTIFIED BY PASSWORD 'hash'".into()
        )
        .contains("hash"));
    }
    #[test]
    fn password_hash_matches_mariadb_password_function() {
        // SELECT PASSWORD('password') on MariaDB / MySQL 5.x.
        assert_eq!(
            native_password_hash(&Secret::new("password".into())).as_str(),
            "*2470C0C06DEE42FD1618BB99005ADCA2EC9D1E19"
        );
    }
    #[test]
    fn account_ddl_never_contains_the_plaintext_password() {
        let target = account("u'x", "localhost");
        let password = Secret::new("Tajné'heslo\\x".into());
        let hash = native_password_hash(&password);
        let changes = [
            Change::CreateAccount {
                user: target.user.clone(),
                host: target.host.clone(),
                password: password.clone(),
            },
            Change::ChangePassword {
                account: target.clone(),
                password: password.clone(),
            },
        ];
        for change in &changes {
            let sql = statement(change, true).unwrap();
            assert!(!sql.contains("Tajné"), "{}", sql.as_str());
            assert!(!sql.contains("PASSWORD("));
            assert!(sql.contains(hash.as_str()));
            assert!(sql.contains("'u''x'@'localhost'"));
            // The preview hides even the hash.
            let shown = preview(change).unwrap();
            assert!(!shown.contains(hash.as_str()) && shown.contains("<hash hesla>"));
        }
        assert!(statement(
            &Change::CreateAccount {
                user: "u".into(),
                host: "h".into(),
                password: Secret::default()
            },
            true
        )
        .is_err());
    }
    #[test]
    fn privilege_statements_are_whitelisted_and_escaped() {
        let target = account("app", "%");
        let sql = preview(&Change::Privileges {
            account: target.clone(),
            scope: GrantScope::Database("shop_db".into()),
            grant: vec!["SELECT".into(), "SHOW VIEW".into(), GRANT_OPTION.into()],
            revoke: vec!["DELETE".into()],
        })
        .unwrap();
        assert_eq!(
            sql,
            "GRANT SELECT, SHOW VIEW ON `shop\\_db`.* TO 'app'@'%' WITH GRANT OPTION;\nREVOKE DELETE ON `shop\\_db`.* FROM 'app'@'%'"
        );
        let only_option = preview(&Change::Privileges {
            account: target.clone(),
            scope: GrantScope::Table("shop_db".into(), "orders".into()),
            grant: vec![GRANT_OPTION.into()],
            revoke: vec![],
        })
        .unwrap();
        assert_eq!(
            only_option,
            "GRANT USAGE ON `shop_db`.`orders` TO 'app'@'%' WITH GRANT OPTION"
        );
        for bad in ["SUPER", "SELECT; DROP DATABASE x", "ALL PRIVILEGES"] {
            assert!(preview(&Change::Privileges {
                account: target.clone(),
                scope: GrantScope::Database("shop".into()),
                grant: vec![bad.into()],
                revoke: vec![],
            })
            .is_err());
        }
        // Roles are addressed by identifier.
        assert_eq!(
            preview(&Change::Privileges {
                account: account("čtenáři", ""),
                scope: GrantScope::Global,
                grant: vec!["PROCESS".into()],
                revoke: vec![],
            })
            .unwrap(),
            "GRANT PROCESS ON *.* TO `čtenáři`"
        );
    }
    #[test]
    fn account_roles_and_properties() {
        let target = account("app", "10.0.0.%");
        assert_eq!(
            preview(&Change::RoleMembership {
                role: "r`x".into(),
                account: target.clone(),
                grant: true
            })
            .unwrap(),
            "GRANT `r``x` TO 'app'@'10.0.0.%'"
        );
        assert_eq!(
            preview(&Change::DefaultRole {
                account: target.clone(),
                role: None
            })
            .unwrap(),
            "SET DEFAULT ROLE NONE FOR 'app'@'10.0.0.%'"
        );
        assert!(preview(&Change::CreateRole {
            name: "PUBLIC".into()
        })
        .is_err());
        assert_eq!(
            preview(&Change::AlterAccount {
                account: target.clone(),
                locked: true,
                expire_password: true,
                tls: Some(TlsRequirement::Ssl),
                limits: Limits {
                    user_connections: 5,
                    ..Default::default()
                },
            })
            .unwrap(),
            "ALTER USER 'app'@'10.0.0.%' REQUIRE SSL WITH MAX_QUERIES_PER_HOUR 0 MAX_UPDATES_PER_HOUR 0 MAX_CONNECTIONS_PER_HOUR 0 MAX_USER_CONNECTIONS 5 ACCOUNT LOCK PASSWORD EXPIRE"
        );
        assert_eq!(
            preview(&Change::DropAccount {
                account: account("r", "")
            })
            .unwrap(),
            "DROP ROLE `r`"
        );
        assert!(preview(&Change::DropAccount {
            account: account("mariadb.sys", "localhost")
        })
        .is_err());
        assert!(preview(&Change::DropDatabase {
            name: "mysql".into()
        })
        .is_err());
    }
    #[test]
    fn set_global_uses_validated_literals() {
        let variable = Variable {
            name: "max_connections".into(),
            value: "151".into(),
            scope: "GLOBAL".into(),
            var_type: "BIGINT UNSIGNED".into(),
            min: Some("10".into()),
            max: Some("100000".into()),
            ..Default::default()
        };
        assert_eq!(
            preview(&Change::SetGlobal {
                variable: variable.clone(),
                value: "300".into()
            })
            .unwrap(),
            "SET GLOBAL max_connections = 300"
        );
        let mut bad_name = variable.clone();
        bad_name.name = "x; DROP".into();
        assert!(preview(&Change::SetGlobal {
            variable: bad_name,
            value: "1".into()
        })
        .is_err());
        let mut read_only = variable;
        read_only.read_only = true;
        assert!(preview(&Change::SetGlobal {
            variable: read_only,
            value: "300".into()
        })
        .is_err());
    }
}
