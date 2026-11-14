//! The stories themselves: what each one does to the island, the clock and
//! the mascot at any moment of its timeline.

use crate::ui::play::trigger::Story;

/// The story has a timeline to play. Only ready stories enter the pool
/// (`trigger::pool`), so a mascot with none never plays.
pub fn ready(story: Story) -> bool {
    matches!(story, Story::MalingAngka)
}
