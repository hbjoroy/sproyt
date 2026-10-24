//! Explicit diagnostic hooks; absent from the ordinary native library.
use crate::store::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    AfterInsert,
    BeforeCommit,
    AfterCommit,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Error,
    #[cfg(feature = "fault-injection")]
    Pause,
}

pub struct Fault {
    pub site: Site,
    pub mode: Mode,
}

pub fn hit(fault: Option<&Fault>, site: Site, _payload: &str) -> Result<()> {
    let Some(fault) = fault.filter(|fault| fault.site == site) else {
        return Ok(());
    };
    match fault.mode {
        Mode::Error => Err(if site == Site::AfterCommit {
            "injected_unknown_outcome"
        } else {
            "injected_failure"
        }),
        #[cfg(feature = "fault-injection")]
        Mode::Pause => {
            use std::io::Write;
            println!(
                "PROBE_FAULT {}",
                serde_json::json!({"site":site,"payload":_payload})
            );
            std::io::stdout()
                .flush()
                .map_err(|_| "diagnostic_output_failed")?;
            // The parent must receive this marker before killing and reaping
            // the child. Only the explicit diagnostic build can pause here.
            loop {
                std::thread::park();
            }
        }
    }
}
