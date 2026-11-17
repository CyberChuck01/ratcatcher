//! Tunnel R.A.T. — feeds the cell counter from the modem's own measurements.
//!
//! Rayhunter already asks the modem for LTE serving-cell and neighbour-cell
//! measurement logs. Each (EARFCN, PCI) pair in them is one cell the Orbic
//! can hear. A single tower usually carries several cells (one per sector and
//! band), so this is a cell count; grouping cells into physical towers needs
//! the cell IDs from SIB1 and happens later, on the map side.

use rayhunter::analysis::information_element::{InformationElement, LteInformationElement};
use rayhunter::diag::Message;
use rayhunter::diag::diaglog::LogBody;
use rayhunter::gsmtap::parser as gsmtap_parser;
use telcom_parser::lte_rrc::{BCCH_DL_SCH_MessageType, BCCH_DL_SCH_MessageType_c1};

use super::status;

/// Value of a bit string (most significant bit first), e.g. the 16-bit TAC
/// or the 28-bit cell identity from SIB1.
fn bits_value<I>(bits: I) -> u32
where
    I: IntoIterator,
    I::Item: std::ops::Deref<Target = bool>,
{
    bits.into_iter()
        .fold(0u32, |acc, bit| (acc << 1) | u32::from(*bit))
}

/// SIB1 carries the cell's real identity: PLMN (MCC + MNC), tracking area
/// code and cell identity. Rayhunter already decodes it for its own checks.
fn observe_sib1(msg: &Message) {
    let Message::Log {
        body: LogBody::LteRrcOtaMessage { .. },
        ..
    } = msg
    else {
        return;
    };
    let Ok(Some((_, gsmtap_msg))) = gsmtap_parser::parse(msg.clone()) else {
        return;
    };
    let Ok(ie) = InformationElement::try_from(&gsmtap_msg) else {
        return;
    };
    if let InformationElement::LTE(lte_ie) = &ie
        && let LteInformationElement::BcchDlSch(sch_msg) = &**lte_ie
        && let BCCH_DL_SCH_MessageType::C1(c1) = &sch_msg.message
        && let BCCH_DL_SCH_MessageType_c1::SystemInformationBlockType1(sib1) = c1
    {
        let info = &sib1.cell_access_related_info;
        let Some(first) = info.plmn_identity_list.0.first() else {
            return;
        };
        let Some(mcc) = &first.plmn_identity.mcc else {
            return;
        };
        let digits = |d: &[telcom_parser::lte_rrc::MCC_MNC_Digit]| -> String {
            d.iter().map(|x| char::from(b'0' + x.0.min(9))).collect()
        };
        let plmn = format!("{}-{}", digits(&mcc.0), digits(&first.plmn_identity.mnc.0));
        let tac = bits_value(info.tracking_area_code.0.iter()) as u16;
        let eci = bits_value(info.cell_identity.0.iter());
        status::cell_id_seen(plmn, tac, eci);
    }
}

pub fn observe_message(msg: &Message) {
    observe_sib1(msg);
    let Message::Log { body, .. } = msg else {
        return;
    };
    match body {
        LogBody::LteMl1ServingCellMeasurementAndEvaluation { data } => {
            status::set_serving(data.get_earfcn(), data.get_pci());
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
