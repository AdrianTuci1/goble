//! Trackpad inertia: the glide a precise scroll keeps after the fingers lift.
//!
//! The model is warp-new's momentum scroll, from the winit event loop in
//! `octomusui`: the velocity measured over the flick decays by `MOMENTUM_DECAY`
//! every `MOMENTUM_DECAY_INTERVAL` and each frame applies `velocity * elapsed`,
//! until the velocity is under a subpixel per second.
//!
//! The glide is only started for a gesture that ends. macOS already sends its
//! own momentum as further wheel events after the fingers lift, and winit
//! reports that stream with the same touch phases; the synthetic glide yields
//! to it (see [`ScrollMomentum::track`]), so a trackpad whose platform has no
//! momentum of its own gets inertia and one that has it is not scrolled twice.

use std::time::{Duration, Instant};

use crate::geometry::{vec2f, Vector2F};

/// Where in its gesture a wheel event sits, as the platform reports it. winit
/// has the same three phases for a trackpad's touches and for the momentum it
/// sends after them, which is why inertia has to tell the two apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollPhase {
    /// The fingers landed.
    Began,
    /// The gesture is moving, or a wheel notch the platform reports without a
    /// gesture around it.
    #[default]
    Moved,
    /// The fingers lifted.
    Ended,
}

/// Velocity keeps this fraction of itself every [`MOMENTUM_DECAY_INTERVAL`].
/// Between iOS' "normal" (0.984) and "fast" (0.923) feel.
const MOMENTUM_DECAY: f32 = 0.968;
/// The period the decay above is stated over.
const MOMENTUM_DECAY_INTERVAL: f32 = 0.008;
/// A flick slower than this (points per second) starts no glide.
const MOMENTUM_THRESHOLD: f32 = 50.0;
/// Below this the glide stops: under a point per second is under a pixel.
const MOMENTUM_MIN_VELOCITY: f32 = 1.0;
/// The fastest a glide may start, so a batched or spiky stream cannot fling the
/// content.
const MOMENTUM_MAX_VELOCITY: f32 = 2000.0;
/// Floor on the time between two moves: a stream batched into one wake would
/// otherwise divide by nearly zero and report an enormous velocity.
const MIN_VELOCITY_TIME_DELTA: f32 = 0.004;
/// The longest step one frame may apply. A frame that arrives late (a hidden
/// window, a slow frame) would otherwise carry the motion it missed; a
/// twentieth of a second of it in one step reads as a teleport.
const MAX_GLIDE_STEP: f32 = 0.05;
/// How soon after the fingers lift the platform's own momentum may begin. A
/// gesture that starts later than this is a new touch, whose velocity counts.
const PLATFORM_MOMENTUM_WINDOW: Duration = Duration::from_millis(100);

/// A glide in flight.
#[derive(Clone, Copy, Debug)]
struct Glide {
    velocity: Vector2F,
    last_tick: Instant,
}

/// Whose movement the gesture in flight carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Gesture {
    #[default]
    None,
    /// The user's fingers.
    User,
    /// The platform's own momentum, which winit reports with touch phases.
    Platform,
}

/// Tracks one wheel stream and the glide it leaves behind.
///
/// The platform owns one: it sees every wheel event, which is the only place
/// the gesture's velocity is known, and it is what drives the frames the glide
/// runs on.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollMomentum {
    gesture: Gesture,
    velocity: Vector2F,
    measured_at: Option<Instant>,
    /// When the user's last gesture ended. A gesture that begins just after is
    /// the platform's momentum, not a new touch.
    released_at: Option<Instant>,
    glide: Option<Glide>,
}

