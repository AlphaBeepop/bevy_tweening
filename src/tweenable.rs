use bevy::prelude::*;
use std::time::Duration;

use crate::{EaseMethod, Lens, TweeningDirection, TweeningType};

/// Playback state of a [`Tweenable`].
///
/// This is returned by [`Tweenable::tick()`] to allow the caller to execute some logic based on the
/// updated state of the tweenable, like advanding a sequence to its next child tweenable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweenState {
    /// The tweenable is still active, and did not reach its end state yet.
    Active,
    /// Animation reached its end state. The tweenable is idling at its latest time. This can only happen
    /// for [`TweeningType::Once`], since other types loop indefinitely.
    Completed,
}

/// Event raised when a tween completed.
#[derive(Copy, Clone)]
pub struct TweenCompleted {
    /// The [`Entity`] the tween which completed and its animator are attached to.
    pub entity: Entity,
}

/// An animatable entity, either a single [`Tween`] or a collection of them.
pub trait Tweenable<T>: Send + Sync {
    /// Get the total duration of the animation.
    ///
    /// For [`TweeningType::PingPong`], this is the duration of a single way, either from start
    /// to end or back from end to start. The total loop duration start -> end -> start in this
    /// case is the double of the returned value.
    fn duration(&self) -> Duration;

    /// Return `true` if the animation is looping.
    fn is_looping(&self) -> bool;

    /// Set the current animation playback progress.
    ///
    /// See [`progress()`] for details on the meaning.
    ///
    /// [`progress()`]: Tweenable::progress
    fn set_progress(&mut self, progress: f32);

    /// Get the current progress in \[0:1\] (non-looping) or \[0:1\[ (looping) of the animation.
    ///
    /// For looping animations, this reports the progress of the current iteration,
    /// in the current direction:
    /// - [`TweeningType::Loop`] is 0 at start and 1 at end. The exact value 1.0 is never reached,
    ///   since the tweenable loops over to 0.0 immediately.
    /// - [`TweeningType::PingPong`] is 0 at the source endpoint and 1 and the destination one,
    ///   which are respectively the start/end for [`TweeningDirection::Forward`], or the end/start
    ///   for [`TweeningDirection::Backward`]. The exact value 1.0 is never reached, since the tweenable
    ///   loops over to 0.0 immediately when it changes direction at either endpoint.
    fn progress(&self) -> f32;

    /// Tick the animation, advancing it by the given delta time and mutating the given target component or asset.
    ///
    /// This returns [`TweenState::Active`] if the tweenable didn't reach its final state yet (progress < 1.),
    /// or [`TweenState::Completed`] if the tweenable completed this tick. Only non-looping tweenables return
    /// a completed state, since looping ones continue forever.
    ///
    /// Calling this method with a duration of [`Duration::ZERO`] is valid, and updates the target to the current
    /// state of the tweenable without actually modifying the tweenable state. This is useful after certain operations
    /// like [`rewind()`] or [`set_progress()`] whose effect is otherwise only visible on target on next frame.
    ///
    /// [`rewind()`]: Tweenable::rewind
    /// [`set_progress()`]: Tweenable::set_progress
    fn tick(
        &mut self,
        delta: Duration,
        target: &mut T,
        entity: Entity,
        event_writer: &mut EventWriter<TweenCompleted>,
    ) -> TweenState;

    /// Get the number of times this tweenable completed.
    ///
    /// For looping animations, this returns the number of times a single playback was completed. In the
    /// case of [`TweeningType::PingPong`] this corresponds to a playback in a single direction, so tweening
    /// from start to end and back to start counts as two completed times (one forward, one backward).
    fn times_completed(&self) -> u32;

    /// Rewind the animation to its starting state.
    fn rewind(&mut self);
}

