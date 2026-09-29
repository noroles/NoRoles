//! Small shared pieces: dynamic values read from the company files, canonical JSON, hashes, time.
//! Company files are YAML written by people and by the JavaScript version, so values stay dynamic
//! (serde_json::Value) and are read the way the JavaScript version reads them.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub type V = Value;
pub type Obj = Map<String, Value>;
pub const DAY: i64 = 86_400_000;
pub const HOUR: i64 = 3_600_000;

pub fn sha(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }

/// A number the way JavaScript prints it, so hashes match the JavaScript version byte for byte.
pub fn js_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() { return i.to_string(); }
    if let Some(u) = n.as_u64() { return u.to_string(); }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if !f.is_finite() { return "null".into(); }
    let mut b = ryu_js::Buffer::new();
    b.format(f).to_string()
}

/// Canonical JSON: keys sorted at every level, so a hash covers every nested field.
pub fn canonical(v: &V) -> String {
    match v {
        V::Array(a) => format!("[{}]", a.iter().map(canonical).collect::<Vec<_>>().join(",")),
        V::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            format!("{{{}}}", keys.iter().map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), canonical(&o[k.as_str()]))).collect::<Vec<_>>().join(","))
        }
        V::Number(n) => js_number(n),
        V::String(s) => serde_json::to_string(s).unwrap(),
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
    }
}

/// A JSON number from f64 that prints as an integer when it is one (as JavaScript does).
pub fn num(f: f64) -> V {
    if f.fract() == 0.0 && f.abs() < 9e15 { V::from(f as i64) } else { serde_json::Number::from_f64(f).map(V::Number).unwrap_or(V::Null) }
}

// ---------- reading dynamic values like JavaScript ----------

pub fn truthy(v: &V) -> bool {
    match v { V::Null => false, V::Bool(b) => *b, V::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true), V::String(s) => !s.is_empty(), _ => true }
}
pub fn g<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
pub fn s<'a>(v: &'a V, k: &str) -> Option<&'a str> { v.get(k).and_then(|x| x.as_str()) }
/// String(x) for scalars, None for null/missing.
pub fn text(v: &V) -> Option<String> {
    match v { V::Null => None, V::String(s) => Some(s.clone()), V::Number(n) => Some(js_number(n)), V::Bool(b) => Some(b.to_string()), other => Some(other.to_string()) }
}
pub fn obj(v: &V, k: &str) -> Obj { v.get(k).and_then(|x| x.as_object()).cloned().unwrap_or_default() }
/// [].concat(x || []) as strings.
pub fn list(v: &V) -> Vec<String> {
    match v { V::Null => vec![], V::Array(a) => a.iter().filter_map(text).collect(), V::Bool(false) => vec![], x => text(x).into_iter().collect() }
}
pub fn listk(v: &V, k: &str) -> Vec<String> { list(g(v, k)) }
pub fn f64_of(v: &V) -> Option<f64> {
    match v { V::Number(n) => n.as_f64(), V::String(s) => s.trim().parse().ok(), _ => None }
}

// ---------- time as milliseconds since the epoch, like JavaScript Date ----------

pub fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }

pub fn iso(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()).unwrap_or_default()
}

/// new Date(String(v)).getTime(): a date alone is midnight UTC; a time without a zone is local.
pub fn parse_time(t: &str) -> Option<i64> {
    let t = t.trim();
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(t) { return Some(d.timestamp_millis()); }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") { return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis()); }
    for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(t, f) {
            use chrono::TimeZone;
            return chrono::Local.from_local_datetime(&d).single().map(|x| x.timestamp_millis());
        }
    }
    None
}
pub fn time_of(v: &V) -> Option<i64> { text(v).filter(|s| !s.is_empty()).and_then(|s| parse_time(&s)) }

pub fn parse_duration(v: &V, fallback: &str) -> Result<i64, String> {
    let s = text(v).unwrap_or_else(|| fallback.to_string());
    let t = s.trim();
    let (numpart, unit) = t.split_at(t.len().saturating_sub(1));
    let mult = match unit { "m" => 60_000.0, "h" => 3_600_000.0, "d" => DAY as f64, _ => return Err(format!("bad duration: {s}")) };
    let n: f64 = numpart.trim().parse().map_err(|_| format!("bad duration: {s}"))?;
    Ok((n * mult) as i64)
}

pub fn yaml(text: &str, file: &str) -> Result<V, String> {
    let v: V = serde_norway::from_str(text).map_err(|e| format!("{file}: {e}"))?;
    Ok(if v.is_null() { V::Object(Obj::new()) } else { v })
}
pub fn to_yaml(v: &V) -> String { serde_norway::to_string(v).unwrap_or_default() }
