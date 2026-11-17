//! Tunnel R.A.T. screen loop for the Orbic (ui_level = 5).
//!
//! Redraws every 250 ms so the blue new-cell flash is smooth, but only writes
//! to the panel when the picture changes, or once a second as a refresh in
//! case the device's own UI drew over it.

use std::time::{Duration, Instant};

use log::info;
use tokio::sync::mpsc::{Receiver, error::TryRecvError};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::display::DisplayState;
use crate::display::generic_framebuffer::GenericFramebuffer;
use crate::tunnel_rat::{orbic_screen, pairing, status};

const TICK: Duration = Duration::from_millis(250);
const BLINK: Duration = Duration::from_millis(500);
const FORCE_REDRAW: Duration = Duration::from_secs(1);

pub fn update_ui(
    task_tracker: &TaskTracker,
    mut fb: impl GenericFramebuffer,
    shutdown_token: CancellationToken,
    mut ui_update_rx: Receiver<DisplayState>,
) {
    pairing::init();
    task_tracker.spawn(async move {
        let start = Instant::now();
        let mut last_frame: Option<Vec<orbic_screen::Rgb>> = None;
        let mut last_write = Instant::now();

        loop {
            if shutdown_token.is_cancelled() {
                info!("received UI shutdown");
                break;
            }

            // Keep draining: the diag thread waits on this channel.
            loop {
                match ui_update_rx.try_recv() {
                    Ok(DisplayState::Recording) => status::new_session(),
                    Ok(DisplayState::Paused) => status::set_recording(false),
                    Ok(DisplayState::WarningDetected { .. }) => status::ensure_alert(),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }

            pairing::tick();
            let blink_on = (start.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0;
            let frame = orbic_screen::render(&status::snapshot(), blink_on);

            if last_frame.as_ref() != Some(&frame) || last_write.elapsed() >= FORCE_REDRAW {
                fb.write_buffer(frame.clone()).await;
                last_frame = Some(frame);
                last_write = Instant::now();
            }
            tokio::time::sleep(TICK).await;
        }
    });
}
