//! Tunnel R.A.T. — data for the Towers panel in Rayhunter's web page.
//!
//! `GET /api/tunnel-rat/cells` returns every cell the Orbic has heard this
//! session, the link state and the stingray count. Like the rest of
//! Rayhunter's web API it has no login: it is reachable by anything on the
//! Orbic's own Wi-Fi.

use axum::Json;
use serde_json::{Value, json};

use super::status::{self, Link};

pub async fn cells() -> Json<Value> {
    Json(cells_json())
}

fn cells_json() -> Value {
    let snap = status::snapshot();
    let link = match snap.link {
        Link::Unpaired => "unpaired",
        Link::Waiting => "waiting",
        Link::Connected => "connected",
        Link::Lost => "lost",
    };
    let cells: Vec<Value> = status::cells_detail()
        .into_iter()
        .map(|c| match c.id {
            Some((plmn, tac, eci)) => json!({
                "earfcn": c.earfcn, "pci": c.pci,
                "plmn": plmn, "tac": tac, "eci": eci,
                "enb": eci >> 8, "sector": eci & 0xFF,
            }),
            None => json!({
                "earfcn": c.earfcn, "pci": c.pci,
                "plmn": null, "tac": null, "eci": null, "enb": null, "sector": null,
            }),
        })
        .collect();
    json!({
        "link": link,
        "recording": snap.recording,
        "stingrays": snap.stingrays,
        "cells": cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_json_has_the_expected_shape() {
        let v = cells_json();
        assert!(v["cells"].is_array());
        assert!(v["link"].is_string());
    }
}
