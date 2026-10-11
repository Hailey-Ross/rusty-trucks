//! Button prompt glyphs for the connected pad (doc 31, "Button prompts"): the retail menus draw a native
//! "CustomButton" component for the Xbox 360 button named by `_test`; we draw the button's printed label as SDL
//! reports it for the pad (`ControllerKind::face_labels`), so PlayStation pads show Cross / Circle / Square /
//! Triangle and Nintendo pads their own letters. The glyphs are generated in retail's prompt style (a coloured
//! ball, a light symbol, a dark outline), not console art; mods replace them through the menu bitmap path later.
//! Size: retail's glyph spans about 0.83 of the CustomButton placeholder box (10 of 480 px in a recomp shot of the
//! Trick Guide help row, `.local/research/menus-m6/retail/`; the box is 32 of 1280), centred in it; placement is
//! the movie's own (button_item2 lays the box out).
use crate::input::controller_kind::PromptLabel;

/// Glyph size in pixels (square RGBA8).
pub(crate) const SIZE: u32 = 64;
/// Ball radius as a share of the box (retail: diameter about 0.83 of the placeholder).
const RADIUS: f32 = 0.415;

/// The retail button name in `_test` -> face position (South, East, West, North), Xbox 360 naming.
pub(crate) fn face_index(retail: &str) -> Option<usize> {
    Some(match retail {
        "A" => 0,
        "B" => 1,
        "X" => 2,
        "Y" => 3,
        _ => return None,
    })
}

enum Stroke {
    Line([f32; 2], [f32; 2]),
    /// Centre, radius, right half only (the bowls of a B).
    Arc([f32; 2], f32, bool),
}

fn shape(label: PromptLabel) -> (Vec<Stroke>, [f32; 3]) {
    use Stroke::{Arc, Line};
    match label {
        PromptLabel::A => (
            vec![Line([0.31, 0.74], [0.5, 0.26]), Line([0.5, 0.26], [0.69, 0.74]), Line([0.38, 0.57], [0.62, 0.57])],
            [0.45, 0.78, 0.33],
        ),
        PromptLabel::B => (
            vec![
                Line([0.37, 0.26], [0.37, 0.74]),
                Line([0.37, 0.26], [0.52, 0.26]),
                Arc([0.52, 0.37], 0.11, true),
                Line([0.37, 0.48], [0.54, 0.48]),
                Arc([0.54, 0.61], 0.13, true),
                Line([0.37, 0.74], [0.54, 0.74]),
            ],
            [0.91, 0.33, 0.3],
        ),
        PromptLabel::X => (vec![Line([0.33, 0.27], [0.67, 0.73]), Line([0.67, 0.27], [0.33, 0.73])], [0.31, 0.56, 0.93]),
        PromptLabel::Y => (
            vec![Line([0.33, 0.27], [0.5, 0.5]), Line([0.67, 0.27], [0.5, 0.5]), Line([0.5, 0.5], [0.5, 0.74])],
            [0.96, 0.78, 0.27],
        ),
        PromptLabel::Cross => (vec![Line([0.33, 0.33], [0.67, 0.67]), Line([0.67, 0.33], [0.33, 0.67])], [0.52, 0.66, 0.92]),
        PromptLabel::Circle => (vec![Arc([0.5, 0.5], 0.19, false)], [0.92, 0.38, 0.38]),
        PromptLabel::Square => (
            vec![
                Line([0.33, 0.33], [0.67, 0.33]),
                Line([0.67, 0.33], [0.67, 0.67]),
                Line([0.67, 0.67], [0.33, 0.67]),
                Line([0.33, 0.67], [0.33, 0.33]),
            ],
            [0.86, 0.55, 0.78],
        ),
        PromptLabel::Triangle => (
            vec![Line([0.5, 0.29], [0.7, 0.66]), Line([0.7, 0.66], [0.3, 0.66]), Line([0.3, 0.66], [0.5, 0.29])],
            [0.3, 0.78, 0.66],
        ),
    }
}

