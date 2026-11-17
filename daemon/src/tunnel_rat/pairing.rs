//! Tunnel R.A.T. — one-time pairing.
//!
//! Until paired, the Orbic shows a 6-character code under the yellow rat.
//! Tunnel R.A.T. (or a PiPack, or anything else) sends that code once to
//! `POST /api/tunnel-rat/pair` and gets back a long API key, which the Orbic
//! saves. Every later connection uses the key. Five wrong guesses, or ten
//! minutes, swap in a fresh code.

use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::status::{self, Link};

const DIR: &str = "/data/tunnel-rat";
const KEY_PATH: &str = "/data/tunnel-rat/paired.json";
const CODE_TTL: Duration = Duration::from_secs(600);
const MAX_TRIES: u8 = 5;
/// No 0/O, 1/I/L, so the code is easy to read off a tiny screen.
const CODE_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const KEY_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

#[derive(Serialize, Deserialize, Clone)]
pub struct Paired {
    pub api_key: String,
    pub device_name: String,
}

struct Pairing {
    code: Option<String>,
    issued: Instant,
    tries: u8,
    paired: Option<Paired>,
}

static PAIRING: OnceLock<Mutex<Pairing>> = OnceLock::new();

fn lock() -> MutexGuard<'static, Pairing> {
    PAIRING
        .get_or_init(|| Mutex::new(Pairing::load()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

impl Pairing {
    fn load() -> Self {
        let paired: Option<Paired> = std::fs::read_to_string(KEY_PATH)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        let mut p = Pairing {
            code: None,
            issued: Instant::now(),
            tries: 0,
            paired,
        };
        if p.paired.is_some() {
            status::set_link(Link::Waiting);
            status::set_pair_code(None);
        } else {
            p.new_code();
        }
        p
    }

    fn new_code(&mut self) {
        let code = random_string(CODE_CHARS, 6);
        self.code = Some(code.clone());
        self.issued = Instant::now();
        self.tries = 0;
        status::set_link(Link::Unpaired);
        status::set_pair_code(Some(code));
    }
}

/// Load the saved pairing (or make a code) at startup.
pub fn init() {
    drop(lock());
}

/// Called by the screen loop: rotates the code once it expires.
pub fn tick() {
    let mut p = lock();
    if p.paired.is_none() && p.issued.elapsed() > CODE_TTL {
        p.new_code();
    }
}

/// For the Tunnel R.A.T. link: is this the key we handed out?
#[allow(dead_code)] // used once the link to Tunnel R.A.T. lands
pub fn is_valid_key(key: &str) -> bool {
    lock()
        .paired
        .as_ref()
        .is_some_and(|p| constant_eq(p.api_key.as_bytes(), key.as_bytes()))
}

/// Forget the pairing and show a fresh code.
#[allow(dead_code)] // hooked to a button gesture once it is tested on hardware
pub fn reset() {
    let _ = std::fs::remove_file(KEY_PATH);
    let mut p = lock();
    p.paired = None;
    p.new_code();
}

#[derive(Deserialize)]
pub struct PairReq {
    code: String,
    #[serde(default)]
    device_name: String,
}

#[derive(Serialize)]
pub struct PairResp {
    api_key: String,
}

/// POST /api/tunnel-rat/pair  {"code":"7KX4QM","device_name":"Tunnel R.A.T."}
/// 200 + key on success, 401 wrong code, 409 already paired.
pub async fn pair(Json(req): Json<PairReq>) -> Result<Json<PairResp>, StatusCode> {
    let mut p = lock();
    if p.paired.is_some() {
        return Err(StatusCode::CONFLICT);
    }
    if p.issued.elapsed() > CODE_TTL {
        p.new_code();
    }
    let code = p.code.clone().unwrap_or_default();
    let guess = req.code.trim().to_ascii_uppercase();
    if !constant_eq(code.as_bytes(), guess.as_bytes()) {
        p.tries += 1;
        if p.tries >= MAX_TRIES {
            p.new_code(); // brute-force guard
        }
        return Err(StatusCode::UNAUTHORIZED);
    }

    let device_name = if req.device_name.trim().is_empty() {
        "Tunnel R.A.T.".to_string()
    } else {
        req.device_name.trim().chars().take(64).collect()
    };
    let rec = Paired {
        api_key: random_string(KEY_CHARS, 40),
        device_name,
    };
    save(&rec).map_err(|e| {
        log::error!("tunnel-rat: couldn't save pairing: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    log::info!("tunnel-rat: paired with {}", rec.device_name);
    let api_key = rec.api_key.clone();
    p.paired = Some(rec);
    p.code = None;
    status::set_pair_code(None);
    status::set_link(Link::Waiting);
    Ok(Json(PairResp { api_key }))
}

/// Write via a temp file so a power cut can't leave a half-written key.
fn save(rec: &Paired) -> std::io::Result<()> {
    std::fs::create_dir_all(DIR)?;
    let tmp = format!("{KEY_PATH}.tmp");
    std::fs::write(&tmp, serde_json::to_string(rec)?)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, KEY_PATH)
}

/// Unbiased random string from /dev/urandom (no extra crates needed).
fn random_string(alphabet: &[u8], len: usize) -> String {
    let limit = 256 - (256 % alphabet.len()); // reject bytes that would skew the odds
    let mut urandom = std::fs::File::open("/dev/urandom").expect("no /dev/urandom");
    let mut out = String::with_capacity(len);
    let mut byte = [0u8; 1];
    while out.len() < len {
        urandom
            .read_exact(&mut byte)
            .expect("couldn't read /dev/urandom");
        if (byte[0] as usize) < limit {
            out.push(alphabet[byte[0] as usize % alphabet.len()] as char);
        }
    }
    out
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_use_only_readable_characters() {
        for _ in 0..50 {
            let c = random_string(CODE_CHARS, 6);
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| CODE_CHARS.contains(&b)));
        }
        assert_eq!(random_string(KEY_CHARS, 40).len(), 40);
    }

    #[test]
    fn constant_eq_works() {
        assert!(constant_eq(b"7KX4QM", b"7KX4QM"));
        assert!(!constant_eq(b"7KX4QM", b"7KX4QN"));
        assert!(!constant_eq(b"7KX4QM", b"7KX4Q"));
    }
}