impl<T> Tweenable<T> for Box<dyn Tweenable<T> + Send + Sync + 'static> {
    fn duration(&self) -> Duration {
        self.as_ref().duration()
    }
    fn is_looping(&self) -> bool {
        self.as_ref().is_looping()
    }
    fn set_progress(&mut self, progress: f32) {
        self.as_mut().set_progress(progress);
    }
    fn progress(&self) -> f32 {
        self.as_ref().progress()
    }
    fn tick(
        &mut self,
        delta: Duration,
        target: &mut T,
        entity: Entity,
        event_writer: &mut EventWriter<TweenCompleted>,
    ) -> TweenState {
        self.as_mut().tick(delta, target, entity, event_writer)
    }
    fn times_completed(&self) -> u32 {
        self.as_ref().times_completed()
    }
    fn rewind(&mut self) {
        self.as_mut().rewind()
    }
}

/// Trait for boxing a [`Tweenable`] trait object.
pub trait IntoBoxDynTweenable<T> {
    /// Convert the current object into a boxed [`Tweenable`].
    fn into_box_dyn(this: Self) -> Box<dyn Tweenable<T> + Send + Sync + 'static>;
}

impl<T, U: Tweenable<T> + Send + Sync + 'static> IntoBoxDynTweenable<T> for U {
    fn into_box_dyn(this: U) -> Box<dyn Tweenable<T> + Send + Sync + 'static> {
        Box::new(this)
    }
}

/// Single tweening animation instance.
pub struct Tween<T> {
    ease_function: EaseMethod,
    timer: Timer,
    tweening_type: TweeningType,
    direction: TweeningDirection,
    times_completed: u32,
    lens: Box<dyn Lens<T> + Send + Sync + 'static>,
    on_completed: Option<Box<dyn Fn(Entity, &Tween<T>) + Send + Sync + 'static>>,
    raise_event: bool,
}

impl<T: 'static> Tween<T> {
    /// Chain another [`Tweenable`] after this tween, making a [`Sequence`] with the two.
    ///
    /// # Example
    /// ```
    /// # use bevy_tweening::{lens::*, *};
    /// # use bevy::math::*;
    /// # use std::time::Duration;
    /// let tween1 = Tween::new(
    ///     EaseFunction::QuadraticInOut,
    ///     TweeningType::Once,
    ///     Duration::from_secs_f32(1.0),
    ///     TransformPositionLens {
    ///         start: Vec3::ZERO,
    ///         end: Vec3::new(3.5, 0., 0.),
    ///     },
    /// );
    /// let tween2 = Tween::new(
    ///     EaseFunction::QuadraticInOut,
    ///     TweeningType::Once,
    ///     Duration::from_secs_f32(1.0),
    ///     TransformRotationLens {
    ///         start: Quat::IDENTITY,
    ///         end: Quat::from_rotation_x(90.0_f32.to_radians()),
    ///     },
    /// );
    /// let seq = tween1.then(tween2);
    /// ```
    pub fn then(self, tween: impl Tweenable<T> + Send + Sync + 'static) -> Sequence<T> {
        Sequence::from_single(self).then(tween)
    }
}

impl<T> Tween<T> {
    /// Create a new tween animation.
    ///
    /// # Example
    /// ```
    /// # use bevy_tweening::{lens::*, *};
    /// # use bevy::math::Vec3;
    /// # use std::time::Duration;
    /// let tween = Tween::new(
    ///     EaseFunction::QuadraticInOut,
    ///     TweeningType::Once,
    ///     Duration::from_secs_f32(1.0),
    ///     TransformPositionLens {
    ///         start: Vec3::ZERO,
    ///         end: Vec3::new(3.5, 0., 0.),
    ///     },
    /// );
    /// ```
    pub fn new<L>(
        ease_function: impl Into<EaseMethod>,
        tweening_type: TweeningType,
        duration: Duration,
        lens: L,
    ) -> Self
    where
        L: Lens<T> + Send + Sync + 'static,
    {
        Tween {
            ease_function: ease_function.into(),
            timer: Timer::new(duration, tweening_type != TweeningType::Once),
            tweening_type,
            direction: TweeningDirection::Forward,
            times_completed: 0,
            lens: Box::new(lens),
            on_completed: None,
            raise_event: false,
        }
    }

    /// Enable or disable raising a completed event.
    ///
    /// If enabled, the tween will raise a [`TweenCompleted`] event when the animation completed.
    /// This is similar to the [`set_completed`] callback, but uses Bevy events instead.
    ///
    /// [`set_completed`]: Tween::set_completed
    pub fn with_completed_event(mut self, enabled: bool) -> Self {
        self.raise_event = enabled;
        self
    }

