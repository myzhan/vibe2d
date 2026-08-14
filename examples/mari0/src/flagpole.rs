//! Finishing a level: what the flagpole is worth, and how many fireworks you get.
//!
//! Both are lookup tables with a quirk, and both are pure functions of one number, so
//! they live here away from the game loop.

/// Points for grabbing the pole, by height (`flagscores`, `variables.lua:342`).
const FLAG_SCORES: [u32; 5] = [100, 400, 800, 2000, 5000];

/// The heights that separate them, as the **top edge of Mario in blocks**
/// (`flagvalues`). Lower numbers are higher up the pole, so the test is `y < value`.
///
/// Four thresholds for five bands, and the walk up is not linear: 100 at the bottom,
/// then 400, 800, 2000, and 5000 only for the very top of the pole.
const FLAG_HEIGHTS: [f32; 4] = [9.8125, 7.3125, 5.8125, 2.9375];

/// Each firework is worth 500 (`mario.lua:462`).
pub(crate) const FIREWORK_SCORE: u32 = 500;

/// What the pole pays out for a grab with the player's top edge at `top_blocks`.
///
/// The loop **stops at the first threshold you fail** rather than scanning them all
/// (`mario.lua:2942-2950`). With an ascending-height table that comes to the same
/// thing, and it is worth keeping because it says the bands are ordered on purpose.
pub(crate) fn flagpole_score(top_blocks: f32) -> u32 {
    let mut score = FLAG_SCORES[0];
    for (i, threshold) in FLAG_HEIGHTS.iter().enumerate() {
        if top_blocks < *threshold {
            score = FLAG_SCORES[i + 1];
        } else {
            break;
        }
    }
    score
}

/// How many fireworks go up, from the clock reading.
///
/// The rule is genuinely this odd: take the **last digit** of the rounded-up remaining
/// time, and keep it only if it is 1, 3 or 6 — anything else means no fireworks at all
/// (`mario.lua:2953-2957`, whose own comment reads "Who came up with this?").
///
/// Lab levels never get any: they have no clock, and the original suppresses them
/// whenever `portalbackground` is set.
pub(crate) fn firework_count(time_remaining: f32, portal_pack: bool) -> u32 {
    if portal_pack {
        return 0;
    }
    let digit = (time_remaining.ceil().max(0.0) as u32) % 10;
    if matches!(digit, 1 | 3 | 6) { digit } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five bands, checked at their edges. Sliding down to the bottom is worth 100;
    /// only the very top of the pole pays 5000.
    #[test]
    fn the_pole_pays_by_height() {
        assert_eq!(flagpole_score(12.0), 100, "at the foot of the pole");
        assert_eq!(
            flagpole_score(9.8125),
            100,
            "exactly on a threshold is below it"
        );
        assert_eq!(flagpole_score(9.8), 400);
        assert_eq!(flagpole_score(7.0), 800);
        assert_eq!(flagpole_score(5.0), 2000);
        assert_eq!(flagpole_score(2.0), 5000, "the top of the pole");
        assert_eq!(flagpole_score(0.0), 5000, "and anything above it");
    }

    /// Only three digits produce fireworks, and it is the *last* digit that counts —
    /// so 41 seconds left is one firework and 40 is none.
    #[test]
    fn fireworks_come_from_the_last_digit_of_the_clock() {
        assert_eq!(firework_count(41.0, false), 1);
        assert_eq!(firework_count(43.0, false), 3);
        assert_eq!(firework_count(46.0, false), 6);
        for none in [40.0, 42.0, 44.0, 45.0, 47.0, 48.0, 49.0] {
            assert_eq!(firework_count(none, false), 0, "{none} should give none");
        }
        // The clock is rounded *up*: 40.2 counts as 41.
        assert_eq!(firework_count(40.2, false), 1);
        assert_eq!(firework_count(0.0, false), 0);
    }

    #[test]
    fn the_lab_never_gets_fireworks() {
        assert_eq!(firework_count(41.0, true), 0);
    }
}
