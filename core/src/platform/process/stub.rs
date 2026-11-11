//! Stub process discovery for untested platforms.

use super::ProcessDiscoveryReport;
use crate::error::{Error, Result};
use crate::paths::Paths;

pub fn scan_processes(_p: &Paths) -> Result<ProcessDiscoveryReport> {
    Err(Error::Msg(
        "native process discovery is not supported on this platform".to_string(),
    ))
}
