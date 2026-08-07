/*
 * C ABI of mcp-gateway-agent-ffi.
 *
 * Kept by hand rather than generated, because it is four functions and a
 * callback and a generator would be a build dependency for no benefit. It must
 * stay in step with ../../../ffi/src/lib.rs.
 *
 * Contract, in short:
 *   - Every char* returned by this library is owned by the caller and must be
 *     released with mcpga_string_free.
 *   - Strings passed in are borrowed for the duration of the call.
 *   - The event callback is invoked from a single background thread, never
 *     concurrently with itself, and never on the main thread.
 *   - mcpga_command blocks the calling thread until the command completes.
 */
#ifndef MCP_GATEWAY_AGENT_H
#define MCP_GATEWAY_AGENT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Receives one JSON event batch. `ctx` is whatever was passed to mcpga_start. */
typedef void (*McpgaEventCallback)(void *ctx, const char *event_json);

/**
 * Start the agent: load the config, bring up the local backends, and connect
 * the tunnel. Returns 0 on success.
 *
 * `config_path` may be NULL for the default (~/.mcp-gateway-agent/config.toml).
 * `ctx` must stay valid until mcpga_shutdown returns.
 */
int32_t mcpga_start(const char *config_path, McpgaEventCallback callback, void *ctx);

/** Run one command. Returns a JSON envelope: {"ok":true,"data":…} or {"ok":false,"error":…}. */
char *mcpga_command(const char *request_json);

/** Release a string returned by this library. */
void mcpga_string_free(char *ptr);

/** Stop the local backends and drop the tunnel. */
void mcpga_shutdown(void);

/** Version of the agent core. */
char *mcpga_version(void);

#ifdef __cplusplus
}
#endif

#endif /* MCP_GATEWAY_AGENT_H */
