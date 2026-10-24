//! Optional desktop JVM adapter. No JVM/Android types enter the store or C ABI.
use crate::{INPUT_LIMIT, RESPONSE_CAPACITY};
use jni::{
    JNIEnv,
    objects::{JByteArray, JClass},
    sys::jint,
};
use std::panic::AssertUnwindSafe;

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_sproyt_probe_NativeBridge_abiVersion(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jint {
    crate::sproyt_probe_abi_version() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_sproyt_probe_NativeBridge_responseCapacity(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jint {
    crate::sproyt_probe_response_capacity() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_sproyt_probe_NativeBridge_call(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    input: JByteArray<'_>,
    output: JByteArray<'_>,
) -> jint {
    let mut entered_native = false;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| -> jni::errors::Result<jint> {
        if input.is_null() || output.is_null() {
            return Ok(-1);
        }
        let input_len = env.get_array_length(&input)? as usize;
        let output_len = env.get_array_length(&output)? as usize;
        // Match the ordinary C ABI's validation order; no native operation has
        // run when returning any of these argument errors.
        if input_len == 0 {
            return Ok(-1);
        }
        if output_len < RESPONSE_CAPACITY {
            return Ok(-2);
        }
        if input_len > INPUT_LIMIT {
            return Ok(-3);
        }
        // Copy instead of retaining a JNI critical/pinned array while SQLite
        // may block. All native response storage is allocated before execution.
        let input_copy = env.convert_byte_array(&input)?;
        let mut output_copy = vec![0i8; RESPONSE_CAPACITY];
        entered_native = true;
        // SAFETY: owned input/output arrays are valid, nonoverlapping and remain
        // stable until the synchronous C ABI returns. i8/u8 have equal layout.
        let length = unsafe {
            crate::sproyt_probe_call(
                input_copy.as_ptr(),
                input_copy.len(),
                output_copy.as_mut_ptr().cast(),
                output_copy.len(),
            )
        };
        if length > 0 {
            env.set_byte_array_region(&output, 0, &output_copy[..length as usize])?;
        }
        Ok(length)
    }));
    match result {
        Ok(Ok(length)) => length,
        _ => {
            // Delivery may fail after a committed mutation. Never run it again
            // just to obtain output. If the VM cannot allocate an exception,
            // the negative fallback also means an unknown outcome.
            let _ = env.exception_clear();
            let message = if entered_native {
                "Native probe outcome unknown; recover using the same request ID"
            } else {
                "Native probe bridge failed before execution"
            };
            let _ = env.throw_new("java/lang/IllegalStateException", message);
            -4
        }
    }
}