impl ScrollMomentum {
    /// Feed one wheel event. Returns whether a glide started, which is a frame
    /// the caller owes even when the event itself moved nothing.
    pub fn track(
        &mut self,
        delta: Vector2F,
        precise: bool,
        phase: ScrollPhase,
        now: Instant,
    ) -> bool {
        if !precise {
            // A wheel notch is one discrete step: no velocity to carry, and it
            // stops a glide it lands in.
            *self = Self::default();
            return false;
        }
        match phase {
            ScrollPhase::Began => {
                // The release this gesture follows is the tell: the platform's
                // momentum starts right after the fingers lift, so a gesture
                // that begins within the window is its stream and must not be
                // measured as another flick. A later one is a new touch.
                self.gesture = match self.released_at {
                    Some(released)
                        if now.saturating_duration_since(released) <= PLATFORM_MOMENTUM_WINDOW =>
                    {
                        Gesture::Platform
                    }
                    _ => Gesture::User,
                };
                self.glide = None;
                self.velocity = Vector2F::zero();
                self.measured_at = None;
                false
            }
            ScrollPhase::Moved => {
                // A glide yields to the next delta, whoever sends it.
                self.glide = None;
                if self.gesture == Gesture::Platform {
                    return false;
                }
                self.gesture = Gesture::User;
                let elapsed = self
                    .measured_at
                    .map(|at| now.saturating_duration_since(at).as_secs_f32())
                    .unwrap_or(MOMENTUM_DECAY_INTERVAL)
                    .max(MIN_VELOCITY_TIME_DELTA);
                self.velocity = delta / elapsed;
                self.measured_at = Some(now);
                false
            }
            ScrollPhase::Ended => {
                let user = self.gesture != Gesture::Platform;
                let starts = user && self.velocity.length() >= MOMENTUM_THRESHOLD;
                self.glide = starts.then(|| Glide {
                    velocity: vec2f(
                        self.velocity
                            .x
                            .clamp(-MOMENTUM_MAX_VELOCITY, MOMENTUM_MAX_VELOCITY),
                        self.velocity
                            .y
                            .clamp(-MOMENTUM_MAX_VELOCITY, MOMENTUM_MAX_VELOCITY),
                    ),
                    last_tick: now,
                });
                if user {
                    // Only the user's own release marks where the platform's
                    // momentum may begin; the end of that momentum is a release
                    // again, not a new flick.
                    self.released_at = Some(now);
                }
                self.gesture = Gesture::None;
                self.velocity = Vector2F::zero();
                self.measured_at = None;
                self.glide.is_some()
            }
        }
    }

    /// Advance the glide to `now`, returning how far the content should move
    /// this frame. `None` when no glide is in flight or the velocity has
    /// decayed to where it no longer reads as motion.
    pub fn advance(&mut self, now: Instant) -> Option<Vector2F> {
        let mut glide = self.glide?;
        let elapsed = now.saturating_duration_since(glide.last_tick).as_secs_f32();
        if elapsed <= 0.0 {
            return None;
        }
        let elapsed = elapsed.min(MAX_GLIDE_STEP);
        glide.last_tick = now;
        glide.velocity *= MOMENTUM_DECAY.powf(elapsed / MOMENTUM_DECAY_INTERVAL);
        if glide.velocity.length() < MOMENTUM_MIN_VELOCITY {
            self.glide = None;
            return None;
        }
        self.glide = Some(glide);
        Some(glide.velocity * elapsed)
    }

    /// Drop the glide: nothing under the pointer can take it, or it has run out
    /// of content to travel.
    pub fn stop(&mut self) {
        self.glide = None;
    }

    /// Whether a glide still needs frames.
    pub fn is_gliding(&self) -> bool {
        self.glide.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// A flick: the fingers land, move `delta` every 8ms, then lift.
    fn flick(momentum: &mut ScrollMomentum, base: Instant, per_event: f32) -> bool {
        let mut started = momentum.track(Vector2F::zero(), true, ScrollPhase::Began, base);
        for step in 1..=4u64 {
            let moved = momentum.track(
                vec2f(0.0, per_event),
                true,
                ScrollPhase::Moved,
                at(base, step * 8),
            );
            started = started || moved;
        }
        let ended = momentum.track(Vector2F::zero(), true, ScrollPhase::Ended, at(base, 40));
        started || ended
    }

    #[test]
    fn a_flick_glides_after_the_fingers_lift_and_stops() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(
            flick(&mut momentum, base, 8.0),
            "a fast flick starts a glide"
        );
        assert!(momentum.is_gliding());

        let mut travelled = 0.0;
        let mut previous = f32::INFINITY;
        let mut frame = at(base, 56);
        let mut frames = 0;
        while let Some(delta) = momentum.advance(frame) {
            assert!(
                delta.y > 0.0,
                "the glide carries the flick's direction: {}",
                delta.y
            );
            assert!(
                delta.y < previous,
                "every frame travels less than the one before: {previous} then {}",
                delta.y
            );
            previous = delta.y;
            travelled += delta.y;
            frame += Duration::from_millis(16);
            frames += 1;
            assert!(frames < 400, "the glide comes to rest");
        }
        assert!(travelled > 100.0, "the flick carries on: {travelled}");
        assert!(!momentum.is_gliding(), "and then there is nothing to paint");
    }

