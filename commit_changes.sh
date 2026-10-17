#!/bin/sh
cd /workspace
git add -A
git commit -m "Implement router active-model discovery and startup shutdown

sav: Add loaded_model() router mode handling that queries /models to
find actually loaded models rather than using the router alias. Add
identity validation (nonblank, 256-byte bound, no control chars) and
shared deadline across both metadata GETs. Add test coverage for
router/standalone scenarios including malformed catalogs and
cancellation.

skott: Wire loaded_model() into model selection and add shutdown()
method to SessionHandle that cancels active providers without starting
queued prompts. Add startup.rs module for cleaner init flow.

Both components pass all tests with clean release builds."