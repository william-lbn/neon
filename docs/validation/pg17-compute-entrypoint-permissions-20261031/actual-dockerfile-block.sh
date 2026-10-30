if [ "${PG_VERSION:?}" = "v17" ]; then \
      chown root:root /usr/local /usr/local/bin /usr/local/bin/postgres && \
      chmod go-w /usr/local /usr/local/bin /usr/local/bin/postgres; \
    fi