    /// The current animation direction.
    ///
    /// See [`TweeningDirection`] for details.
    pub fn direction(&self) -> TweeningDirection {
        self.direction
    }

    /// Set a callback invoked when the animation completed.
    ///
    /// The callback when invoked receives as parameters the [`Entity`] on which the target and the
    /// animator are, as well as a reference to the current [`Tween`].
    ///
    /// Only non-looping tweenables can complete.
    pub fn set_completed<C>(&mut self, callback: C)
    where
        C: Fn(Entity, &Tween<T>) + Send + Sync + 'static,
    {
        self.on_completed = Some(Box::new(callback));
    }

    /// Clear the callback invoked when the animation completed.
    pub fn clear_completed(&mut self) {
        self.on_completed = None;
    }

    /// Enable or disable raising a completed event.
    ///
    /// If enabled, the tween will raise a [`TweenCompleted`] event when the animation completed.
    /// This is similar to the [`set_completed`] callback, but uses Bevy events instead.
    ///
    /// [`set_completed`]: Tween::set_completed
    pub fn set_completed_event(&mut self, enabled: bool) {
        self.raise_event = enabled;
    }
}

impl<T> Tweenable<T> for Tween<T> {
    fn duration(&self) -> Duration {
        self.timer.duration()
    }

    fn is_looping(&self) -> bool {
        self.tweening_type != TweeningType::Once
    }

    fn set_progress(&mut self, progress: f32) {
        self.timer.set_elapsed(Duration::from_secs_f64(
            self.timer.duration().as_secs_f64() * progress as f64,
        ));
        // set_elapsed() does not update finished() etc. which we rely on
        self.timer.tick(Duration::ZERO);
    }

    fn progress(&self) -> f32 {
        match self.direction {
            TweeningDirection::Forward => self.timer.percent(),
            TweeningDirection::Backward => self.timer.percent_left(),
        }
    }

    fn tick(
        &mut self,
        delta: Duration,
        target: &mut T,
        entity: Entity,
        event_writer: &mut EventWriter<TweenCompleted>,
    ) -> TweenState {
        if !self.is_looping() && self.timer.finished() {
            return TweenState::Completed;
        }

        let mut state = TweenState::Active;

        // Tick the timer to update the animation time
        self.timer.tick(delta);

        // Toggle direction immediately, so self.progress() returns the correct ratio
        if self.timer.just_finished() && self.tweening_type == TweeningType::PingPong {
            self.direction = !self.direction;
        }

        // Apply the lens, even if the animation finished, to ensure the state is consistent
        let progress = self.progress();
        let factor = self.ease_function.sample(progress);
        self.lens.lerp(target, factor);

        if self.timer.just_finished() {
            if self.tweening_type == TweeningType::Once {
                state = TweenState::Completed;
            }

            // Timer::times_finished() returns the number of finished times since last tick only
            self.times_completed += self.timer.times_finished();

            if self.raise_event {
                event_writer.send(TweenCompleted { entity });
            }
            if let Some(cb) = &self.on_completed {
                cb(entity, &self);
            }
        }

        state
    }

    fn times_completed(&self) -> u32 {
        self.times_completed
    }

    fn rewind(&mut self) {
        self.timer.reset();
        self.times_completed = 0;
    }
}

/// A sequence of tweens played back in order one after the other.
pub struct Sequence<T> {
    tweens: Vec<Box<dyn Tweenable<T> + Send + Sync + 'static>>,
    index: usize,
    duration: Duration,
    time: Duration,
    times_completed: u32,
}

impl<T> Sequence<T> {
    /// Create a new sequence of tweens.
    ///
    /// This method panics if the input collection is empty.
    pub fn new(items: impl IntoIterator<Item = impl IntoBoxDynTweenable<T>>) -> Self {
        let tweens: Vec<_> = items
            .into_iter()
            .map(IntoBoxDynTweenable::into_box_dyn)
            .collect();
        assert!(!tweens.is_empty());
        let duration = tweens.iter().map(|t| t.duration()).sum();
        Sequence {
            tweens,
            index: 0,
            duration,
            time: Duration::from_secs(0),
            times_completed: 0,
        }
    }

