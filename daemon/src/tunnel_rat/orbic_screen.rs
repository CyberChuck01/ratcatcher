//! Tunnel R.A.T. — draws the Orbic's 128x128 status screen.
//!
//! Layout: the rat (96x96, tinted by status) on top, and a 30-pixel strip
//! underneath that shows the pairing code until the Orbic is paired, then the
//! cell and stingray counters while recording.
//!
//! Rat colours: purple = possible stingray, red = link lost,
//! yellow = no link yet, blue flash = new cell found, green = connected.
//!
//! Standalone mode (ui_level 6) is for an Orbic used on its own: no pairing
//! code, the counters show straight away, and the rat follows recording
//! instead of the link: green = recording, blue flash = new cell,
//! purple = possible stingray, grey = paused.
//!
//! Pure functions over a `Snapshot`, standard library only, so this file can
//! be compiled and tested without the rest of the daemon.

use super::status::{Link, Snapshot};

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 128;

pub type Rgb = (u8, u8, u8);

pub const YELLOW: Rgb = (255, 196, 0);
pub const GREEN: Rgb = (0, 230, 90);
pub const BLUE: Rgb = (0, 145, 255);
pub const RED: Rgb = (255, 32, 32);
pub const PURPLE: Rgb = (176, 64, 255);
pub const PAUSED_GREY: Rgb = (170, 170, 170);
const WHITE: Rgb = (240, 240, 240);
const GREY: Rgb = (140, 140, 140);
const DIVIDER: Rgb = (50, 50, 50);

/// Greyscale rat, 96x96, one byte per pixel, rows top to bottom.
const RAT: &[u8] = include_bytes!("tunnel-rat-orbic_rat_mask_96.gray");
const RAT_SIZE: usize = 96;
const RAT_X: usize = (WIDTH - RAT_SIZE) / 2;
const RAT_Y: usize = 1;
const STRIP_Y: usize = RAT_Y + RAT_SIZE + 2; // 99

/// Rat colour and whether it is in the dim half of a blink.
pub fn rat_color(s: &Snapshot, blink_on: bool, standalone: bool) -> (Rgb, bool) {
    if s.stingrays > 0 {
        return (PURPLE, false);
    }
    if standalone {
        return match (s.recording, s.new_cell) {
            (false, _) => (PAUSED_GREY, false),
            (true, true) => (BLUE, !blink_on),
            (true, false) => (GREEN, false),
        };
    }
    match s.link {
        Link::Lost => (RED, false),
        Link::Unpaired | Link::Waiting => (YELLOW, false),
        Link::Connected if s.new_cell => (BLUE, !blink_on),
        Link::Connected => (GREEN, false),
    }
}

/// Render one full frame, rows top to bottom, ready for `write_buffer`.
/// `standalone` hides the pairing code and colours the rat by recording state.
pub fn render(s: &Snapshot, blink_on: bool, standalone: bool) -> Vec<Rgb> {
    let mut fb = vec![(0u8, 0u8, 0u8); WIDTH * HEIGHT];
    let (color, dim) = rat_color(s, blink_on, standalone);
    draw_rat(&mut fb, color, if dim { 64 } else { 256 });

    match (&s.pair_code, s.link) {
        (Some(code), Link::Unpaired) if !standalone => {
            draw_text_centered(&mut fb, WIDTH / 2, STRIP_Y, "PAIR CODE", 1, YELLOW);
            draw_text_centered(&mut fb, WIDTH / 2, STRIP_Y + 10, code, 2, WHITE);
        }
        _ if s.recording => {
            let half = WIDTH / 2;
            draw_text_centered(&mut fb, half / 2, STRIP_Y, "CELLS", 1, GREY);
            draw_text_centered(&mut fb, half / 2, STRIP_Y + 10, &count(s.cells), 2, WHITE);
            let ray_color = if s.stingrays > 0 { PURPLE } else { WHITE };
            draw_text_centered(&mut fb, half + half / 2, STRIP_Y, "STINGRAY", 1, GREY);
            draw_text_centered(
                &mut fb,
                half + half / 2,
                STRIP_Y + 10,
                &count(s.stingrays),
                2,
                ray_color,
            );
            for y in STRIP_Y + 1..HEIGHT - 4 {
                fb[y * WIDTH + half] = DIVIDER;
            }
        }
        _ => draw_text_centered(&mut fb, WIDTH / 2, STRIP_Y + 7, "PAUSED", 2, GREY),
    }
    fb
}

fn count(n: u32) -> String {
    if n > 999 {
        "999+".to_string()
    } else {
        n.to_string()
    }
}

/// Gradient-map the greyscale rat: black -> status colour -> pale tint.
/// `level` is brightness out of 256.
fn draw_rat(fb: &mut [Rgb], color: Rgb, level: u32) {
    for y in 0..RAT_SIZE {
        for x in 0..RAT_SIZE {
            let m = RAT[y * RAT_SIZE + x] as u32;
            if m == 0 {
                continue;
            }
            let px = (
                tint(m, color.0, level),
                tint(m, color.1, level),
                tint(m, color.2, level),
            );
            fb[(RAT_Y + y) * WIDTH + RAT_X + x] = px;
        }
    }
}

