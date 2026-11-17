//! Tunnel R.A.T. — feeds the cell counter from the modem's own measurements.
//!
//! Rayhunter already asks the modem for LTE serving-cell and neighbour-cell
//! measurement logs. Each (EARFCN, PCI) pair in them is one cell the Orbic
//! can hear. A single tower usually carries several cells (one per sector and
//! band), so this is a cell count; grouping cells into physical towers needs
//! the cell IDs from SIB1 and happens later, on the map side.

use rayhunter::diag::Message;
use rayhunter::diag::diaglog::LogBody;

use super::status;

pub fn observe_message(msg: &Message) {
    let Message::Log { body, .. } = msg else {
        return;
    };
    match body {
        LogBody::LteMl1ServingCellMeasurementAndEvaluation { data } => {
            status::cell_seen(data.get_earfcn(), data.get_pci());
        }
        LogBody::LteMl1NeighborCellsMeasurements { data } => {
            let earfcn = data.get_earfcn();
            for cell in &data.cells {
                status::cell_seen(earfcn, cell.pci);
            }
        }
        _ => {}
    }
}