    /// Create a new sequence containing a single tween.
    pub fn from_single(tween: impl Tweenable<T> + Send + Sync + 'static) -> Self {
        let duration = tween.duration();
        Sequence {
            tweens: vec![Box::new(tween)],
            index: 0,
            duration,
            time: Duration::from_secs(0),
            times_completed: 0,
        }
    }

    /// Append a [`Tweenable`] to this sequence.
    pub fn then(mut self, tween: impl Tweenable<T> + Send + Sync + 'static) -> Self {
        self.duration += tween.duration();
        self.tweens.push(Box::new(tween));
        self
    }

    /// Index of the current active tween in the sequence.
    pub fn index(&self) -> usize {
        self.index.min(self.tweens.len() - 1)
    }

    /// Get the current active tween in the sequence.
    pub fn current(&self) -> &dyn Tweenable<T> {
        self.tweens[self.index()].as_ref()
    }
}

impl<T> Tweenable<T> for Sequence<T> {
    fn duration(&self) -> Duration {
        self.duration
    }

    fn is_looping(&self) -> bool {
        false // TODO - implement looping sequences...
    }

    fn set_progress(&mut self, progress: f32) {
        let progress = progress.max(0.);
        self.times_completed = progress as u32;
        let progress = if self.is_looping() {
            progress.fract()
        } else {
            progress.min(1.)
        };

        // Set the total sequence progress
        let total_elapsed_secs = self.duration().as_secs_f64() * progress as f64;
        self.time = Duration::from_secs_f64(total_elapsed_secs);

        // Find which tween is active in the sequence
        let mut accum_duration = 0.;
        for index in 0..self.tweens.len() {
            let tween = &mut self.tweens[index];
            let tween_duration = tween.duration().as_secs_f64();
            if total_elapsed_secs < accum_duration + tween_duration {
                self.index = index;
                let local_duration = total_elapsed_secs - accum_duration;
                tween.set_progress((local_duration / tween_duration) as f32);
                // TODO?? set progress of other tweens after that one to 0. ??
                return;
            }
            tween.set_progress(1.); // ?? to prepare for next loop/rewind?
            accum_duration += tween_duration;
        }

        // None found; sequence ended
        self.index = self.tweens.len();
    }

    fn progress(&self) -> f32 {
        self.time.as_secs_f32() / self.duration.as_secs_f32()
    }

    fn tick(
        &mut self,
        delta: Duration,
        target: &mut T,
        entity: Entity,
        event_writer: &mut EventWriter<TweenCompleted>,
    ) -> TweenState {
        if self.index < self.tweens.len() {
            let mut state = TweenState::Active;
            self.time = (self.time + delta).min(self.duration);
            let tween = &mut self.tweens[self.index];
            let tween_state = tween.tick(delta, target, entity, event_writer);
            if tween_state == TweenState::Completed {
                tween.rewind();
                self.index += 1;
                if self.index >= self.tweens.len() {
                    state = TweenState::Completed;
                    self.times_completed = 1;
                }
            }
            state
        } else {
            TweenState::Completed
        }
    }

    fn times_completed(&self) -> u32 {
        self.times_completed
    }

    fn rewind(&mut self) {
        self.time = Duration::from_secs(0);
        self.index = 0;
        self.times_completed = 0;
        for tween in &mut self.tweens {
            // or only first?
            tween.rewind();
        }
    }
}

/// A collection of [`Tweenable`] executing in parallel.
pub struct Tracks<T> {
    tracks: Vec<Box<dyn Tweenable<T> + Send + Sync + 'static>>,
    duration: Duration,
    time: Duration,
    times_completed: u32,
}

impl<T> Tracks<T> {
    /// Create a new [`Tracks`] from an iterator over a collection of [`Tweenable`].
    pub fn new(items: impl IntoIterator<Item = impl IntoBoxDynTweenable<T>>) -> Self {
        let tracks: Vec<_> = items
            .into_iter()
            .map(IntoBoxDynTweenable::into_box_dyn)
            .collect();
        let duration = tracks.iter().map(|t| t.duration()).max().unwrap();
        Tracks {
            tracks,
            duration,
            time: Duration::from_secs(0),
            times_completed: 0,
        }
    }
}