fn distance(p: [f32; 2], stroke: &Stroke) -> f32 {
    match *stroke {
        Stroke::Line(a, b) => {
            let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
            let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1])).clamp(0.0, 1.0);
            (ap[0] - ab[0] * t).hypot(ap[1] - ab[1] * t)
        }
        Stroke::Arc(c, r, right_half) => {
            let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
            if !right_half || dx >= 0.0 {
                (dx.hypot(dy) - r).abs()
            } else {
                // Off the half circle: the nearest end point (top or bottom).
                dx.hypot(dy - r).min(dx.hypot(dy + r))
            }
        }
    }
}

/// The glyph as RGBA8, `SIZE` x `SIZE`, sRGB.
pub(crate) fn glyph(label: PromptLabel) -> Vec<u8> {
    let (strokes, colour) = shape(label);
    let n = SIZE as f32;
    let pixel = 1.0 / n;
    let half_width = 0.045;
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = [(x as f32 + 0.5) / n, (y as f32 + 0.5) / n];
            let r = (p[0] - 0.5).hypot(p[1] - 0.5);
            let coverage = |d: f32| (0.5 - d / pixel).clamp(0.0, 1.0);
            let disc = coverage(r - RADIUS);
            let inner = coverage(r - (RADIUS - 0.045));
            // The symbol at the ball's scale (strokes are authored for a 0.46 ball).
            let k = RADIUS / 0.46;
            let q = [0.5 + (p[0] - 0.5) / k, 0.5 + (p[1] - 0.5) / k];
            let ink = strokes.iter().map(|s| coverage((distance(q, s) - half_width) * k)).fold(0.0, f32::max) * inner;
            // Lit from the top, like a button.
            let shade = 1.0 + 0.18 * (0.5 - p[1]).max(-0.3) / 0.5;
            let outline = [0.08, 0.08, 0.09];
            let rgb: [f32; 3] = std::array::from_fn(|i| {
                let ball = outline[i] + (colour[i] * shade - outline[i]) * inner;
                ball + (0.97 - ball) * ink
            });
            out.extend(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
            out.push((disc * 255.0).round() as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_a_coloured_ball_with_a_light_symbol() {
        for label in [PromptLabel::A, PromptLabel::B, PromptLabel::Cross, PromptLabel::Circle, PromptLabel::Triangle] {
            let g = glyph(label);
            assert_eq!(g.len(), (SIZE * SIZE * 4) as usize);
            let px = |x: u32, y: u32| &g[((y * SIZE + x) * 4) as usize..][..4];
            assert_eq!(px(0, 0)[3], 0, "outside the disc is clear");
            assert_eq!(px(SIZE / 2, (SIZE as f32 * 0.07) as u32)[3], 0, "the ball spans 0.83 of the box");
            assert!(px(SIZE / 2, SIZE / 2)[3] > 200, "the ball is opaque");
        }
        let at = |g: &[u8], x: f32, y: f32| {
            let i = (((y * SIZE as f32) as u32 * SIZE + (x * SIZE as f32) as u32) * 4) as usize;
            [g[i], g[i + 1], g[i + 2]]
        };
        // The A is a green ball with a light letter; inside a Circle's ring the red ball shows.
        let a = glyph(PromptLabel::A);
        let ball = at(&a, 0.5, 0.82);
        assert!(ball[1] > ball[0] && ball[1] > ball[2], "green ball {ball:?}");
        assert!(at(&a, 0.5, 0.57).iter().all(|c| *c > 200), "light crossbar");
        let c = at(&glyph(PromptLabel::Circle), 0.5, 0.5);
        assert!(c[0] > c[1] && c[0] > c[2], "red ball {c:?}");
        assert_eq!(face_index("B"), Some(1));
        assert_eq!(face_index("LB"), None);
    }
}
