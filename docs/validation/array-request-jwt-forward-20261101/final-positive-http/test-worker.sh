#!/bin/bash
set -u
date -u +%FT%TZ > /out/step-0.start
cargo test --locked --offline -p compute_tools --test dsg_http array_authority_routes_require_signed_body_and_separate_scope -- --nocapture > /out/step-0.log 2>&1
step_rc=$?
printf "%s\n" "$step_rc" > /out/step-0.rc
if [ "$step_rc" -ne 0 ]; then exit "$step_rc"; fi
