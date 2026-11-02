//! 只用于定向验证：真实18原生PG文件进入正式Rust FFI，不能代替PS/SK服务验收。
use std::{env, fs, str::FromStr};

use anyhow::{Context, Result, ensure};
use postgres_ffi::walrecord::{DecodedWALRecord, decode_wal_record};
use postgres_ffi::{PgMajorVersion, v18, waldecoder::WalStreamDecoder};
use utils::lsn::Lsn;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("control") => {
            ensure!(args.len() == 3, "control needs original file");
            let raw = fs::read(&args[2])?;
            let control = v18::ControlFileData::decode(&raw)?;
            ensure!(
                control.pg_control_version == 1800,
                "not actual PG18 control"
            );
            ensure!(
                control.catalog_version_no == 202506291,
                "unexpected actual catalog"
            );
            ensure!(
                v18::bindings::PG_CONTROL_VERSION == 1800,
                "wrong header input"
            );
            let encoded = control.encode();
            ensure!(
                encoded.as_ref() == raw.as_slice(),
                "control bytes changed in roundtrip"
            );
            let roundtrip = v18::ControlFileData::decode(&encoded)?;
            ensure!(
                roundtrip.system_identifier == control.system_identifier,
                "roundtrip system id"
            );
            ensure!(
                roundtrip.default_char_signedness == control.default_char_signedness,
                "new18 field lost"
            );
            // CRC正确的外版本也须拒绝；不能只靠破坏CRC证明版本界限。
            let mut foreign = control;
            foreign.pg_control_version = 1700;
            let error = v18::ControlFileData::decode(&foreign.encode()).unwrap_err();
            ensure!(
                error
                    .to_string()
                    .contains("unexpected PostgreSQL control version"),
                "wrong-version cause"
            );
            let mut corrupt = raw;
            corrupt[0] ^= 1;
            ensure!(
                v18::ControlFileData::decode(&corrupt).is_err(),
                "CRC corruption accepted"
            );
            println!(
                "{{\"control_version\":1800,\"catalog_version\":202506291,\"roundtrip\":true,\"CRC_negative\":true,\"foreign_version_with_valid_CRC_negative\":true,\"Source\":false,\"E\":0}}"
            );
        }
        Some("wal") => {
            ensure!(args.len() == 5, "wal needs original file/start/end LSN");
            let start = Lsn::from_str(&args[3]).context("start LSN")?;
            let end = Lsn::from_str(&args[4]).context("end LSN")?;
            ensure!(end.0 > start.0, "empty original WAL interval");
            let raw = fs::read(&args[2])?;
            let len = usize::try_from(end.0 - start.0)?;
            ensure!(len <= raw.len(), "interval beyond original segment");
            let raw = &raw[..len];
            ensure!(
                u16::from_le_bytes(raw[..2].try_into()?) == 0xD118,
                "not actual18 WAL"
            );
            let mut decoder = WalStreamDecoder::new(start, PgMajorVersion::PG18);
            decoder.feed_bytes(raw);
            let mut records = 0u64;
            let mut marker = false;
            let mut last = start;
            while let Some((lsn, record)) = decoder.poll_decode()? {
                let mut decoded = DecodedWALRecord::default();
                decode_wal_record(record.clone(), &mut decoded, PgMajorVersion::PG18)?;
                marker |= record
                    .windows(b"actual-v18-format".len())
                    .any(|w| w == b"actual-v18-format");
                ensure!(
                    lsn > last && lsn <= end,
                    "record position outside original interval"
                );
                last = lsn;
                records += 1;
            }
            ensure!(records > 0 && marker, "actual SQL marker not decoded");
            // 17页magic绝不通过18入口，即便实际页其余字段未变。
            let mut wrong_magic = raw.to_vec();
            wrong_magic[..2].copy_from_slice(&0xD116u16.to_le_bytes());
            let mut bad = WalStreamDecoder::new(start, PgMajorVersion::PG18);
            bad.feed_bytes(&wrong_magic);
            ensure!(bad.poll_decode().is_err(), "foreign WAL magic accepted");
            // 内容损坏必须在实际record CRC路径失败，不改header/不假造完成。
            let offset = raw
                .windows(b"actual-v18-format".len())
                .position(|w| w == b"actual-v18-format")
                .context("marker original offset")?;
            let mut corrupt = raw.to_vec();
            corrupt[offset] ^= 1;
            let mut bad = WalStreamDecoder::new(start, PgMajorVersion::PG18);
            bad.feed_bytes(&corrupt);
            let mut rejected = false;
            loop {
                match bad.poll_decode() {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(_) => {
                        rejected = true;
                        break;
                    }
                }
            }
            ensure!(rejected, "actual WAL CRC corruption accepted");
            println!(
                "{{\"major\":18,\"records\":{records},\"last_lsn\":\"{last}\",\"marker\":true,\"foreign_magic_negative\":true,\"CRC_negative\":true,\"Source\":false,\"E\":0}}"
            );
        }
        _ => anyhow::bail!("use control/wal with original PG18 files"),
    }
    Ok(())
}
