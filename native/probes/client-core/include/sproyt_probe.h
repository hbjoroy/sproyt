#ifndef SPROYT_PROBE_H
#define SPROYT_PROBE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
uint32_t sproyt_probe_abi_version(void);
size_t sproyt_probe_response_capacity(void);
/* Valid non-overlapping buffers required. Call on a worker, not a UI thread.
   Reserve response_capacity() before the call: a size query never executes it.
   Integer handles are process-local, never reused, and must be closed.
   Positive result: response bytes; negative: ABI error, see README. */
int32_t sproyt_probe_call(const uint8_t *input, size_t input_len,
                         uint8_t *output, size_t output_cap);
#ifdef __cplusplus
}
#endif
#endif
