/*
 * SwiftPM requires a C target to have at least one source file, even when the
 * target exists only to publish a header. The implementation of everything
 * declared in include/mcp_gateway_agent.h lives in the Rust staticlib.
 */
#include "include/mcp_gateway_agent.h"
