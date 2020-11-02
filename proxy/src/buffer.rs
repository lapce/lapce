use serde::{Deserialize, Deserializer, Serialize};
use xi_rope::{
    interval::IntervalBounds, rope::Rope, Cursor, Delta, DeltaBuilder, Interval,
    LinesMetric, RopeDelta, RopeInfo, Transformer,
};

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct BufferId(pub usize);

pub struct Buffer {
    pub id: BufferId,
    pub rope: Rope,
}
