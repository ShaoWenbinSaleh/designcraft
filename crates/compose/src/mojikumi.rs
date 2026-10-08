//! Mojikumi boundary widths. The same values feed line selection and placement.
use crate::shape::Glyph;
use designcraft_doc::{
    Styles,
    mojikumi::{self as rules, Aki, Rules},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub active: bool,
    /// Desired internal gap attached to this glyph's advance.
    pub gap: f64,
    pub stretch: f64,
    pub shrink: f64,
    pub priority: u8,
    pub discrete: bool,
    /// Width changes when this glyph starts / ends a line.
    pub start: f64,
    pub end: f64,
    pub end_aki: Aki,
    pub start_aki: Aki,
    pub at_start: bool,
    pub explicit_before: bool,
    pub explicit_after: bool,
}

fn eligible(g: &Glyph) -> bool {
    g.adv > 0.0
        && !g.locked_advance
        && !g.ch.is_control()
        && (!g.ch.is_whitespace() || g.ch == '\u{3000}')
        && !matches!(g.ch, '\u{e000}'..='\u{e1ff}')
}
fn em(g: &Glyph) -> f64 {
    // Actual scaled em, including vertical and composite-font scaling.
    g.face.units_per_em() * if g.upright { g.sy } else { g.sx }
}

pub fn apply(glyphs: &mut [Glyph], styles: &Styles, name: &str) {
    let Ok(Some(table)) = Rules::resolve(styles, name) else { return };
    // Remove punctuation's built-in blank half-body. The table then contributes
    // the desired spacing once per boundary, rather than adding it twice.
    for g in glyphs.iter_mut().filter(|g| eligible(g)) {
        g.moji.active = true;
        let class = rules::class(g.rendered_char);
        let unit = em(g);
        if g.adv >= unit * 0.8 {
            let before = if g.moji.explicit_before { 0.0 } else { rules::leading(class) * unit };
            let after = if g.moji.explicit_after { 0.0 } else { rules::trailing(class) * unit };
            g.adv = (g.adv - before - after).max(0.0);
            g.dx -= before;
        }
    }
    for i in 0..glyphs.len() {
        let (left, right) = glyphs.split_at_mut(i + 1);
        let Some(g) = left.last_mut() else { continue };
        if !g.moji.active {
            continue;
        }
        let c = rules::class(g.rendered_char);
        let unit = em(g);
        let start = table.pair(if i == 0 { 23 } else { 22 }, c);
        g.moji.start_aki = start;
        g.moji.start = if g.moji.explicit_before { 0.0 } else { start.desired * unit };
        g.moji.end_aki = table.pair(c, 22);
        let mut gap = Aki::default();
        if let Some(next) = right.first().filter(|n| n.moji.active && n.byte != g.byte)
            && !g.moji.explicit_after
            && !next.moji.explicit_before
        {
            gap = table.pair(c, rules::class(next.rendered_char));
        }
        let desired = (gap.desired * unit).max(-g.adv);
        g.moji.gap = desired;
        g.moji.stretch = (gap.max * unit - desired).max(0.0);
        g.moji.shrink = (desired - gap.min * unit).max(0.0).min(g.adv + desired);
        g.moji.priority = gap.priority;
        g.moji.discrete = gap.discrete;
        g.moji.end = if g.moji.explicit_after { 0.0 } else { g.moji.end_aki.desired * unit } - desired;
        g.adv += desired;
    }
}

pub fn edges(line: &mut [Glyph]) {
    if let Some(g) = line.first_mut() {
        g.moji.at_start = true;
        g.adv += g.moji.start;
        g.dx += g.moji.start;
    }
    if let Some(g) = line.iter_mut().rev().find(|g| !crate::breaker::is_forced(g.ch)) {
        g.adv += g.moji.end;
        // A line-end gap is not the internal gap that happened to follow it.
        let unit = em(g);
        let aki = g.moji.end_aki;
        g.moji.stretch = if g.moji.explicit_after { 0.0 } else { (aki.max - aki.desired).max(0.0) * unit };
        g.moji.shrink = if g.moji.explicit_after { 0.0 } else { (aki.desired - aki.min).max(0.0) * unit };
        g.moji.priority = aki.priority;
        g.moji.discrete = aki.discrete;
    }
}

/// Consume spacing in priority order; discrete rules use an entire endpoint.
/// Returns the residual for ordinary word/letter/glyph justification.
pub fn distribute(line: &mut [Glyph], extra: f64, add: &mut [f64]) -> f64 {
    let mut rem = extra.abs();
    let sign = extra.signum();
    for priority in 0..=10 {
        // Start and following gaps share the same priority queue. Moving a
        // leading gap also moves the glyph outline, not just the next glyph.
        for before in [true, false] {
            let properties = |g: &Glyph| {
                if before {
                    let a = g.moji.start_aki;
                    let cap = if !g.moji.at_start || g.moji.explicit_before {
                        0.0
                    } else if sign < 0.0 {
                        (a.desired - a.min).max(0.0) * em(g)
                    } else {
                        (a.max - a.desired).max(0.0) * em(g)
                    };
                    (a.priority, a.discrete, cap)
                } else {
                    (g.moji.priority, g.moji.discrete, if sign < 0.0 { g.moji.shrink } else { g.moji.stretch })
                }
            };
            for (g, a) in line.iter_mut().zip(add.iter_mut()) {
                let (p, discrete, cap) = properties(g);
                if p == priority && discrete && cap <= rem + 1e-9 {
                    *a += sign * cap;
                    if before {
                        g.dx += sign * cap;
                    }
                    rem = (rem - cap).max(0.0);
                }
            }
            let total: f64 = line
                .iter()
                .map(|g| {
                    let (p, discrete, cap) = properties(g);
                    if p == priority && !discrete { cap } else { 0.0 }
                })
                .sum();
            if total > 0.0 {
                let take = rem.min(total);
                for (g, a) in line.iter_mut().zip(add.iter_mut()) {
                    let (p, discrete, cap) = properties(g);
                    if p == priority && !discrete {
                        let amount = sign * take * cap / total;
                        *a += amount;
                        if before {
                            g.dx += amount;
                        }
                    }
                }
                rem -= take;
            }
        }
    }
    sign * rem
}

/// Replace the final internal gap's elasticity by its line-end elasticity.
pub fn end_elastic(g: &Glyph, justify: bool, shrink: bool) -> [f64; 2] {
    let unit = em(g);
    let a = g.moji.end_aki;
    [
        if justify { (if g.moji.explicit_after { 0.0 } else { (a.max - a.desired).max(0.0) * unit }) - g.moji.stretch } else { 0.0 },
        if shrink { (if g.moji.explicit_after { 0.0 } else { (a.desired - a.min).max(0.0) * unit }) - g.moji.shrink } else { 0.0 },
    ]
}

pub fn start_elastic(g: &Glyph, justify: bool, shrink: bool) -> [f64; 2] {
    let a = g.moji.start_aki;
    if g.moji.explicit_before {
        return [0.0; 2];
    }
    [if justify { (a.max - a.desired).max(0.0) * em(g) } else { 0.0 }, if shrink { (a.desired - a.min).max(0.0) * em(g) } else { 0.0 }]
}