fn tint(m: u32, c: u8, level: u32) -> u8 {
    const KNEE: u32 = 140; // ~55% grey maps to the pure status colour
    let c = c as u32;
    let v = if m <= KNEE {
        c * m / KNEE
    } else {
        c + (255 - c) * 55 * (m - KNEE) / (100 * (255 - KNEE))
    };
    (v * level / 256).min(255) as u8
}

// ---------------------------------------------------------------- 5x7 font --

const GLYPH_W: usize = 5;
const GLYPH_H: usize = 7;

fn glyph(c: char) -> [u8; GLYPH_H] {
    match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        _ => [0; GLYPH_H],
    }
}

pub fn text_width(text: &str, scale: usize) -> usize {
    let n = text.chars().count();
    if n == 0 {
        0
    } else {
        n * (GLYPH_W + 1) * scale - scale
    }
}

fn draw_text_centered(fb: &mut [Rgb], cx: usize, y: usize, text: &str, scale: usize, color: Rgb) {
    let x = cx.saturating_sub(text_width(text, scale) / 2);
    draw_text(fb, x, y, text, scale, color);
}

fn draw_text(fb: &mut [Rgb], x0: usize, y0: usize, text: &str, scale: usize, color: Rgb) {
    for (i, ch) in text.chars().enumerate() {
        let rows = glyph(ch.to_ascii_uppercase());
        let gx = x0 + i * (GLYPH_W + 1) * scale;
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..GLYPH_W {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let (x, y) = (gx + col * scale + dx, y0 + row * scale + dy);
                        if x < WIDTH && y < HEIGHT {
                            fb[y * WIDTH + x] = color;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(link: Link) -> Snapshot {
        Snapshot {
            link,
            pair_code: None,
            recording: true,
            cells: 0,
            stingrays: 0,
            new_cell: false,
        }
    }

    #[test]
    fn colour_priority() {
        let mut s = snap(Link::Connected);
        assert_eq!(rat_color(&s, true, false).0, GREEN);
        s.new_cell = true;
        assert_eq!(rat_color(&s, true, false), (BLUE, false));
        assert_eq!(rat_color(&s, false, false), (BLUE, true));
        assert_eq!(rat_color(&snap(Link::Waiting), true, false).0, YELLOW);
        assert_eq!(rat_color(&snap(Link::Unpaired), true, false).0, YELLOW);
        assert_eq!(rat_color(&snap(Link::Lost), true, false).0, RED);
        let mut s = snap(Link::Lost);
        s.stingrays = 2;
        assert_eq!(rat_color(&s, true, false).0, PURPLE); // a stingray beats everything
    }

    #[test]
    fn standalone_ignores_the_link() {
        // Never paired, but recording: green, not yellow
        let mut s = snap(Link::Unpaired);
        assert_eq!(rat_color(&s, true, true).0, GREEN);
        s.new_cell = true;
        assert_eq!(rat_color(&s, false, true), (BLUE, true));
        s.recording = false;
        assert_eq!(rat_color(&s, true, true).0, PAUSED_GREY);
        s.stingrays = 1;
        assert_eq!(rat_color(&s, true, true).0, PURPLE);
        assert_eq!(rat_color(&snap(Link::Lost), true, true).0, GREEN); // no red without a hub
    }

    #[test]
    fn standalone_hides_the_pair_code() {
        let mut s = snap(Link::Unpaired);
        s.pair_code = Some("7KX4QM".into());
        let strip = |fb: &[Rgb]| fb[STRIP_Y * WIDTH..].to_vec();
        // Paired mode draws the yellow "PAIR CODE" label in the strip...
        assert!(strip(&render(&s, true, false)).contains(&YELLOW));
        // ...standalone draws the counters instead, with no yellow anywhere.
        let fb = render(&s, true, true);
        assert!(!fb.contains(&YELLOW));
        assert!(strip(&fb).contains(&DIVIDER)); // the line between CELLS and STINGRAY
    }

    #[test]
    fn frame_is_full_screen_and_text_fits() {
        assert_eq!(RAT.len(), RAT_SIZE * RAT_SIZE);
        let mut s = snap(Link::Unpaired);
        s.pair_code = Some("7KX4QM".into());
        assert_eq!(render(&s, true, false).len(), WIDTH * HEIGHT);
        assert!(text_width("7KX4QM", 2) <= WIDTH);
        assert!(text_width("STINGRAY", 1) <= WIDTH / 2);
        assert!(text_width(&count(5000), 2) <= WIDTH / 2);
        assert!(STRIP_Y + 10 + GLYPH_H * 2 <= HEIGHT);
    }

    #[test]
    fn tint_endpoints() {
        assert_eq!(tint(0, 200, 256), 0);
        assert_eq!(tint(140, 200, 256), 200);
        assert_eq!(tint(255, 0, 256), 140); // pale tint, not pure white
    }
}