impl<T> Tweenable<T> for Tracks<T> {
    fn duration(&self) -> Duration {
        self.duration
    }

    fn is_looping(&self) -> bool {
        false // TODO - implement looping tracks...
    }

    fn set_progress(&mut self, progress: f32) {
        let progress = progress.max(0.);
        self.times_completed = progress as u32;
        let progress = progress.fract();
        self.time = Duration::from_secs_f64(self.duration().as_secs_f64() * progress as f64);
    }

    fn progress(&self) -> f32 {
        self.time.as_secs_f32() / self.duration.as_secs_f32()
    }

    fn tick(
        &mut self,
        delta: Duration,
        target: &mut T,
        entity: Entity,
        event_writer: &mut EventWriter<TweenCompleted>,
    ) -> TweenState {
        self.time = (self.time + delta).min(self.duration);
        let mut any_active = false;
        for tweenable in &mut self.tracks {
            let state = tweenable.tick(delta, target, entity, event_writer);
            any_active = any_active || (state == TweenState::Active);
        }
        if any_active {
            TweenState::Active
        } else {
            TweenState::Completed
        }
    }

    fn times_completed(&self) -> u32 {
        self.times_completed
    }

    fn rewind(&mut self) {
        self.time = Duration::from_secs(0);
        self.times_completed = 0;
        for tween in &mut self.tracks {
            tween.rewind();
        }
    }
}

/// A time delay that doesn't animate anything.
///
/// This is generally useful for combining with other tweenables into sequences and tracks,
/// for example to delay the start of a tween in a track relative to another track. The `menu`
/// example (`examples/menu.rs`) uses this technique to delay the animation of its buttons.
pub struct Delay {
    timer: Timer,
}

impl Delay {
    /// Create a new [`Delay`] with a given duration.
    pub fn new(duration: Duration) -> Self {
        Delay {
            timer: Timer::new(duration, false),
        }
    }

    /// Chain another [`Tweenable`] after this tween, making a sequence with the two.
    pub fn then<T>(self, tween: impl Tweenable<T> + Send + Sync + 'static) -> Sequence<T> {
        Sequence::from_single(self).then(tween)
    }
}

impl<T> Tweenable<T> for Delay {
    fn duration(&self) -> Duration {
        self.timer.duration()
    }

    fn is_looping(&self) -> bool {
        false
    }

    fn set_progress(&mut self, progress: f32) {
        self.timer.set_elapsed(Duration::from_secs_f64(
            self.timer.duration().as_secs_f64() * progress as f64,
        ));
        // set_elapsed() does not update finished() etc. which we rely on
        self.timer.tick(Duration::ZERO);
    }

    fn progress(&self) -> f32 {
        self.timer.percent()
    }

    fn tick(
        &mut self,
        delta: Duration,
        _target: &mut T,
        _entity: Entity,
        _event_writer: &mut EventWriter<TweenCompleted>,
    ) -> TweenState {
        self.timer.tick(delta);
        if self.timer.finished() {
            TweenState::Completed
        } else {
            TweenState::Active
        }
    }

    fn times_completed(&self) -> u32 {
        if self.timer.finished() {
            1
        } else {
            0
        }
    }

