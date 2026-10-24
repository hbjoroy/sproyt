//! Local-only N02 interoperability probe, not a shipping native client SDK.
mod store;

use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    panic::AssertUnwindSafe,
    sync::{Mutex, OnceLock},
};
use store::{Result, Store};

pub const INPUT_LIMIT: usize = 16 * 1024;
pub const RESPONSE_CAPACITY: usize = 256 * 1024;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Open {
        database: String,
        account: String,
    },
    Close {
        handle: u64,
    },
    Snapshot {
        handle: u64,
    },
    SaveDraft {
        handle: u64,
        generation: u64,
        channel: String,
        body: String,
    },
    Enqueue {
        handle: u64,
        generation: u64,
        request_id: String,
        channel: String,
        body: String,
    },
    Delivery {
        handle: u64,
        generation: u64,
        request_id: String,
        action: String,
    },
    ApplyMessage {
        handle: u64,
        generation: u64,
        message_id: String,
        revision: u64,
        body: String,
        deleted: bool,
    },
    Invalidate {
        handle: u64,
        generation: u64,
    },
}

#[derive(Default)]
struct Registry {
    next: u64,
    stores: BTreeMap<u64, Store>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn execute(request: Request) -> Result<Value> {
    let mut registry = REGISTRY
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "probe_poisoned")?;
    match request {
        Request::Open { database, account } => {
            if registry.stores.len() >= 16 {
                return Err("handle_capacity");
            }
            let store = Store::open(&database, &account)?;
            let handle = registry.next.checked_add(1).ok_or("handle_capacity")?;
            registry.next = handle;
            registry.stores.insert(handle, store);
            Ok(json!({"handle":handle}))
        }
        Request::Close { handle } => {
            registry.stores.remove(&handle).ok_or("invalid_handle")?;
            Ok(json!({}))
        }
        request => {
            let handle = match &request {
                Request::Snapshot { handle }
                | Request::SaveDraft { handle, .. }
                | Request::Enqueue { handle, .. }
                | Request::Delivery { handle, .. }
                | Request::ApplyMessage { handle, .. }
                | Request::Invalidate { handle, .. } => *handle,
                _ => unreachable!(),
            };
            let store = registry.stores.get_mut(&handle).ok_or("invalid_handle")?;
            match request {
                Request::Snapshot { .. } => return store.snapshot(),
                Request::SaveDraft {
                    generation,
                    channel,
                    body,
                    ..
                } => store.save_draft(generation, &channel, &body)?,
                Request::Enqueue {
                    generation,
                    request_id,
                    channel,
                    body,
                    ..
                } => store.enqueue(generation, &request_id, &channel, &body)?,
                Request::Delivery {
                    generation,
                    request_id,
                    action,
                    ..
                } => store.delivery(generation, &request_id, &action)?,
                Request::ApplyMessage {
                    generation,
                    message_id,
                    revision,
                    body,
                    deleted,
                    ..
                } => store.apply_message(generation, &message_id, revision, &body, deleted)?,
                Request::Invalidate { generation, .. } => store.invalidate(generation)?,
                _ => unreachable!(),
            }
            Ok(json!({}))
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sproyt_probe_abi_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn sproyt_probe_response_capacity() -> usize {
    RESPONSE_CAPACITY
}

/// Execute once using bounded, caller-owned UTF-8 buffers. Positive return is
/// the exact response length (no NUL terminator). JSON errors have `ok:false`.
/// -1: invalid/null arguments; -2: output too small; -3: input too large;
/// -4: internal panic. Argument failures run no command. No size-probe retry.
///
/// # Safety
/// `input` must refer to `input_len` readable bytes and `output` to `output_cap`
/// writable bytes. Both regions must remain valid, non-overlapping and stable
/// until return. Null pointers are rejected, but arbitrary dangling pointers
/// cannot be validated by the library. Do not call from a UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sproyt_probe_call(
    input: *const u8,
    input_len: usize,
    output: *mut u8,
    output_cap: usize,
) -> i32 {
    if input.is_null() || output.is_null() || input_len == 0 {
        return -1;
    }
    if output_cap < RESPONSE_CAPACITY {
        return -2;
    }
    if input_len > INPUT_LIMIT {
        return -3;
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the foreign caller guarantees the documented buffer contract.
        let input = unsafe { std::slice::from_raw_parts(input, input_len) };
        let result = serde_json::from_slice::<Request>(input)
            .map_err(|_| "invalid_request")
            .and_then(execute);
        let response = match result {
            Ok(value) => json!({"ok":true,"result":value}),
            Err(code) => json!({"ok":false,"error":code}),
        };
        let bytes = serde_json::to_vec(&response).expect("bounded probe response serializes");
        // Mutations return only small acknowledgements. Snapshot is read-only;
        // persisted item/text limits bound its response well below this cap.
        if bytes.len() > RESPONSE_CAPACITY {
            return -4;
        }
        // SAFETY: caller provides at least RESPONSE_CAPACITY nonoverlapping bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
        }
        bytes.len() as i32
    }));
    result.unwrap_or(-4)
}

#[cfg(test)]
mod tests;
