//! Playlist: rotates an `AnimationPlayer` through a shuffled list of modes.
//!
//! A single mode is a length-1 playlist that never switches — behaviourally
//! identical to owning one `AnimationPlayer`. With more than one mode, each
//! plays for `cycle` before a hard cut to the next; the order is a shuffle
//! that reshuffles on wrap with no back-to-back repeat (the "playlist shuffle"
//! feel, à la xscreensaver's `cycle`).
//!
//! It exposes the same surface the event loop already drives on a player
//! (`advance`/`ensure_sized`/`blit_into`/`next_wake`), so
//! the switching stays contained here and `AnimationPlayer` keeps its single
//! job of driving one animation's frame clock.

use super::{AnimConfig, AnimRegistry, AnimationPlayer};
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub struct Playlist {
    /// Valid mode names, length >= 1; reshuffled in place on wrap.
    order: Vec<String>,
    idx: usize,
    cycle: Duration,
    params: AnimConfig,
    background: (f64, f64, f64, f64),
    current: AnimationPlayer,
    last_size: Option<(u32, u32)>,
    /// When the current mode's airtime ends. `None` for a single-mode
    /// playlist (which never switches) and until the first `advance`.
    switch_at: Option<Instant>,
}

impl Playlist {
    /// Build a playlist from `modes`, skipping any that aren't registered
    /// (warned, never fatal — a locker must still lock on a typo). Returns
    /// `None` if none are valid, so the caller falls back to a solid
    /// background. The order is shuffled up front.
    pub fn new(
        modes: &[String],
        cycle: Duration,
        params: AnimConfig,
        background: (f64, f64, f64, f64),
    ) -> Option<Self> {
        use rand::seq::SliceRandom;

        let known: HashSet<String> = AnimRegistry::new().available_modes().into_iter().collect();
        let mut order: Vec<String> = modes
            .iter()
            .filter(|m| {
                let ok = known.contains(*m);
                if !ok {
                    log::warn!("unknown animation mode {m:?}; skipping");
                }
                ok
            })
            .cloned()
            .collect();
        if order.is_empty() {
            return None;
        }
        order.shuffle(&mut rand::rng());

        // order[0] is registered (filtered above), so this is Some.
        let current = AnimationPlayer::new(&order[0], params.clone(), background)?;
        Some(Playlist {
            order,
            idx: 0,
            cycle,
            params,
            background,
            current,
            last_size: None,
            switch_at: None,
        })
    }

    fn multi(&self) -> bool {
        self.order.len() > 1
    }

    /// Advance to the next mode: reshuffle on wrap (avoiding an immediate
    /// repeat), rebuild the player, and restore the current surface size.
    fn switch_to_next(&mut self) {
        use rand::seq::SliceRandom;
        self.idx += 1;
        if self.idx >= self.order.len() {
            let last = self.order[self.order.len() - 1].clone();
            self.order.shuffle(&mut rand::rng());
            if self.order.len() > 1 && self.order[0] == last {
                self.order.swap(0, 1);
            }
            self.idx = 0;
        }
        if let Some(p) =
            AnimationPlayer::new(&self.order[self.idx], self.params.clone(), self.background)
        {
            self.current = p;
            if let Some((w, h)) = self.last_size {
                self.current.ensure_sized(w, h);
            }
        }
        // On the unexpected None (mode was valid at startup) keep the current
        // player rather than blanking the screen.
    }

    pub fn advance(&mut self, now: Instant) {
        if self.multi() {
            match self.switch_at {
                None => self.switch_at = Some(now + self.cycle),
                Some(at) if now >= at => {
                    self.switch_to_next();
                    self.switch_at = Some(now + self.cycle);
                }
                Some(_) => {}
            }
        }
        self.current.advance(now);
    }

    pub fn ensure_sized(&mut self, width: u32, height: u32) {
        self.last_size = Some((width, height));
        self.current.ensure_sized(width, height);
    }

    pub fn blit_into(&self, dst: &mut [u8], width: i32, height: i32) -> Result<(), String> {
        self.current.blit_into(dst, width, height)
    }

    /// The earlier of the current mode's next frame and the next mode switch,
    /// so even a static mode (no frame clock) still wakes the loop in time to
    /// rotate.
    pub fn next_wake(&self) -> Option<Instant> {
        match (self.current.next_wake(), self.switch_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 1.0);

    #[test]
    fn skips_unknown_keeps_known() {
        let pl = Playlist::new(
            &["spiral".into(), "definitely-not-a-mode".into()],
            Duration::from_secs(60),
            AnimConfig::default(),
            BG,
        )
        .expect("one valid mode remains");
        assert_eq!(pl.order, vec!["spiral".to_string()]);
    }

    #[test]
    fn none_when_all_unknown() {
        assert!(Playlist::new(
            &["nope".into(), "nada".into()],
            Duration::from_secs(60),
            AnimConfig::default(),
            BG
        )
        .is_none());
    }

    #[test]
    fn single_mode_never_arms_a_switch() {
        let mut pl = Playlist::new(
            &["spiral".into()],
            Duration::from_millis(1),
            AnimConfig::default(),
            BG,
        )
        .unwrap();
        pl.advance(Instant::now());
        assert!(
            pl.switch_at.is_none(),
            "single mode must not schedule a switch"
        );
    }

    #[test]
    fn multi_mode_arms_a_switch() {
        let mut pl = Playlist::new(
            &["spiral".into(), "worm".into()],
            Duration::from_secs(60),
            AnimConfig::default(),
            BG,
        )
        .unwrap();
        pl.advance(Instant::now());
        assert!(pl.switch_at.is_some(), "multi mode must schedule a switch");
    }
}
