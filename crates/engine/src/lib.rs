pub mod anchors;
pub mod command;
pub mod deck;
pub mod engine;
pub mod mixer;
pub mod output;
pub mod record;
pub mod secondary;
pub mod snapshot;

pub use command::{Command, JogMode, LoadRequest, MixerParam};
pub use engine::{create_engine, EngineHandle, RtEngine, Routing, RoutingMode};
pub use record::{record_channel, RecordFrame, RecordShared, RecordTap, RecordTapSlot, RECORD_RING_FRAMES};
pub use secondary::{
    secondary_channel, SecondaryOutputRequest, SecondaryOutputStatus, SecondaryRenderer, SecondaryShared, SecondarySource,
    SecondaryState, SecondaryStats, SecondaryTap, DEFAULT_LEVEL as DEFAULT_SECONDARY_LEVEL, RING_FRAMES as SECONDARY_RING_FRAMES,
};
pub use output::{list_output_devices, ActiveOutput, AudioManager, OutputDeviceInfo, OutputRequest, OutputState, OutputStatus};
pub use snapshot::{
    BeatFxSnapshot, ChannelSnapshot, DeckSnapshot, EngineSnapshot, MasterSnapshot, PadFxSnapshot, ReleaseFxSnapshot, SmartCfxSnapshot,
};

/// Jog resolution used to convert controller ticks into platter revolutions.
pub const JOG_TICKS_PER_REV: f64 = 720.0;
