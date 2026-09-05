//! Boot state machine implementation.

use super::BootState;

/// Boot state transition result.
#[derive(Debug)]
pub enum TransitionResult {
    /// State advanced successfully.
    Advanced(BootState),
    /// State was already at or beyond the target.
    AlreadyAt(BootState),
    /// Invalid transition (skipped a state).
    Invalid { from: BootState, to: BootState },
}

/// Attempt to advance to a specific state.
///
/// Returns the result of the transition attempt.
pub fn try_advance(current: BootState, target: BootState) -> TransitionResult {
    if current == target {
        TransitionResult::AlreadyAt(current)
    } else if target as u8 == current as u8 + 1 {
        TransitionResult::Advanced(target)
    } else {
        TransitionResult::Invalid {
            from: current,
            to: target,
        }
    }
}
