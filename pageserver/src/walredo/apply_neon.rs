use crate::pgdatadir_mapping::AuxFilesDirectory;
use crate::walrecord::NeonWalRecord;
use anyhow::Context;
use byteorder::{ByteOrder, LittleEndian};
use bytes::{BufMut, BytesMut};
use pageserver_api::key::{key_to_rel_block, key_to_slru_block, Key};
use pageserver_api::reltag::SlruKind;
use postgres_ffi::pg_constants;
use postgres_ffi::relfile_utils::VISIBILITYMAP_FORKNUM;
use postgres_ffi::v14::nonrelfile_utils::{
    mx_offset_to_flags_bitshift, mx_offset_to_flags_offset, mx_offset_to_member_offset,
    transaction_id_set_status,
};
use postgres_ffi::BLCKSZ;
use tracing::*;
use utils::bin_ser::BeSer;

/// Can this request be served by neon redo functions
/// or we need to pass it to wal-redo postgres process?
pub(crate) fn can_apply_in_neon(rec: &NeonWalRecord) -> bool {
    // Currently, we don't have bespoken Rust code to replay any
    // Postgres WAL records. But everything else is handled in neon.
    #[allow(clippy::match_like_matches_macro)]
    match rec {
        NeonWalRecord::Postgres {
            will_init: _,
            rec: _,
        } => false,
        _ => true,
    }
}