    fn rewind(&mut self) {
        self.timer.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lens::*;
    use bevy::ecs::{event::Events, system::SystemState};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Utility to compare floating-point values with a tolerance.
    fn abs_diff_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[derive(Default, Copy, Clone)]
    struct CallbackMonitor {
        invoke_count: u64,
        last_reported_count: u32,
    }

    /// Test ticking of a single tween in isolation.
    #[test]
    fn tween_tick() {
        for tweening_type in &[
            TweeningType::Once,
            TweeningType::Loop,
            TweeningType::PingPong,
        ] {
            println!("TweeningType: {:?}", tweening_type);

            // Create a linear tween over 1 second
            let mut tween = Tween::new(
                EaseMethod::Linear,
                *tweening_type,
                Duration::from_secs_f32(1.0),
                TransformPositionLens {
                    start: Vec3::ZERO,
                    end: Vec3::ONE,
                },
            );

            let dummy_entity = Entity::from_raw(42);

            // Register callbacks to count started/ended events
            let callback_monitor = Arc::new(Mutex::new(CallbackMonitor::default()));
            let cb_mon_ptr = Arc::clone(&callback_monitor);
            tween.set_completed(move |entity, tween| {
                assert_eq!(dummy_entity, entity);
                let mut cb_mon = cb_mon_ptr.lock().unwrap();
                cb_mon.invoke_count += 1;
                cb_mon.last_reported_count = tween.times_completed();
            });
            assert_eq!(callback_monitor.lock().unwrap().invoke_count, 0);

            // Dummy world and event writer
            let mut world = World::new();
            world.insert_resource(Events::<TweenCompleted>::default());
            let mut system_state: SystemState<EventWriter<TweenCompleted>> =
                SystemState::new(&mut world);
            let mut event_writer = system_state.get_mut(&mut world);

            // Loop over 2.2 seconds, so greater than one ping-pong loop
            let mut transform = Transform::default();
            let tick_duration = Duration::from_secs_f32(0.2);
            for i in 1..=11 {
                // Calculate expected values
                let (progress, times_completed, direction, expected_state) = match tweening_type {
                    TweeningType::Once => {
                        let progress = (i as f32 * 0.2).min(1.0);
                        let times_completed = if i >= 5 { 1 } else { 0 };
                        let state = if i < 5 {
                            TweenState::Active
                        } else {
                            TweenState::Completed
                        };
                        (progress, times_completed, TweeningDirection::Forward, state)
                    }
                    TweeningType::Loop => {
                        let progress = (i as f32 * 0.2).fract();
                        let times_completed = i / 5;
                        (
                            progress,
                            times_completed,
                            TweeningDirection::Forward,
                            TweenState::Active,
                        )
                    }
                    TweeningType::PingPong => {
                        let i10 = i % 10;
                        let progress = if i10 >= 5 {
                            (10 - i10) as f32 * 0.2
                        } else {
                            i10 as f32 * 0.2
                        };
                        let times_completed = i / 5;
                        let direction = if i10 >= 5 {
                            TweeningDirection::Backward
                        } else {
                            TweeningDirection::Forward
                        };
                        (progress, times_completed, direction, TweenState::Active)
                    }
                };
                println!(
                    "Expected: progress={} times_completed={} direction={:?} state={:?}",
                    progress, times_completed, direction, expected_state
                );

                // Tick the tween
                let actual_state = tween.tick(
                    tick_duration,
                    &mut transform,
                    dummy_entity,
                    &mut event_writer,
                );

                // Check actual values
                assert_eq!(tween.direction(), direction);
                assert_eq!(tween.is_looping(), *tweening_type != TweeningType::Once);
                assert_eq!(actual_state, expected_state);
                assert!(abs_diff_eq(tween.progress(), progress, 1e-5));
                assert_eq!(tween.times_completed(), times_completed);
                assert!(transform
                    .translation
                    .abs_diff_eq(Vec3::splat(progress), 1e-5));
                assert!(transform.rotation.abs_diff_eq(Quat::IDENTITY, 1e-5));
                let cb_mon = callback_monitor.lock().unwrap();
                assert_eq!(cb_mon.invoke_count, times_completed as u64);
                assert_eq!(cb_mon.last_reported_count, times_completed);
            }

            // Rewind
            tween.rewind();
            assert_eq!(tween.direction(), TweeningDirection::Forward);
            assert_eq!(tween.is_looping(), *tweening_type != TweeningType::Once);
            assert!(abs_diff_eq(tween.progress(), 0., 1e-5));
            assert_eq!(tween.times_completed(), 0);

            // Dummy tick to update target
            let actual_state = tween.tick(
                Duration::ZERO,
                &mut transform,
                Entity::from_raw(0),
                &mut event_writer,
            );
            assert_eq!(actual_state, TweenState::Active);
            assert!(transform.translation.abs_diff_eq(Vec3::ZERO, 1e-5));
            assert!(transform.rotation.abs_diff_eq(Quat::IDENTITY, 1e-5));
        }
    }

    /// Test ticking a sequence of tweens.
    #[test]
    fn seq_tick() {
        let tween1 = Tween::new(
            EaseMethod::Linear,
            TweeningType::Once,
            Duration::from_secs_f32(1.0),
            TransformPositionLens {
                start: Vec3::ZERO,
                end: Vec3::ONE,
            },
        );
        let tween2 = Tween::new(
            EaseMethod::Linear,
            TweeningType::Once,
            Duration::from_secs_f32(1.0),
            TransformRotationLens {
                start: Quat::IDENTITY,
                end: Quat::from_rotation_x(180_f32.to_radians()),
            },
        );
        let mut seq = tween1.then(tween2);
        let mut transform = Transform::default();

        // Dummy world and event writer
        let mut world = World::new();
        world.insert_resource(Events::<TweenCompleted>::default());
        let mut system_state: SystemState<EventWriter<TweenCompleted>> =
            SystemState::new(&mut world);
        let mut event_writer = system_state.get_mut(&mut world);

        for i in 1..=16 {
            let state = seq.tick(
                Duration::from_secs_f32(0.2),
                &mut transform,
                Entity::from_raw(0),
                &mut event_writer,
            );
            if i < 5 {
                assert_eq!(state, TweenState::Active);
                let r = i as f32 * 0.2;
                assert_eq!(transform, Transform::from_translation(Vec3::splat(r)));
            } else if i < 10 {
                assert_eq!(state, TweenState::Active);
                let alpha_deg = (36 * (i - 5)) as f32;
                assert!(transform.translation.abs_diff_eq(Vec3::splat(1.), 1e-5));
                assert!(transform
                    .rotation
                    .abs_diff_eq(Quat::from_rotation_x(alpha_deg.to_radians()), 1e-5));
            } else {
                assert_eq!(state, TweenState::Completed);
                assert!(transform.translation.abs_diff_eq(Vec3::splat(1.), 1e-5));
                assert!(transform
                    .rotation
                    .abs_diff_eq(Quat::from_rotation_x(180_f32.to_radians()), 1e-5));
            }
        }
    }

    /// Test ticking parallel tracks of tweens.
    #[test]
    fn tracks_tick() {
        let tween1 = Tween::new(
            EaseMethod::Linear,
            TweeningType::Once,
            Duration::from_secs_f32(1.0),
            TransformPositionLens {
                start: Vec3::ZERO,
                end: Vec3::ONE,
            },
        );
        let tween2 = Tween::new(
            EaseMethod::Linear,
            TweeningType::Once,
            Duration::from_secs_f32(0.8), // shorter
            TransformRotationLens {
                start: Quat::IDENTITY,
                end: Quat::from_rotation_x(180_f32.to_radians()),
            },
        );
        let mut tracks = Tracks::new([tween1, tween2]);
        let mut transform = Transform::default();

        // Dummy world and event writer
        let mut world = World::new();
        world.insert_resource(Events::<TweenCompleted>::default());
        let mut system_state: SystemState<EventWriter<TweenCompleted>> =
            SystemState::new(&mut world);
        let mut event_writer = system_state.get_mut(&mut world);

        for i in 1..=6 {
            let state = tracks.tick(
                Duration::from_secs_f32(0.2),
                &mut transform,
                Entity::from_raw(0),
                &mut event_writer,
            );
            if i < 5 {
                assert_eq!(state, TweenState::Active);
                let r = i as f32 * 0.2;
                let alpha_deg = (45 * i) as f32;
                assert!(transform.translation.abs_diff_eq(Vec3::splat(r), 1e-5));
                assert!(transform
                    .rotation
                    .abs_diff_eq(Quat::from_rotation_x(alpha_deg.to_radians()), 1e-5));
            } else {
                assert_eq!(state, TweenState::Completed);
                assert!(transform.translation.abs_diff_eq(Vec3::splat(1.), 1e-5));
                assert!(transform
                    .rotation
                    .abs_diff_eq(Quat::from_rotation_x(180_f32.to_radians()), 1e-5));
            }
        }
    }
}