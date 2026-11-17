//! Tunnel R.A.T. — live status shared by the diag thread, the pairing
//! endpoint and the Orbic screen.
//!
//! It is one process-wide value on purpose: every part of the fork can reach
//! it without changing upstream Rayhunter's function signatures, which keeps
//! pulling in new EFF releases easy. Standard library only, so the screen
//! renderer can be compiled and tested on its own.

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// State of the link to Tunnel R.A.T. (or a PiPack, or anything else paired).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// Never paired: the screen shows the pairing code.
    Unpaired,
    /// Paired, but nothing has connected yet this boot.
    Waiting,
    /// A paired client is connected right now.
    Connected,
    /// A paired client was connected and dropped.
    Lost,
}

/// Warnings closer together than this count as one stingray encounter.
pub const ALERT_MERGE: Duration = Duration::from_secs(60);
/// How long the rat flashes blue after a cell it hasn't seen before shows up.
pub const CELL_FLASH: Duration = Duration::from_secs(3);
/// How many recent events the live link can catch up on.
const EVENT_BACKLOG: usize = 256;

/// Something the live link forwards to the paired hub as it happens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A cell this session hasn't heard before.
    Cell { earfcn: u32, pci: u16 },
    /// A new stingray encounter (bursts are merged like the counter).
    Alert { severity: String, stingrays: u32 },
    /// A new recording started; counters went back to zero.
    NewSession,
}

struct Inner {
    link: Link,
    pair_code: Option<String>,
    recording: bool,
    /// LTE cells seen this session, keyed by (EARFCN, PCI).
    cells: BTreeSet<(u32, u16)>,
    last_new_cell: Option<Instant>,
    stingrays: u32,
    last_alert: Option<Instant>,
    events: VecDeque<(u64, Event)>,
    next_seq: u64,
}

static STATUS: Mutex<Inner> = Mutex::new(Inner {
    link: Link::Unpaired,
    pair_code: None,
    recording: false,
    cells: BTreeSet::new(),
    last_new_cell: None,
    stingrays: 0,
    last_alert: None,
    events: VecDeque::new(),
    next_seq: 1,
});

fn lock() -> MutexGuard<'static, Inner> {
    // A panic elsewhere must never take the status screen down with it.
    STATUS.lock().unwrap_or_else(|e| e.into_inner())
}

fn push_event(s: &mut Inner, event: Event) {
    let seq = s.next_seq;
    s.next_seq += 1;
    if s.events.len() == EVENT_BACKLOG {
        s.events.pop_front();
    }
    s.events.push_back((seq, event));
}

/// Events newer than `seq`, plus the sequence number to pass next time.
/// Start with `latest_seq()` to get only what happens from now on.
pub fn events_after(seq: u64) -> (Vec<Event>, u64) {
    let s = lock();
    let events = s
        .events
        .iter()
        .filter(|(n, _)| *n > seq)
        .map(|(_, e)| e.clone())
        .collect();
    (events, s.next_seq - 1)
}

pub fn latest_seq() -> u64 {
    lock().next_seq - 1
}

/// Every cell heard this session, as (EARFCN, PCI).
pub fn cells_list() -> Vec<(u32, u16)> {
    lock().cells.iter().copied().collect()
}

/// Everything the screen needs, copied out so no lock is held while drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub link: Link,
    pub pair_code: Option<String>,
    pub recording: bool,
    pub cells: u32,
    pub stingrays: u32,
    /// A new cell appeared within the last `CELL_FLASH`.
    pub new_cell: bool,
}

pub fn snapshot() -> Snapshot {
    let s = lock();
    Snapshot {
        link: s.link,
        pair_code: s.pair_code.clone(),
        recording: s.recording,
        cells: s.cells.len() as u32,
        stingrays: s.stingrays,
        new_cell: s.last_new_cell.is_some_and(|t| t.elapsed() < CELL_FLASH),
    }
}

pub fn set_link(link: Link) {
    lock().link = link;
}

pub fn set_pair_code(code: Option<String>) {
    lock().pair_code = code;
}

/// A new recording started: clear the counters for a fresh session.
pub fn new_session() {
    let mut s = lock();
    s.recording = true;
    s.cells.clear();
    s.last_new_cell = None;
    s.stingrays = 0;
    s.last_alert = None;
    push_event(&mut s, Event::NewSession);
}

pub fn set_recording(recording: bool) {
    lock().recording = recording;
}

/// The modem reported a serving or neighbour LTE cell.
pub fn cell_seen(earfcn: u32, pci: u16) {
    let mut s = lock();
    if s.cells.insert((earfcn, pci)) {
        s.last_new_cell = Some(Instant::now());
        push_event(&mut s, Event::Cell { earfcn, pci });
    }
}

/// A Rayhunter heuristic fired. Bursts within `ALERT_MERGE` count once.
/// `severity` is Rayhunter's event type, e.g. "Medium" or "High".
pub fn stingray_hit(severity: &str) {
    let mut s = lock();
    let now = Instant::now();
    if s.last_alert
        .is_none_or(|t| now.duration_since(t) >= ALERT_MERGE)
    {
        s.stingrays += 1;
        let stingrays = s.stingrays;
        push_event(
            &mut s,
            Event::Alert {
                severity: severity.to_string(),
                stingrays,
            },
        );
    }
    s.last_alert = Some(now);
}

/// Make sure a warning shows even if it arrived by a path that skipped
/// `stingray_hit` (for example Rayhunter's debug display-state endpoint).
pub fn ensure_alert() {
    let mut s = lock();
    if s.stingrays == 0 {
        s.stingrays = 1;
        s.last_alert = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The status is process-wide, so keep every assertion in one test.
    #[test]
    fn counters_dedupe_and_reset() {
        new_session();
        cell_seen(5230, 101);
        cell_seen(5230, 101);
        cell_seen(5230, 102);
        cell_seen(66486, 101);
        let seq = latest_seq();
        stingray_hit("High");
        stingray_hit("High"); // same burst
        let s = snapshot();
        assert_eq!(s.cells, 3);
        assert_eq!(s.stingrays, 1);
        assert!(s.new_cell);
        assert!(s.recording);
        // one alert event for the burst, none for the repeat
        let (ev, next) = events_after(seq);
        assert_eq!(
            ev,
            vec![Event::Alert {
                severity: "High".into(),
                stingrays: 1
            }]
        );
        assert_eq!(events_after(next).0, vec![]);
        assert_eq!(cells_list(), vec![(5230, 101), (5230, 102), (66486, 101)]);

        let seq = latest_seq();
        new_session();
        let s = snapshot();
        assert_eq!((s.cells, s.stingrays, s.new_cell), (0, 0, false));
        assert_eq!(events_after(seq).0, vec![Event::NewSession]);
        cell_seen(5230, 101);
        assert_eq!(
            events_after(seq).0,
            vec![Event::NewSession, Event::Cell { earfcn: 5230, pci: 101 }]
        );
        ensure_alert();
        assert_eq!(snapshot().stingrays, 1);
    }
}