    #[test]
    fn a_slow_release_starts_nothing() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(!flick(&mut momentum, base, 0.1));
        assert!(!momentum.is_gliding());
    }

    /// A slow release starts no glide, but the platform's own momentum still
    /// follows it, and its stream must not be measured as a flick either.
    #[test]
    fn momentum_after_a_slow_release_is_still_the_platforms() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        momentum.track(Vector2F::zero(), true, ScrollPhase::Began, base);
        momentum.track(vec2f(0.0, 0.1), true, ScrollPhase::Moved, at(base, 8));
        assert!(!momentum.track(Vector2F::zero(), true, ScrollPhase::Ended, at(base, 16)));

        // Two milliseconds later the platform's momentum begins, moving fast.
        assert!(!momentum.track(Vector2F::zero(), true, ScrollPhase::Began, at(base, 18)));
        momentum.track(vec2f(0.0, 8.0), true, ScrollPhase::Moved, at(base, 26));
        assert!(
            !momentum.track(Vector2F::zero(), true, ScrollPhase::Ended, at(base, 60)),
            "the end of the platform's momentum starts no glide of its own"
        );
        assert!(!momentum.is_gliding());
    }

    #[test]
    fn a_wheel_notch_never_glides() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(!momentum.track(vec2f(0.0, 40.0), false, ScrollPhase::Moved, base));
        assert!(!momentum.track(Vector2F::zero(), false, ScrollPhase::Ended, at(base, 8)));
        assert!(!momentum.is_gliding());
    }

    /// macOS sends its own momentum right after the fingers lift, reported with
    /// the same phases. The synthetic glide must step aside rather than scroll
    /// the content twice.
    #[test]
    fn the_platforms_own_momentum_takes_over_from_the_glide() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(flick(&mut momentum, base, 8.0));

        // Momentum begins 2ms after the release.
        assert!(!momentum.track(Vector2F::zero(), true, ScrollPhase::Began, at(base, 42)));
        assert!(!momentum.is_gliding(), "the glide gives way at once");

        // Its deltas are the platform's, so they are not measured as a flick.
        for step in 0..4u64 {
            assert!(!momentum.track(
                vec2f(0.0, 2.0),
                true,
                ScrollPhase::Moved,
                at(base, 50 + step * 8),
            ));
        }
        assert!(
            !momentum.track(Vector2F::zero(), true, ScrollPhase::Ended, at(base, 90),),
            "the end of the platform's momentum starts no glide of its own"
        );
        assert!(!momentum.is_gliding());
    }

    /// A new touch right after a release is the user's, so its flick glides.
    #[test]
    fn a_new_touch_after_the_platforms_window_is_measured_again() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(flick(&mut momentum, base, 8.0));

        let later = at(base, 40 + PLATFORM_MOMENTUM_WINDOW.as_millis() as u64 + 1);
        assert!(flick(&mut momentum, later, 8.0));
        assert!(momentum.is_gliding());
    }

    #[test]
    fn a_late_frame_carries_a_bounded_step() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(flick(&mut momentum, base, 8.0));

        let delta = momentum
            .advance(at(base, 40 + 1_000))
            .expect("the glide is still running");
        assert!(
            delta.y <= MOMENTUM_MAX_VELOCITY * MAX_GLIDE_STEP + 1.0,
            "one late frame cannot teleport the content: {delta:?}"
        );
    }

    #[test]
    fn stopping_drops_the_glide() {
        let base = Instant::now();
        let mut momentum = ScrollMomentum::default();
        assert!(flick(&mut momentum, base, 8.0));
        momentum.stop();
        assert!(!momentum.is_gliding());
        assert!(momentum.advance(at(base, 56)).is_none());
    }
}
