//! Tray icons: the six states use different colors and shapes, with no external image files.

use crate::state::TrayState;

pub const SIZE: u32 = 32;

type Rgba = [u8; 4];

fn paint(f: impl Fn(f32, f32) -> Option<Rgba>) -> Vec<u8> {
    let mut buf = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Coordinates are measured from the icon center, at pixel centers.
            let (dx, dy) = (x as f32 + 0.5 - SIZE as f32 / 2.0, y as f32 + 0.5 - SIZE as f32 / 2.0);
            buf.extend_from_slice(&f(dx, dy).unwrap_or([0, 0, 0, 0]));
        }
    }
    buf
}

/// Returns `SIZE x SIZE` RGBA pixels.
pub fn rgba(state: TrayState) -> Vec<u8> {
    const WHITE: Rgba = [255, 255, 255, 255];
    match state {
        // Synced: solid green circle
        TrayState::Synced => paint(|dx, dy| (dx * dx + dy * dy <= 13.0 * 13.0).then_some([34, 197, 94, 255])),
        // Syncing: blue circle with a white dot in the middle
        TrayState::Syncing => paint(|dx, dy| {
            let d2 = dx * dx + dy * dy;
            if d2 <= 5.0 * 5.0 {
                Some(WHITE)
            } else if d2 <= 13.0 * 13.0 {
                Some([59, 130, 246, 255])
            } else {
                None
            }
        }),
        // Offline: grey hollow ring
        TrayState::Offline => paint(|dx, dy| {
            let d2 = dx * dx + dy * dy;
            (d2 <= 13.0 * 13.0 && d2 >= 8.0 * 8.0).then_some([156, 163, 175, 255])
        }),
        // Local only: purple circle with a white bar across the middle
        TrayState::Local => paint(|dx, dy| {
            if dx * dx + dy * dy > 13.0 * 13.0 {
                None
            } else if dy.abs() <= 2.5 && dx.abs() <= 8.0 {
                Some(WHITE)
            } else {
                Some([139, 92, 246, 255])
            }
        }),
        // Paused: amber diamond with two white bars
        TrayState::Paused => paint(|dx, dy| {
            if dx.abs() + dy.abs() > 14.0 {
                None
            } else if dx.abs() >= 2.0 && dx.abs() <= 4.5 && dy.abs() <= 6.0 {
                Some(WHITE)
            } else {
                Some([245, 158, 11, 255])
            }
        }),
        // Error: red square with a white cross
        TrayState::Error => paint(|dx, dy| {
            if dx.abs() > 12.0 || dy.abs() > 12.0 {
                None
            } else if (dx.abs() - dy.abs()).abs() <= 1.6 && dx.abs() <= 7.0 {
                Some(WHITE)
            } else {
                Some([239, 68, 68, 255])
            }
        }),
    }
}

/// The same six shapes as a template image for the macOS menu bar: black where the shape is, transparent where the picture
/// has a hole or nothing. The system draws a template in the colour that suits the menu bar (light or dark), so it is
/// always visible; the states differ by shape, not by colour.
pub fn template(state: TrayState) -> Vec<u8> {
    let mut px = rgba(state);
    for p in px.chunks_mut(4) {
        let hole = p[3] == 0 || (p[0] == 255 && p[1] == 255 && p[2] == 255);
        p.copy_from_slice(if hole { &[0, 0, 0, 0] } else { &[0, 0, 0, 255] });
    }
    px
}

/// What the tray shows: the template image on macOS, the coloured one elsewhere.
pub fn tray_pixels(state: TrayState) -> Vec<u8> {
    if cfg!(target_os = "macos") {
        template(state)
    } else {
        rgba(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_images_differ_by_shape_and_have_something_to_draw() {
        let all = [TrayState::Synced, TrayState::Syncing, TrayState::Offline, TrayState::Error, TrayState::Local, TrayState::Paused].map(template);
        for i in 0..all.len() {
            assert!(all[i].chunks(4).any(|p| p[3] == 255), "a template with nothing in it would be invisible");
            assert!(all[i].chunks(4).all(|p| p[..3] == [0, 0, 0]), "templates are black and transparent only");
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "two states look the same in the menu bar");
            }
        }
    }

    #[test]
    fn the_six_icons_are_pairwise_different() {
        let all = [TrayState::Synced, TrayState::Syncing, TrayState::Offline, TrayState::Error, TrayState::Local, TrayState::Paused].map(rgba);
        for i in 0..all.len() {
            assert_eq!(all[i].len(), (SIZE * SIZE * 4) as usize);
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }
}