pub(crate) fn apply_in_neon(
    record: &NeonWalRecord,
    key: Key,
    page: &mut BytesMut,
) -> Result<(), anyhow::Error> {
    match record {
        NeonWalRecord::Postgres {
            will_init: _,
            rec: _,
        } => {
            anyhow::bail!("tried to pass postgres wal record to neon WAL redo");
        }
        NeonWalRecord::ClearVisibilityMapFlags {
            new_heap_blkno,
            old_heap_blkno,
            flags,
        } => {
            // sanity check that this is modifying the correct relation
            let (rel, blknum) = key_to_rel_block(key).context("invalid record")?;
            assert!(
                rel.forknum == VISIBILITYMAP_FORKNUM,
                "ClearVisibilityMapFlags record on unexpected rel {}",
                rel
            );
            if let Some(heap_blkno) = *new_heap_blkno {
                // Calculate the VM block and offset that corresponds to the heap block.
                let map_block = pg_constants::HEAPBLK_TO_MAPBLOCK(heap_blkno);
                let map_byte = pg_constants::HEAPBLK_TO_MAPBYTE(heap_blkno);
                let map_offset = pg_constants::HEAPBLK_TO_OFFSET(heap_blkno);

                // Check that we're modifying the correct VM block.
                assert!(map_block == blknum);

                // equivalent to PageGetContents(page)
                let map = &mut page[pg_constants::MAXALIGN_SIZE_OF_PAGE_HEADER_DATA..];

                map[map_byte as usize] &= !(flags << map_offset);
            }

            // Repeat for 'old_heap_blkno', if any
            if let Some(heap_blkno) = *old_heap_blkno {
                let map_block = pg_constants::HEAPBLK_TO_MAPBLOCK(heap_blkno);
                let map_byte = pg_constants::HEAPBLK_TO_MAPBYTE(heap_blkno);
                let map_offset = pg_constants::HEAPBLK_TO_OFFSET(heap_blkno);

                assert!(map_block == blknum);

                let map = &mut page[pg_constants::MAXALIGN_SIZE_OF_PAGE_HEADER_DATA..];

                map[map_byte as usize] &= !(flags << map_offset);
            }
        }
        // Non-relational WAL records are handled here, with custom code that has the
        // same effects as the corresponding Postgres WAL redo function.
        NeonWalRecord::ClogSetCommitted { xids, timestamp } => {
            let (slru_kind, segno, blknum) = key_to_slru_block(key).context("invalid record")?;
            assert_eq!(
                slru_kind,
                SlruKind::Clog,
                "ClogSetCommitted record with unexpected key {}",
                key
            );
            for &xid in xids {
                let pageno = xid / pg_constants::CLOG_XACTS_PER_PAGE;
                let expected_segno = pageno / pg_constants::SLRU_PAGES_PER_SEGMENT;
                let expected_blknum = pageno % pg_constants::SLRU_PAGES_PER_SEGMENT;

                // Check that we're modifying the correct CLOG block.
                assert!(
                    segno == expected_segno,
                    "ClogSetCommitted record for XID {} with unexpected key {}",
                    xid,
                    key
                );
                assert!(
                    blknum == expected_blknum,
                    "ClogSetCommitted record for XID {} with unexpected key {}",
                    xid,
                    key
                );

                transaction_id_set_status(xid, pg_constants::TRANSACTION_STATUS_COMMITTED, page);
            }

            // Append the timestamp
            if page.len() == BLCKSZ as usize + 8 {
                page.truncate(BLCKSZ as usize);
            }
            if page.len() == BLCKSZ as usize {
                page.extend_from_slice(&timestamp.to_be_bytes());
            } else {
                warn!(
                    "CLOG blk {} in seg {} has invalid size {}",
                    blknum,
                    segno,
                    page.len()
                );
            }
        }
        NeonWalRecord::ClogSetAborted { xids } => {
            let (slru_kind, segno, blknum) = key_to_slru_block(key).context("invalid record")?;
            assert_eq!(
                slru_kind,
                SlruKind::Clog,
                "ClogSetAborted record with unexpected key {}",
                key
            );
            for &xid in xids {
                let pageno = xid / pg_constants::CLOG_XACTS_PER_PAGE;
                let expected_segno = pageno / pg_constants::SLRU_PAGES_PER_SEGMENT;
                let expected_blknum = pageno % pg_constants::SLRU_PAGES_PER_SEGMENT;

                // Check that we're modifying the correct CLOG block.
                assert!(
                    segno == expected_segno,
                    "ClogSetAborted record for XID {} with unexpected key {}",
                    xid,
                    key
                );
                assert!(
                    blknum == expected_blknum,
                    "ClogSetAborted record for XID {} with unexpected key {}",
                    xid,
                    key
                );

                transaction_id_set_status(xid, pg_constants::TRANSACTION_STATUS_ABORTED, page);
            }
        }
        NeonWalRecord::MultixactOffsetCreate { mid, moff } => {
            let (slru_kind, segno, blknum) = key_to_slru_block(key).context("invalid record")?;
            assert_eq!(
                slru_kind,
                SlruKind::MultiXactOffsets,
                "MultixactOffsetCreate record with unexpected key {}",
                key
            );
            // Compute the block and offset to modify.
            // See RecordNewMultiXact in PostgreSQL sources.
            let pageno = mid / pg_constants::MULTIXACT_OFFSETS_PER_PAGE as u32;
            let entryno = mid % pg_constants::MULTIXACT_OFFSETS_PER_PAGE as u32;
            let offset = (entryno * 4) as usize;

            // Check that we're modifying the correct multixact-offsets block.
            let expected_segno = pageno / pg_constants::SLRU_PAGES_PER_SEGMENT;
            let expected_blknum = pageno % pg_constants::SLRU_PAGES_PER_SEGMENT;
            assert!(
                segno == expected_segno,
                "MultiXactOffsetsCreate record for multi-xid {} with unexpected key {}",
                mid,
                key
            );
            assert!(
                blknum == expected_blknum,
                "MultiXactOffsetsCreate record for multi-xid {} with unexpected key {}",
                mid,
                key
            );

            LittleEndian::write_u32(&mut page[offset..offset + 4], *moff);
        }
        NeonWalRecord::MultixactMembersCreate { moff, members } => {
            let (slru_kind, segno, blknum) = key_to_slru_block(key).context("invalid record")?;
            assert_eq!(
                slru_kind,
                SlruKind::MultiXactMembers,
                "MultixactMembersCreate record with unexpected key {}",
                key
            );
            for (i, member) in members.iter().enumerate() {
                let offset = moff + i as u32;

                // Compute the block and offset to modify.
                // See RecordNewMultiXact in PostgreSQL sources.
                let pageno = offset / pg_constants::MULTIXACT_MEMBERS_PER_PAGE as u32;
                let memberoff = mx_offset_to_member_offset(offset);
                let flagsoff = mx_offset_to_flags_offset(offset);
                let bshift = mx_offset_to_flags_bitshift(offset);

                // Check that we're modifying the correct multixact-members block.
                let expected_segno = pageno / pg_constants::SLRU_PAGES_PER_SEGMENT;
                let expected_blknum = pageno % pg_constants::SLRU_PAGES_PER_SEGMENT;
                assert!(
                    segno == expected_segno,
                    "MultiXactMembersCreate record for offset {} with unexpected key {}",
                    moff,
                    key
                );
                assert!(
                    blknum == expected_blknum,
                    "MultiXactMembersCreate record for offset {} with unexpected key {}",
                    moff,
                    key
                );

                let mut flagsval = LittleEndian::read_u32(&page[flagsoff..flagsoff + 4]);
                flagsval &= !(((1 << pg_constants::MXACT_MEMBER_BITS_PER_XACT) - 1) << bshift);
                flagsval |= member.status << bshift;
                LittleEndian::write_u32(&mut page[flagsoff..flagsoff + 4], flagsval);
                LittleEndian::write_u32(&mut page[memberoff..memberoff + 4], member.xid);
            }
        }
        NeonWalRecord::AuxFile { file_path, content } => {
            let mut dir = AuxFilesDirectory::des(page)?;
            dir.upsert(file_path.clone(), content.clone());

            page.clear();
            let mut writer = page.writer();
            dir.ser_into(&mut writer)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use pageserver_api::key::AUX_FILES_KEY;

    use super::*;
    use std::collections::HashMap;

    use crate::{pgdatadir_mapping::AuxFilesDirectory, walrecord::NeonWalRecord};

    /// Test [`apply_in_neon`]'s handling of NeonWalRecord::AuxFile
    #[test]
    fn apply_aux_file_deltas() -> anyhow::Result<()> {
        let base_dir = AuxFilesDirectory {
            files: HashMap::from([
                ("two".to_string(), Bytes::from_static(b"content0")),
                ("three".to_string(), Bytes::from_static(b"contentX")),
            ]),
        };
        let base_image = AuxFilesDirectory::ser(&base_dir)?;

        let deltas = vec![
            // Insert
            NeonWalRecord::AuxFile {
                file_path: "one".to_string(),
                content: Some(Bytes::from_static(b"content1")),
            },
            // Update
            NeonWalRecord::AuxFile {
                file_path: "two".to_string(),
                content: Some(Bytes::from_static(b"content99")),
            },
            // Delete
            NeonWalRecord::AuxFile {
                file_path: "three".to_string(),
                content: None,
            },
        ];

        let file_path = AUX_FILES_KEY;
        let mut page = BytesMut::from_iter(base_image);

        for record in deltas {
            apply_in_neon(&record, file_path, &mut page)?;
        }

        let reconstructed = AuxFilesDirectory::des(&page)?;
        let expect = HashMap::from([
            ("one".to_string(), Bytes::from_static(b"content1")),
            ("two".to_string(), Bytes::from_static(b"content99")),
        ]);

        assert_eq!(reconstructed.files, expect);

        Ok(())
    }
}
