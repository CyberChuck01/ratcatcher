//! Tunnel R.A.T. additions to Rayhunter for the Orbic.
//!
//! - `status`: live link state and session counters
//! - `pairing`: one-time pairing code -> saved API key
//! - `link`: the live, API-keyed WebSocket to the paired hub (green/red rat)
//! - `cells`: counts LTE cells from the modem's measurement logs
//! - `web`: the towers list for Rayhunter's web page (`GET /api/tunnel-rat/cells`)
//! - `orbic_screen`: draws the rat, pairing code and counters

pub mod cells;
pub mod link;
pub mod orbic_screen;
pub mod pairing;
pub mod status;
pub mod web;
