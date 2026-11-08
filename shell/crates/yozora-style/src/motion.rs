use crate::{Fx, Look};

/// How a panel arrives and leaves. Each look has one signature, reused everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrance {
    /// A drop buds out of the bar and retracts back into it.
    Bud,
    /// The panel unfolds from the rule like a folded sheet.
    Unfold,
    /// A scan line draws the panel in steps; it switches off like a tube.
    Scan,
    /// An iris opens from the icon, with a light sweep across the glass.
    Iris,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub entrance: Entrance,
    /// Seconds to open and to close.
    pub open: f32,
    pub close: f32,
    /// Seconds between rows as a panel's content arrives.
    pub row_stagger: f32,
    /// A spring (stiffness, damping) when the look overshoots; None eases without overshoot.
    pub spring: Option<(f32, f32)>,
    /// Steps for a look that moves in frames (Phosphor), 0 for continuous motion.
    pub steps: u32,
    /// Whether the ambient layer (aurora, scanlines, light sweep, glow) runs.
    pub ambient: bool,
}

impl Motion {
    pub fn of(look: Look, fx: Fx) -> Motion {
        let m = match look {
            Look::Tension | Look::Daybreak => Motion { entrance: Entrance::Bud, open: 0.63, close: 0.54, row_stagger: 0.028, spring: Some((260.0, 19.0)), steps: 0, ambient: true },
            Look::Folio => Motion { entrance: Entrance::Unfold, open: 0.45, close: 0.36, row_stagger: 0.040, spring: None, steps: 0, ambient: false },
            Look::Phosphor => Motion { entrance: Entrance::Scan, open: 0.54, close: 0.54, row_stagger: 0.045, spring: None, steps: 9, ambient: true },
            Look::Prism => Motion { entrance: Entrance::Iris, open: 0.70, close: 0.50, row_stagger: 0.024, spring: Some((200.0, 17.0)), steps: 0, ambient: true },
        };
        match fx {
            Fx::Full => m,
            // Calm keeps the arrival, drops the ambient layer and the overshoot.
            Fx::Calm => Motion { ambient: false, spring: None, ..m },
            // Still keeps nothing in motion: panels appear at once.
            Fx::Still => Motion { open: 0.0, close: 0.0, row_stagger: 0.0, spring: None, steps: 0, ambient: false, ..m },
        }
    }
}
