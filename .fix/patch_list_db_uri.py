from pathlib import Path

path = Path(r"src-tauri/src/smartbrain/commands.rs")
text = path.read_text(encoding="utf-8")

old = """fn first_non_empty(values: &[&str]) -> Option<String> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn parse_cli_database_names(stdout: &str) -> Vec<String> {"""

new = r'''fn first_non_empty(values: &[&str]) -> Option<String> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn normalize_connection_key(raw_key: &str) -> String {
    raw_key
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-')
        .collect()
}

fn looks_like_key_value_connection_string(value: &str) -> bool {
    value.contains('=') && value.contains(';') && !value.contains("://")
}

fn parse_key_value_connection_string(connection_uri: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for segment in connection_uri.split(';') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(separator_index) = trimmed.find('=') else {
            continue;
        };
        if separator_index == 0 {
            continue;
        }
        let key = normalize_connection_key(&trimmed[..separator_index]);
        let value = trimmed[separator_index + 1..].trim().to_string();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        result.insert(key, value);
    }
    result
}

fn pick_connection_value<'a>(
    values: &'a HashMap<String, String>,
    aliases: &[&str],
) -> Option<&'a str> {
    for alias in aliases {
        let normalized = normalize_connection_key(alias);
        if let Some(value) = values.get(&normalized) {
            if !value.trim().is_empty() {
                return Some(value.as_str());
            }
        }
    }
    None
}

fn parse_server_host_and_port(db_type: &str, raw_value: &str) -> (String, Option<u16>) {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }

    if db_type == "sqlserver" || db_type == "mssql" {
        if let Some((host, port)) = trimmed.rsplit_once(',') {
            if let Ok(port_value) = port.trim().parse::<u16>() {
                return (host.trim().to_string(), Some(port_value));
            }
        }
    }

    if let Some((host, port)) = trimmed.rsplit_once(':') {
        if !host.contains(']') && port.chars().all(|ch| ch.is_ascii_digit()) {
            if let Ok(port_value) = port.parse::<u16>() {
                return (host.trim().to_string(), Some(port_value));
            }
        }
    }

    (trimmed.to_string(), None)
}

fn default_port_for_db_type(db_type: &str) -> Option<u16> {
    match db_type {
        "postgresql" | "postgres" => Some(5432),
        "mysql" => Some(3306),
        "sqlserver" | "mssql" => Some(1433),
        _ => None,
    }
}

fn normalize_db_protocol(db_type: &str, uri: &str) -> String {
    if uri.contains("://") {
        return uri.to_string();
    }
    match db_type {
        "postgresql" | "postgres" => format!("postgresql://{uri}"),
        "mysql" => format!("mysql://{uri}"),
        "sqlserver" | "mssql" => format!("sqlserver://{uri}"),
        _ => uri.to_string(),
    }
}

fn percent_decode_loose(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(decoded) = u8::from_str_radix(hex, 16) {
                output.push(decoded);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn parse_connection_uri_fields(
    db_type: &str,
    connection_uri: &str,
) -> Option<(String, Option<u16>, String, String, String, String)> {
    let trimmed = connection_uri.trim();
    if trimmed.is_empty() {
        return None;
    }

    let db_type = db_type.trim().to_ascii_lowercase();
    if db_type == "sqlite" {
        let path = if trimmed.contains("://") {
            let without_scheme = trimmed.splitn(2, "://").nth(1).unwrap_or(trimmed);
            let without_query = without_scheme.split('?').next().unwrap_or(without_scheme);
            percent_decode_loose(without_query)
        } else {
            trimmed.to_string()
        };
        return Some((
            String::new(),
            None,
            path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(path.as_str())
                .to_string(),
            String::new(),
            String::new(),
            path,
        ));
    }

    if looks_like_key_value_connection_string(trimmed) {
        let values = parse_key_value_connection_string(trimmed);
        let server = pick_connection_value(
            &values,
            &[
                "server",
                "host",
                "hostname",
                "data source",
                "datasource",
                "address",
                "addr",
                "network address",
            ],
        )
        .unwrap_or_default();
        let (host, parsed_port) = parse_server_host_and_port(&db_type, server);
        let port = pick_connection_value(&values, &["port"])
            .and_then(|value| value.parse::<u16>().ok())
            .or(parsed_port)
            .or_else(|| default_port_for_db_type(&db_type));
        let database_name =
            pick_connection_value(&values, &["database", "initial catalog"]).unwrap_or_default();
        let username =
            pick_connection_value(&values, &["uid", "user id", "user", "username"]).unwrap_or_default();
        let password =
            pick_connection_value(&values, &["pwd", "password", "pass"]).unwrap_or_default();
        return Some((
            host.to_string(),
            port,
            database_name.to_string(),
            username.to_string(),
            password.to_string(),
            String::new(),
        ));
    }

    let normalized = normalize_db_protocol(&db_type, trimmed);
    let without_scheme = normalized
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(normalized.as_str());
    let (authority_and_path, _query) = without_scheme
        .split_once('?')
        .map(|(left, right)| (left, Some(right)))
        .unwrap_or((without_scheme, None));

    let (credentials, host_path) = if let Some(at_index) = authority_and_path.rfind('@') {
        (
            Some(&authority_and_path[..at_index]),
            &authority_and_path[at_index + 1..],
        )
    } else {
        (None, authority_and_path)
    };

    let (username, password) = if let Some(credentials) = credentials {
        if let Some((user, pass)) = credentials.split_once(':') {
            (percent_decode_loose(user), percent_decode_loose(pass))
        } else {
            (percent_decode_loose(credentials), String::new())
        }
    } else {
        (String::new(), String::new())
    };

    let (host_port, path) = host_path
        .split_once('/')
        .map(|(host_port, path)| (host_port, path))
        .unwrap_or((host_path, ""));
    let (host, port) = parse_server_host_and_port(&db_type, host_port);
    let port = port.or_else(|| default_port_for_db_type(&db_type));
    let database_name = percent_decode_loose(path.trim_matches('/'));

    Some((
        host,
        port,
        database_name,
        username,
        password,
        String::new(),
    ))
}

fn enrich_list_request_from_connection_uri(
    mut request: SmartbrainDatabaseListRequest,
) -> SmartbrainDatabaseListRequest {
    if request.connection_uri.trim().is_empty() {
        return request;
    }

    let Some((host, port, database_name, username, password, file_path)) =
        parse_connection_uri_fields(&request.db_type, &request.connection_uri)
    else {
        return request;
    };

    if request.host.trim().is_empty() {
        request.host = host;
    }
    if request.port.is_none() {
        request.port = port;
    }
    if request.database_name.trim().is_empty() {
        request.database_name = database_name;
    }
    if request.username.trim().is_empty() {
        request.username = username;
    }
    if request.password.trim().is_empty() {
        request.password = password;
    }
    if request.file_path.trim().is_empty() {
        request.file_path = file_path;
    }

    request
}

fn parse_cli_database_names(stdout: &str) -> Vec<String> {'''

if old not in text:
    raise SystemExit("anchor not found for first_non_empty")
text = text.replace(old, new, 1)

old2 = """async fn list_databases_for_request(request: SmartbrainDatabaseListRequest) -> Result<Vec<String>, String> {
    let db_type = request.db_type.trim().to_ascii_lowercase();
    match db_type.as_str() {"""

new2 = """async fn list_databases_for_request(request: SmartbrainDatabaseListRequest) -> Result<Vec<String>, String> {
    let request = enrich_list_request_from_connection_uri(request);
    let db_type = request.db_type.trim().to_ascii_lowercase();
    match db_type.as_str() {"""

if old2 not in text:
    raise SystemExit("anchor not found for list_databases_for_request")
text = text.replace(old2, new2, 1)

path.write_text(text, encoding="utf-8")
print("updated commands.rs")
