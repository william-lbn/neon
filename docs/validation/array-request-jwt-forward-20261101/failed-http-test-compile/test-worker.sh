#!/bin/bash
set -u
date -u +%FT%TZ > /out/step-0.start
cargo test --locked --offline -p compute_api --lib bound_array_claims -- --nocapture > /out/step-0.log 2>&1
step_rc=$?
printf "%s\n" "$step_rc" > /out/step-0.rc
if [ "$step_rc" -ne 0 ]; then exit "$step_rc"; fi
date -u +%FT%TZ > /out/step-1.start
cargo test --locked --offline -p compute_tools --lib fixed_resource_proxy_preserves_original_and_closes_connection -- --nocapture > /out/step-1.log 2>&1
step_rc=$?
printf "%s\n" "$step_rc" > /out/step-1.rc
if [ "$step_rc" -ne 0 ]; then exit "$step_rc"; fi
date -u +%FT%TZ > /out/step-2.start
cargo test --locked --offline -p compute_tools --test dsg_http array_authority_routes_require_signed_body_and_separate_scope -- --nocapture > /out/step-2.log 2>&1
step_rc=$?
printf "%s\n" "$step_rc" > /out/step-2.rc
if [ "$step_rc" -ne 0 ]; then exit "$step_rc"; fi
date -u +%FT%TZ > /out/step-3.start
cargo test --locked --offline -p compute_tools --test dsg_http resource_routes_require_control_scope_and_frozen_instance_before_uds -- --nocapture > /out/step-3.log 2>&1
step_rc=$?
printf "%s\n" "$step_rc" > /out/step-3.rc
if [ "$step_rc" -ne 0 ]; then exit "$step_rc"; fi
