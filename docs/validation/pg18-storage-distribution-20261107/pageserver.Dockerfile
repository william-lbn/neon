FROM 127.0.0.1:5001/neon-candidate/pageserver@sha256:e6160b8bcb6eb3a4bb1a02d4bd43145f4396987d2d7ad40fbb9e83756674d055
COPY --chown=0:0 --chmod=0755 neon_walredo.so /usr/local/v18/lib/neon_walredo.so
