pub const PROTOCOL_V1: &str = "spm.cloud.v1";
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const HEARTBEAT_INTERVAL_SECONDS: u64 = 15;
pub const HEARTBEAT_TTL_SECONDS: u64 = 45;

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/spm.cloud.v1.rs"));
}

