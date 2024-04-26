use std::fs::{File, OpenOptions};
use std::io;
use std::io::prelude::*;
use std::path::Path;

use anyhow::Result;

use crate::pg_helpers::escape_conf_value;
use crate::pg_helpers::PgOptionsSerialize;
use compute_api::spec::{ComputeMode, ComputeSpec};

/// Check that `line` is inside a text file and put it there if it is not.
/// Create file if it doesn't exist.
pub fn line_in_file(path: &Path, line: &str) -> Result<bool> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .append(false)
        .open(path)?;
    let buf = io::BufReader::new(&file);
    let mut count: usize = 0;

    for l in buf.lines() {
        if l? == line {
            return Ok(false);
        }
        count = 1;
    }

    write!(file, "{}{}", "\n".repeat(count), line)?;
    Ok(true)
}

/// Create or completely rewrite configuration file specified by `path`
pub fn write_postgres_conf(
    path: &Path,
    spec: &ComputeSpec,
    extension_server_port: Option<u16>,
) -> Result<()> {
    // File::create() destroys the file content if it exists.
    let mut file = File::create(path)?;

    // Write the postgresql.conf content from the spec file as is.
    if let Some(conf) = &spec.cluster.postgresql_conf {
        writeln!(file, "{}", conf)?;
    }

    // Add options for connecting to storage
    writeln!(file, "# Neon storage settings")?;
    if let Some(s) = &spec.pageserver_connstring {
        writeln!(file, "neon.pageserver_connstring={}", escape_conf_value(s))?;
    }
    if let Some(stripe_size) = spec.shard_stripe_size {
        writeln!(file, "neon.stripe_size={stripe_size}")?;
    }
    if !spec.safekeeper_connstrings.is_empty() {
        writeln!(
            file,
            "neon.safekeepers={}",
            escape_conf_value(&spec.safekeeper_connstrings.join(","))
        )?;
    }
    if let Some(s) = &spec.tenant_id {
        writeln!(file, "neon.tenant_id={}", escape_conf_value(&s.to_string()))?;
    }
    if let Some(s) = &spec.timeline_id {
        writeln!(
            file,
            "neon.timeline_id={}",
            escape_conf_value(&s.to_string())
        )?;
    }

    match spec.mode {
        ComputeMode::Primary => {}
        ComputeMode::Static(lsn) => {
            // hot_standby is 'on' by default, but let's be explicit
            writeln!(file, "hot_standby=on")?;
            writeln!(file, "recovery_target_lsn='{lsn}'")?;
        }
        ComputeMode::Replica => {
            // hot_standby is 'on' by default, but let's be explicit
            writeln!(file, "hot_standby=on")?;

            // Inform the replica about the primary state
            // Default is 'false'
            if let Some(primary_is_running) = spec.primary_is_running {
                writeln!(file, "neon.primary_is_running={}", primary_is_running)?;
            }
        }
    }

    // If there are any extra options in the 'settings' field, append those
    if spec.cluster.settings.is_some() {
        writeln!(file, "# Managed by compute_ctl: begin")?;
        write!(file, "{}", spec.cluster.settings.as_pg_settings())?;
        writeln!(file, "# Managed by compute_ctl: end")?;
    }

    if let Some(port) = extension_server_port {
        writeln!(file, "neon.extension_server_port={}", port)?;
    }

    // This is essential to keep this line at the end of the file,
    // because it is intended to override any settings above.
    writeln!(file, "include_if_exists = 'compute_ctl_temp_override.conf'")?;

    Ok(())
}

/// create file compute_ctl_temp_override.conf in pgdata_dir
/// add provided options to this file
pub fn compute_ctl_temp_override_create(pgdata_path: &Path, options: &str) -> Result<()> {
    let path = pgdata_path.join("compute_ctl_temp_override.conf");
    let mut file = File::create(path)?;
    write!(file, "{}", options)?;
    Ok(())
}

/// remove file compute_ctl_temp_override.conf in pgdata_dir
pub fn compute_ctl_temp_override_remove(pgdata_path: &Path) -> Result<()> {
    let path = pgdata_path.join("compute_ctl_temp_override.conf");
    std::fs::remove_file(path)?;
    Ok(())
}
