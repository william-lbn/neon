//! Acceptor part of proposer-acceptor consensus algorithm.

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use bytes::{Buf, BufMut, Bytes, BytesMut};

use postgres_ffi::{TimeLineID, MAX_SEND_SIZE};
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::cmp::min;
use std::fmt;
use std::io::Read;
use std::time::Duration;
use storage_broker::proto::SafekeeperTimelineInfo;

use tracing::*;

use crate::control_file;
use crate::send_wal::HotStandbyFeedback;

use crate::state::TimelineState;
use crate::wal_storage;
use pq_proto::SystemId;
use utils::pageserver_feedback::PageserverFeedback;
use utils::{
    bin_ser::LeSer,
    id::{NodeId, TenantId, TimelineId},
    lsn::Lsn,
};

const SK_PROTOCOL_VERSION: u32 = 2;
pub const UNKNOWN_SERVER_VERSION: u32 = 0;

/// Consensus logical timestamp.
pub type Term = u64;
pub const INVALID_TERM: Term = 0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TermLsn {
    pub term: Term,
    pub lsn: Lsn,
}

// Creation from tuple provides less typing (e.g. for unit tests).
impl From<(Term, Lsn)> for TermLsn {
    fn from(pair: (Term, Lsn)) -> TermLsn {
        TermLsn {
            term: pair.0,
            lsn: pair.1,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct TermHistory(pub Vec<TermLsn>);

impl TermHistory {
    pub fn empty() -> TermHistory {
        TermHistory(Vec::new())
    }

    // Parse TermHistory as n_entries followed by TermLsn pairs
    pub fn from_bytes(bytes: &mut Bytes) -> Result<TermHistory> {
        if bytes.remaining() < 4 {
            bail!("TermHistory misses len");
        }
        let n_entries = bytes.get_u32_le();
        let mut res = Vec::with_capacity(n_entries as usize);
        for _ in 0..n_entries {
            if bytes.remaining() < 16 {
                bail!("TermHistory is incomplete");
            }
            res.push(TermLsn {
                term: bytes.get_u64_le(),
                lsn: bytes.get_u64_le().into(),
            })
        }
        Ok(TermHistory(res))
    }

    /// Return copy of self with switches happening strictly after up_to
    /// truncated.
    pub fn up_to(&self, up_to: Lsn) -> TermHistory {
        let mut res = Vec::with_capacity(self.0.len());
        for e in &self.0 {
            if e.lsn > up_to {
                break;
            }
            res.push(*e);
        }
        TermHistory(res)
    }

    /// Find point of divergence between leader (walproposer) term history and
    /// safekeeper. Arguments are not symmetrics as proposer history ends at
    /// +infinity while safekeeper at flush_lsn.
    /// C version is at walproposer SendProposerElected.
    pub fn find_highest_common_point(
        prop_th: &TermHistory,
        sk_th: &TermHistory,
        sk_wal_end: Lsn,
    ) -> Option<TermLsn> {
        let (prop_th, sk_th) = (&prop_th.0, &sk_th.0); // avoid .0 below

        if let Some(sk_th_last) = sk_th.last() {
            assert!(
                sk_th_last.lsn <= sk_wal_end,
                "safekeeper term history end {:?} LSN is higher than WAL end {:?}",
                sk_th_last,
                sk_wal_end
            );
        }

        // find last common term, if any...
        let mut last_common_idx = None;
        for i in 0..min(sk_th.len(), prop_th.len()) {
            if prop_th[i].term != sk_th[i].term {
                break;
            }
            // If term is the same, LSN must be equal as well.
            assert!(
                prop_th[i].lsn == sk_th[i].lsn,
                "same term {} has different start LSNs: prop {}, sk {}",
                prop_th[i].term,
                prop_th[i].lsn,
                sk_th[i].lsn
            );
            last_common_idx = Some(i);
        }
        let last_common_idx = match last_common_idx {
            None => return None, // no common point
            Some(lci) => lci,
        };
        // Now find where it ends at both prop and sk and take min. End of
        // (common) term is the start of the next except it is the last one;
        // there it is flush_lsn in case of safekeeper or, in case of proposer
        // +infinity, so we just take flush_lsn then.
        if last_common_idx == prop_th.len() - 1 {
            Some(TermLsn {
                term: prop_th[last_common_idx].term,
                lsn: sk_wal_end,
            })
        } else {
            let prop_common_term_end = prop_th[last_common_idx + 1].lsn;
            let sk_common_term_end = if last_common_idx + 1 < sk_th.len() {
                sk_th[last_common_idx + 1].lsn
            } else {
                sk_wal_end
            };
            Some(TermLsn {
                term: prop_th[last_common_idx].term,
                lsn: min(prop_common_term_end, sk_common_term_end),
            })
        }
    }
}

/// Display only latest entries for Debug.
impl fmt::Debug for TermHistory {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let n_printed = 20;
        write!(
            fmt,
            "{}{:?}",
            if self.0.len() > n_printed { "... " } else { "" },
            self.0
                .iter()
                .rev()
                .take(n_printed)
                .map(|&e| (e.term, e.lsn)) // omit TermSwitchEntry
                .collect::<Vec<_>>()
        )
    }
}

/// Unique id of proposer. Not needed for correctness, used for monitoring.
pub type PgUuid = [u8; 16];

/// Persistent consensus state of the acceptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcceptorState {
    /// acceptor's last term it voted for (advanced in 1 phase)
    pub term: Term,
    /// History of term switches for safekeeper's WAL.
    /// Actually it often goes *beyond* WAL contents as we adopt term history
    /// from the proposer before recovery.
    pub term_history: TermHistory,
}

impl AcceptorState {
    /// acceptor's epoch is the term of the highest entry in the log
    pub fn get_epoch(&self, flush_lsn: Lsn) -> Term {
        let th = self.term_history.up_to(flush_lsn);
        match th.0.last() {
            Some(e) => e.term,
            None => 0,
        }
    }
}

/// Information about Postgres. Safekeeper gets it once and then verifies
/// all further connections from computes match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Postgres server version
    pub pg_version: u32,
    pub system_id: SystemId,
    pub wal_seg_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedPeerInfo {
    /// LSN up to which safekeeper offloaded WAL to s3.
    pub backup_lsn: Lsn,
    /// Term of the last entry.
    pub term: Term,
    /// LSN of the last record.
    pub flush_lsn: Lsn,
    /// Up to which LSN safekeeper regards its WAL as committed.
    pub commit_lsn: Lsn,
}

impl PersistedPeerInfo {
    pub fn new() -> Self {
        Self {
            backup_lsn: Lsn::INVALID,
            term: INVALID_TERM,
            flush_lsn: Lsn(0),
            commit_lsn: Lsn(0),
        }
    }
}

// make clippy happy
impl Default for PersistedPeerInfo {
    fn default() -> Self {
        Self::new()
    }
}

// protocol messages

/// Initial Proposer -> Acceptor message
#[derive(Debug, Deserialize)]
pub struct ProposerGreeting {
    /// proposer-acceptor protocol version
    pub protocol_version: u32,
    /// Postgres server version
    pub pg_version: u32,
    pub proposer_id: PgUuid,
    pub system_id: SystemId,
    pub timeline_id: TimelineId,
    pub tenant_id: TenantId,
    pub tli: TimeLineID,
    pub wal_seg_size: u32,
}

/// Acceptor -> Proposer initial response: the highest term known to me
/// (acceptor voted for).
#[derive(Debug, Serialize)]
pub struct AcceptorGreeting {
    term: u64,
    node_id: NodeId,
}

/// Vote request sent from proposer to safekeepers
#[derive(Debug, Deserialize)]
pub struct VoteRequest {
    pub term: Term,
}

/// Vote itself, sent from safekeeper to proposer
#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub term: Term, // safekeeper's current term; if it is higher than proposer's, the compute is out of date.
    vote_given: u64, // fixme u64 due to padding
    // Safekeeper flush_lsn (end of WAL) + history of term switches allow
    // proposer to choose the most advanced one.
    pub flush_lsn: Lsn,
    truncate_lsn: Lsn,
    pub term_history: TermHistory,
    timeline_start_lsn: Lsn,
}

/*
 * Proposer -> Acceptor message announcing proposer is elected and communicating
 * term history to it.
 */
#[derive(Debug)]
pub struct ProposerElected {
    pub term: Term,
    pub start_streaming_at: Lsn,
    pub term_history: TermHistory,
    pub timeline_start_lsn: Lsn,
}

/// Request with WAL message sent from proposer to safekeeper. Along the way it
/// communicates commit_lsn.
#[derive(Debug)]
pub struct AppendRequest {
    pub h: AppendRequestHeader,
    pub wal_data: Bytes,
}
#[derive(Debug, Clone, Deserialize)]
pub struct AppendRequestHeader {
    // safekeeper's current term; if it is higher than proposer's, the compute is out of date.
    pub term: Term,
    // TODO: remove this field, it in unused -- LSN of term switch can be taken
    // from ProposerElected (as well as from term history).
    pub epoch_start_lsn: Lsn,
    /// start position of message in WAL
    pub begin_lsn: Lsn,
    /// end position of message in WAL
    pub end_lsn: Lsn,
    /// LSN committed by quorum of safekeepers
    pub commit_lsn: Lsn,
    /// minimal LSN which may be needed by proposer to perform recovery of some safekeeper
    pub truncate_lsn: Lsn,
    // only for logging/debugging
    pub proposer_uuid: PgUuid,
}

/// Report safekeeper state to proposer
#[derive(Debug, Serialize)]
pub struct AppendResponse {
    // Current term of the safekeeper; if it is higher than proposer's, the
    // compute is out of date.
    pub term: Term,
    // NOTE: this is physical end of wal on safekeeper; currently it doesn't
    // make much sense without taking epoch into account, as history can be
    // diverged.
    pub flush_lsn: Lsn,
    // We report back our awareness about which WAL is committed, as this is
    // a criterion for walproposer --sync mode exit
    pub commit_lsn: Lsn,
    pub hs_feedback: HotStandbyFeedback,
    pub pageserver_feedback: PageserverFeedback,
}

impl AppendResponse {
    fn term_only(term: Term) -> AppendResponse {
        AppendResponse {
            term,
            flush_lsn: Lsn(0),
            commit_lsn: Lsn(0),
            hs_feedback: HotStandbyFeedback::empty(),
            pageserver_feedback: PageserverFeedback::empty(),
        }
    }
}

/// Proposer -> Acceptor messages
#[derive(Debug)]
pub enum ProposerAcceptorMessage {
    Greeting(ProposerGreeting),
    VoteRequest(VoteRequest),
    Elected(ProposerElected),
    AppendRequest(AppendRequest),
    NoFlushAppendRequest(AppendRequest),
    FlushWAL,
}

impl ProposerAcceptorMessage {
    /// Parse proposer message.
    pub fn parse(msg_bytes: Bytes) -> Result<ProposerAcceptorMessage> {
        // xxx using Reader is inefficient but easy to work with bincode
        let mut stream = msg_bytes.reader();
        // u64 is here to avoid padding; it will be removed once we stop packing C structs into the wire as is
        let tag = stream.read_u64::<LittleEndian>()? as u8 as char;
        match tag {
            'g' => {
                let msg = ProposerGreeting::des_from(&mut stream)?;
                Ok(ProposerAcceptorMessage::Greeting(msg))
            }
            'v' => {
                let msg = VoteRequest::des_from(&mut stream)?;
                Ok(ProposerAcceptorMessage::VoteRequest(msg))
            }
            'e' => {
                let mut msg_bytes = stream.into_inner();
                if msg_bytes.remaining() < 16 {
                    bail!("ProposerElected message is not complete");
                }
                let term = msg_bytes.get_u64_le();
                let start_streaming_at = msg_bytes.get_u64_le().into();
                let term_history = TermHistory::from_bytes(&mut msg_bytes)?;
                if msg_bytes.remaining() < 8 {
                    bail!("ProposerElected message is not complete");
                }
                let timeline_start_lsn = msg_bytes.get_u64_le().into();
                let msg = ProposerElected {
                    term,
                    start_streaming_at,
                    timeline_start_lsn,
                    term_history,
                };
                Ok(ProposerAcceptorMessage::Elected(msg))
            }
            'a' => {
                // read header followed by wal data
                let hdr = AppendRequestHeader::des_from(&mut stream)?;
                let rec_size = hdr
                    .end_lsn
                    .checked_sub(hdr.begin_lsn)
                    .context("begin_lsn > end_lsn in AppendRequest")?
                    .0 as usize;
                if rec_size > MAX_SEND_SIZE {
                    bail!(
                        "AppendRequest is longer than MAX_SEND_SIZE ({})",
                        MAX_SEND_SIZE
                    );
                }

                let mut wal_data_vec: Vec<u8> = vec![0; rec_size];
                stream.read_exact(&mut wal_data_vec)?;
                let wal_data = Bytes::from(wal_data_vec);
                let msg = AppendRequest { h: hdr, wal_data };

                Ok(ProposerAcceptorMessage::AppendRequest(msg))
            }
            _ => bail!("unknown proposer-acceptor message tag: {}", tag),
        }
    }
}

/// Acceptor -> Proposer messages
#[derive(Debug)]
pub enum AcceptorProposerMessage {
    Greeting(AcceptorGreeting),
    VoteResponse(VoteResponse),
    AppendResponse(AppendResponse),
}

impl AcceptorProposerMessage {
    /// Serialize acceptor -> proposer message.
    pub fn serialize(&self, buf: &mut BytesMut) -> Result<()> {
        match self {
            AcceptorProposerMessage::Greeting(msg) => {
                buf.put_u64_le('g' as u64);
                buf.put_u64_le(msg.term);
                buf.put_u64_le(msg.node_id.0);
            }
            AcceptorProposerMessage::VoteResponse(msg) => {
                buf.put_u64_le('v' as u64);
                buf.put_u64_le(msg.term);
                buf.put_u64_le(msg.vote_given);
                buf.put_u64_le(msg.flush_lsn.into());
                buf.put_u64_le(msg.truncate_lsn.into());
                buf.put_u32_le(msg.term_history.0.len() as u32);
                for e in &msg.term_history.0 {
                    buf.put_u64_le(e.term);
                    buf.put_u64_le(e.lsn.into());
                }
                buf.put_u64_le(msg.timeline_start_lsn.into());
            }
            AcceptorProposerMessage::AppendResponse(msg) => {
                buf.put_u64_le('a' as u64);
                buf.put_u64_le(msg.term);
                buf.put_u64_le(msg.flush_lsn.into());
                buf.put_u64_le(msg.commit_lsn.into());
                buf.put_i64_le(msg.hs_feedback.ts);
                buf.put_u64_le(msg.hs_feedback.xmin);
                buf.put_u64_le(msg.hs_feedback.catalog_xmin);

                msg.pageserver_feedback.serialize(buf);
            }
        }

        Ok(())
    }
}

/// Safekeeper implements consensus to reliably persist WAL across nodes.
/// It controls all WAL disk writes and updates of control file.
///
/// Currently safekeeper processes:
/// - messages from compute (proposers) and provides replies
/// - messages from broker peers
pub struct SafeKeeper<CTRL: control_file::Storage, WAL: wal_storage::Storage> {
    /// LSN since the proposer safekeeper currently talking to appends WAL;
    /// determines epoch switch point.
    pub epoch_start_lsn: Lsn,

    pub state: TimelineState<CTRL>, // persistent state storage
    pub wal_store: WAL,

    node_id: NodeId, // safekeeper's node id
}

impl<CTRL, WAL> SafeKeeper<CTRL, WAL>
where
    CTRL: control_file::Storage,
    WAL: wal_storage::Storage,
{
    /// Accepts a control file storage containing the safekeeper state.
    /// State must be initialized, i.e. contain filled `tenant_id`, `timeline_id`
    /// and `server` (`wal_seg_size` inside it) fields.
    pub fn new(state: CTRL, wal_store: WAL, node_id: NodeId) -> Result<SafeKeeper<CTRL, WAL>> {
        if state.tenant_id == TenantId::from([0u8; 16])
            || state.timeline_id == TimelineId::from([0u8; 16])
        {
            bail!(
                "Calling SafeKeeper::new with empty tenant_id ({}) or timeline_id ({})",
                state.tenant_id,
                state.timeline_id
            );
        }

        Ok(SafeKeeper {
            epoch_start_lsn: Lsn(0),
            state: TimelineState::new(state),
            wal_store,
            node_id,
        })
    }

    /// Get history of term switches for the available WAL
    fn get_term_history(&self) -> TermHistory {
        self.state
            .acceptor_state
            .term_history
            .up_to(self.flush_lsn())
    }

    /// Get current term.
    pub fn get_term(&self) -> Term {
        self.state.acceptor_state.term
    }

    pub fn get_epoch(&self) -> Term {
        self.state.acceptor_state.get_epoch(self.flush_lsn())
    }

    /// wal_store wrapper avoiding commit_lsn <= flush_lsn violation when we don't have WAL yet.
    pub fn flush_lsn(&self) -> Lsn {
        max(self.wal_store.flush_lsn(), self.state.timeline_start_lsn)
    }

    /// Process message from proposer and possibly form reply. Concurrent
    /// callers must exclude each other.
    pub async fn process_msg(
        &mut self,
        msg: &ProposerAcceptorMessage,
    ) -> Result<Option<AcceptorProposerMessage>> {
        match msg {
            ProposerAcceptorMessage::Greeting(msg) => self.handle_greeting(msg).await,
            ProposerAcceptorMessage::VoteRequest(msg) => self.handle_vote_request(msg).await,
            ProposerAcceptorMessage::Elected(msg) => self.handle_elected(msg).await,
            ProposerAcceptorMessage::AppendRequest(msg) => {
                self.handle_append_request(msg, true).await
            }
            ProposerAcceptorMessage::NoFlushAppendRequest(msg) => {
                self.handle_append_request(msg, false).await
            }
            ProposerAcceptorMessage::FlushWAL => self.handle_flush().await,
        }
    }

    /// Handle initial message from proposer: check its sanity and send my
    /// current term.
    async fn handle_greeting(
        &mut self,
        msg: &ProposerGreeting,
    ) -> Result<Option<AcceptorProposerMessage>> {
        // Check protocol compatibility
        if msg.protocol_version != SK_PROTOCOL_VERSION {
            bail!(
                "incompatible protocol version {}, expected {}",
                msg.protocol_version,
                SK_PROTOCOL_VERSION
            );
        }
        /* Postgres major version mismatch is treated as fatal error
         * because safekeepers parse WAL headers and the format
         * may change between versions.
         */
        if msg.pg_version / 10000 != self.state.server.pg_version / 10000
            && self.state.server.pg_version != UNKNOWN_SERVER_VERSION
        {
            bail!(
                "incompatible server version {}, expected {}",
                msg.pg_version,
                self.state.server.pg_version
            );
        }

        if msg.tenant_id != self.state.tenant_id {
            bail!(
                "invalid tenant ID, got {}, expected {}",
                msg.tenant_id,
                self.state.tenant_id
            );
        }
        if msg.timeline_id != self.state.timeline_id {
            bail!(
                "invalid timeline ID, got {}, expected {}",
                msg.timeline_id,
                self.state.timeline_id
            );
        }
        if self.state.server.wal_seg_size != msg.wal_seg_size {
            bail!(
                "invalid wal_seg_size, got {}, expected {}",
                msg.wal_seg_size,
                self.state.server.wal_seg_size
            );
        }

        // system_id will be updated on mismatch
        // sync-safekeepers doesn't know sysid and sends 0, ignore it
        if self.state.server.system_id != msg.system_id && msg.system_id != 0 {
            if self.state.server.system_id != 0 {
                warn!(
                    "unexpected system ID arrived, got {}, expected {}",
                    msg.system_id, self.state.server.system_id
                );
            }

            let mut state = self.state.start_change();
            state.server.system_id = msg.system_id;
            if msg.pg_version != UNKNOWN_SERVER_VERSION {
                state.server.pg_version = msg.pg_version;
            }
            self.state.finish_change(&state).await?;
        }

        info!(
            "processed greeting from walproposer {}, sending term {:?}",
            msg.proposer_id.map(|b| format!("{:X}", b)).join(""),
            self.state.acceptor_state.term
        );
        Ok(Some(AcceptorProposerMessage::Greeting(AcceptorGreeting {
            term: self.state.acceptor_state.term,
            node_id: self.node_id,
        })))
    }

    /// Give vote for the given term, if we haven't done that previously.
    async fn handle_vote_request(
        &mut self,
        msg: &VoteRequest,
    ) -> Result<Option<AcceptorProposerMessage>> {
        // Once voted, we won't accept data from older proposers; flush
        // everything we've already received so that new proposer starts
        // streaming at end of our WAL, without overlap. Currently we truncate
        // WAL at streaming point, so this avoids truncating already committed
        // WAL.
        //
        // TODO: it would be smoother to not truncate committed piece at
        // handle_elected instead. Currently not a big deal, as proposer is the
        // only source of WAL; with peer2peer recovery it would be more
        // important.
        self.wal_store.flush_wal().await?;
        // initialize with refusal
        let mut resp = VoteResponse {
            term: self.state.acceptor_state.term,
            vote_given: false as u64,
            flush_lsn: self.flush_lsn(),
            truncate_lsn: self.state.inmem.peer_horizon_lsn,
            term_history: self.get_term_history(),
            timeline_start_lsn: self.state.timeline_start_lsn,
        };
        if self.state.acceptor_state.term < msg.term {
            let mut state = self.state.start_change();
            state.acceptor_state.term = msg.term;
            // persist vote before sending it out
            self.state.finish_change(&state).await?;

            resp.term = self.state.acceptor_state.term;
            resp.vote_given = true as u64;
        }
        info!("processed VoteRequest for term {}: {:?}", msg.term, &resp);
        Ok(Some(AcceptorProposerMessage::VoteResponse(resp)))
    }

    /// Form AppendResponse from current state.
    fn append_response(&self) -> AppendResponse {
        let ar = AppendResponse {
            term: self.state.acceptor_state.term,
            flush_lsn: self.flush_lsn(),
            commit_lsn: self.state.commit_lsn,
            // will be filled by the upper code to avoid bothering safekeeper
            hs_feedback: HotStandbyFeedback::empty(),
            pageserver_feedback: PageserverFeedback::empty(),
        };
        trace!("formed AppendResponse {:?}", ar);
        ar
    }

    async fn handle_elected(
        &mut self,
        msg: &ProposerElected,
    ) -> Result<Option<AcceptorProposerMessage>> {
        info!("received ProposerElected {:?}", msg);
        if self.state.acceptor_state.term < msg.term {
            let mut state = self.state.start_change();
            state.acceptor_state.term = msg.term;
            self.state.finish_change(&state).await?;
        }

        // If our term is higher, ignore the message (next feedback will inform the compute)
        if self.state.acceptor_state.term > msg.term {
            return Ok(None);
        }

        // This might happen in a rare race when another (old) connection from
        // the same walproposer writes + flushes WAL after this connection
        // already sent flush_lsn in VoteRequest. It is generally safe to
        // proceed, but to prevent commit_lsn surprisingly going down we should
        // either refuse the session (simpler) or skip the part we already have
        // from the stream (can be implemented).
        if msg.term == self.get_epoch() && self.flush_lsn() > msg.start_streaming_at {
            bail!("refusing ProposerElected which is going to overwrite correct WAL: term={}, flush_lsn={}, start_streaming_at={}; restarting the handshake should help",
                   msg.term, self.flush_lsn(), msg.start_streaming_at)
        }
        // Otherwise we must never attempt to truncate committed data.
        assert!(
            msg.start_streaming_at >= self.state.inmem.commit_lsn,
            "attempt to truncate committed data: start_streaming_at={}, commit_lsn={}",
            msg.start_streaming_at,
            self.state.inmem.commit_lsn
        );

        // TODO: cross check divergence point, check if msg.start_streaming_at corresponds to
        // intersection of our history and history from msg

        // truncate wal, update the LSNs
        self.wal_store.truncate_wal(msg.start_streaming_at).await?;

        // and now adopt term history from proposer
        {
            let mut state = self.state.start_change();

            // Here we learn initial LSN for the first time, set fields
            // interested in that.

            if state.timeline_start_lsn == Lsn(0) {
                // Remember point where WAL begins globally.
                state.timeline_start_lsn = msg.timeline_start_lsn;
                info!(
                    "setting timeline_start_lsn to {:?}",
                    state.timeline_start_lsn
                );
            }
            if state.peer_horizon_lsn == Lsn(0) {
                // Update peer_horizon_lsn as soon as we know where timeline starts.
                // It means that peer_horizon_lsn cannot be zero after we know timeline_start_lsn.
                state.peer_horizon_lsn = msg.timeline_start_lsn;
            }
            if state.local_start_lsn == Lsn(0) {
                state.local_start_lsn = msg.start_streaming_at;
                info!("setting local_start_lsn to {:?}", state.local_start_lsn);
            }
            // Initializing commit_lsn before acking first flushed record is
            // important to let find_end_of_wal skip the hole in the beginning
            // of the first segment.
            //
            // NB: on new clusters, this happens at the same time as
            // timeline_start_lsn initialization, it is taken outside to provide
            // upgrade.
            state.commit_lsn = max(state.commit_lsn, state.timeline_start_lsn);

            // Initializing backup_lsn is useful to avoid making backup think it should upload 0 segment.
            state.backup_lsn = max(state.backup_lsn, state.timeline_start_lsn);

            state.acceptor_state.term_history = msg.term_history.clone();
            self.state.finish_change(&state).await?;
        }

        info!("start receiving WAL since {:?}", msg.start_streaming_at);

        // Cache LSN where term starts to immediately fsync control file with
        // commit_lsn once we reach it -- sync-safekeepers finishes when
        // persisted commit_lsn on majority of safekeepers aligns.
        self.epoch_start_lsn = match msg.term_history.0.last() {
            None => bail!("proposer elected with empty term history"),
            Some(term_lsn_start) => term_lsn_start.lsn,
        };

        Ok(None)
    }

    /// Advance commit_lsn taking into account what we have locally.
    ///
    /// Note: it is assumed that 'WAL we have is from the right term' check has
    /// already been done outside.
    async fn update_commit_lsn(&mut self, mut candidate: Lsn) -> Result<()> {
        // Both peers and walproposer communicate this value, we might already
        // have a fresher (higher) version.
        candidate = max(candidate, self.state.inmem.commit_lsn);
        let commit_lsn = min(candidate, self.flush_lsn());
        assert!(
            commit_lsn >= self.state.inmem.commit_lsn,
            "commit_lsn monotonicity violated: old={} new={}",
            self.state.inmem.commit_lsn,
            commit_lsn
        );

        self.state.inmem.commit_lsn = commit_lsn;

        // If new commit_lsn reached epoch switch, force sync of control
        // file: walproposer in sync mode is very interested when this
        // happens. Note: this is for sync-safekeepers mode only, as
        // otherwise commit_lsn might jump over epoch_start_lsn.
        if commit_lsn >= self.epoch_start_lsn && self.state.commit_lsn < self.epoch_start_lsn {
            self.state.flush().await?;
        }

        Ok(())
    }

    /// Persist control file if there is something to save and enough time
    /// passed after the last save.
    pub async fn maybe_persist_inmem_control_file(&mut self) -> Result<()> {
        const CF_SAVE_INTERVAL: Duration = Duration::from_secs(300);
        if self.state.pers.last_persist_at().elapsed() < CF_SAVE_INTERVAL {
            return Ok(());
        }
        let need_persist = self.state.inmem.commit_lsn > self.state.commit_lsn
            || self.state.inmem.backup_lsn > self.state.backup_lsn
            || self.state.inmem.peer_horizon_lsn > self.state.peer_horizon_lsn
            || self.state.inmem.remote_consistent_lsn > self.state.remote_consistent_lsn;
        if need_persist {
            self.state.flush().await?;
            trace!("saved control file: {CF_SAVE_INTERVAL:?} passed");
        }
        Ok(())
    }

    /// Handle request to append WAL.
    #[allow(clippy::comparison_chain)]
    async fn handle_append_request(
        &mut self,
        msg: &AppendRequest,
        require_flush: bool,
    ) -> Result<Option<AcceptorProposerMessage>> {
        if self.state.acceptor_state.term < msg.h.term {
            bail!("got AppendRequest before ProposerElected");
        }

        // If our term is higher, immediately refuse the message.
        if self.state.acceptor_state.term > msg.h.term {
            let resp = AppendResponse::term_only(self.state.acceptor_state.term);
            return Ok(Some(AcceptorProposerMessage::AppendResponse(resp)));
        }

        // Now we know that we are in the same term as the proposer,
        // processing the message.

        self.state.inmem.proposer_uuid = msg.h.proposer_uuid;

        // do the job
        if !msg.wal_data.is_empty() {
            self.wal_store
                .write_wal(msg.h.begin_lsn, &msg.wal_data)
                .await?;
        }

        // flush wal to the disk, if required
        if require_flush {
            self.wal_store.flush_wal().await?;
        }

        // Update commit_lsn.
        if msg.h.commit_lsn != Lsn(0) {
            self.update_commit_lsn(msg.h.commit_lsn).await?;
        }
        // Value calculated by walproposer can always lag:
        // - safekeepers can forget inmem value and send to proposer lower
        //   persisted one on restart;
        // - if we make safekeepers always send persistent value,
        //   any compute restart would pull it down.
        // Thus, take max before adopting.
        self.state.inmem.peer_horizon_lsn =
            max(self.state.inmem.peer_horizon_lsn, msg.h.truncate_lsn);

        // Update truncate and commit LSN in control file.
        // To avoid negative impact on performance of extra fsync, do it only
        // when commit_lsn delta exceeds WAL segment size.
        if self.state.commit_lsn + (self.state.server.wal_seg_size as u64)
            < self.state.inmem.commit_lsn
        {
            self.state.flush().await?;
        }

        trace!(
            "processed AppendRequest of len {}, end_lsn={:?}, commit_lsn={:?}, truncate_lsn={:?}, flushed={:?}",
            msg.wal_data.len(),
            msg.h.end_lsn,
            msg.h.commit_lsn,
            msg.h.truncate_lsn,
            require_flush,
        );

        // If flush_lsn hasn't updated, AppendResponse is not very useful.
        if !require_flush {
            return Ok(None);
        }

        let resp = self.append_response();
        Ok(Some(AcceptorProposerMessage::AppendResponse(resp)))
    }

    /// Flush WAL to disk. Return AppendResponse with latest LSNs.
    async fn handle_flush(&mut self) -> Result<Option<AcceptorProposerMessage>> {
        self.wal_store.flush_wal().await?;
        Ok(Some(AcceptorProposerMessage::AppendResponse(
            self.append_response(),
        )))
    }

    /// Update timeline state with peer safekeeper data.
    pub async fn record_safekeeper_info(&mut self, sk_info: &SafekeeperTimelineInfo) -> Result<()> {
        let mut sync_control_file = false;

        if (Lsn(sk_info.commit_lsn) != Lsn::INVALID) && (sk_info.last_log_term != INVALID_TERM) {
            // Note: the check is too restrictive, generally we can update local
            // commit_lsn if our history matches (is part of) history of advanced
            // commit_lsn provider.
            if sk_info.last_log_term == self.get_epoch() {
                self.update_commit_lsn(Lsn(sk_info.commit_lsn)).await?;
            }
        }

        self.state.inmem.backup_lsn = max(Lsn(sk_info.backup_lsn), self.state.inmem.backup_lsn);
        sync_control_file |= self.state.backup_lsn + (self.state.server.wal_seg_size as u64)
            < self.state.inmem.backup_lsn;

        self.state.inmem.remote_consistent_lsn = max(
            Lsn(sk_info.remote_consistent_lsn),
            self.state.inmem.remote_consistent_lsn,
        );
        sync_control_file |= self.state.remote_consistent_lsn
            + (self.state.server.wal_seg_size as u64)
            < self.state.inmem.remote_consistent_lsn;

        self.state.inmem.peer_horizon_lsn = max(
            Lsn(sk_info.peer_horizon_lsn),
            self.state.inmem.peer_horizon_lsn,
        );
        sync_control_file |= self.state.peer_horizon_lsn + (self.state.server.wal_seg_size as u64)
            < self.state.inmem.peer_horizon_lsn;

        if sync_control_file {
            self.state.flush().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures::future::BoxFuture;
    use postgres_ffi::{XLogSegNo, WAL_SEGMENT_SIZE};

    use super::*;
    use crate::{
        state::{PersistedPeers, TimelinePersistentState},
        wal_storage::Storage,
    };
    use std::{ops::Deref, str::FromStr, time::Instant};

    // fake storage for tests
    struct InMemoryState {
        persisted_state: TimelinePersistentState,
    }

    #[async_trait::async_trait]
    impl control_file::Storage for InMemoryState {
        async fn persist(&mut self, s: &TimelinePersistentState) -> Result<()> {
            self.persisted_state = s.clone();
            Ok(())
        }

        fn last_persist_at(&self) -> Instant {
            Instant::now()
        }
    }

    impl Deref for InMemoryState {
        type Target = TimelinePersistentState;

        fn deref(&self) -> &Self::Target {
            &self.persisted_state
        }
    }

    fn test_sk_state() -> TimelinePersistentState {
        let mut state = TimelinePersistentState::empty();
        state.server.wal_seg_size = WAL_SEGMENT_SIZE as u32;
        state.tenant_id = TenantId::from([1u8; 16]);
        state.timeline_id = TimelineId::from([1u8; 16]);
        state
    }

    struct DummyWalStore {
        lsn: Lsn,
    }

    #[async_trait::async_trait]
    impl wal_storage::Storage for DummyWalStore {
        fn flush_lsn(&self) -> Lsn {
            self.lsn
        }

        async fn write_wal(&mut self, startpos: Lsn, buf: &[u8]) -> Result<()> {
            self.lsn = startpos + buf.len() as u64;
            Ok(())
        }

        async fn truncate_wal(&mut self, end_pos: Lsn) -> Result<()> {
            self.lsn = end_pos;
            Ok(())
        }

        async fn flush_wal(&mut self) -> Result<()> {
            Ok(())
        }

        fn remove_up_to(&self, _segno_up_to: XLogSegNo) -> BoxFuture<'static, anyhow::Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn get_metrics(&self) -> crate::metrics::WalStorageMetrics {
            crate::metrics::WalStorageMetrics::default()
        }
    }

    #[tokio::test]
    async fn test_voting() {
        let storage = InMemoryState {
            persisted_state: test_sk_state(),
        };
        let wal_store = DummyWalStore { lsn: Lsn(0) };
        let mut sk = SafeKeeper::new(storage, wal_store, NodeId(0)).unwrap();

        // check voting for 1 is ok
        let vote_request = ProposerAcceptorMessage::VoteRequest(VoteRequest { term: 1 });
        let mut vote_resp = sk.process_msg(&vote_request).await;
        match vote_resp.unwrap() {
            Some(AcceptorProposerMessage::VoteResponse(resp)) => assert!(resp.vote_given != 0),
            r => panic!("unexpected response: {:?}", r),
        }

        // reboot...
        let state = sk.state.deref().clone();
        let storage = InMemoryState {
            persisted_state: state,
        };

        sk = SafeKeeper::new(storage, sk.wal_store, NodeId(0)).unwrap();

        // and ensure voting second time for 1 is not ok
        vote_resp = sk.process_msg(&vote_request).await;
        match vote_resp.unwrap() {
            Some(AcceptorProposerMessage::VoteResponse(resp)) => assert!(resp.vote_given == 0),
            r => panic!("unexpected response: {:?}", r),
        }
    }

    #[tokio::test]
    async fn test_epoch_switch() {
        let storage = InMemoryState {
            persisted_state: test_sk_state(),
        };
        let wal_store = DummyWalStore { lsn: Lsn(0) };

        let mut sk = SafeKeeper::new(storage, wal_store, NodeId(0)).unwrap();

        let mut ar_hdr = AppendRequestHeader {
            term: 1,
            epoch_start_lsn: Lsn(3),
            begin_lsn: Lsn(1),
            end_lsn: Lsn(2),
            commit_lsn: Lsn(0),
            truncate_lsn: Lsn(0),
            proposer_uuid: [0; 16],
        };
        let mut append_request = AppendRequest {
            h: ar_hdr.clone(),
            wal_data: Bytes::from_static(b"b"),
        };

        let pem = ProposerElected {
            term: 1,
            start_streaming_at: Lsn(1),
            term_history: TermHistory(vec![TermLsn {
                term: 1,
                lsn: Lsn(3),
            }]),
            timeline_start_lsn: Lsn(0),
        };
        sk.process_msg(&ProposerAcceptorMessage::Elected(pem))
            .await
            .unwrap();

        // check that AppendRequest before epochStartLsn doesn't switch epoch
        let resp = sk
            .process_msg(&ProposerAcceptorMessage::AppendRequest(append_request))
            .await;
        assert!(resp.is_ok());
        assert_eq!(sk.get_epoch(), 0);

        // but record at epochStartLsn does the switch
        ar_hdr.begin_lsn = Lsn(2);
        ar_hdr.end_lsn = Lsn(3);
        append_request = AppendRequest {
            h: ar_hdr,
            wal_data: Bytes::from_static(b"b"),
        };
        let resp = sk
            .process_msg(&ProposerAcceptorMessage::AppendRequest(append_request))
            .await;
        assert!(resp.is_ok());
        sk.wal_store.truncate_wal(Lsn(3)).await.unwrap(); // imitate the complete record at 3 %)
        assert_eq!(sk.get_epoch(), 1);
    }

    #[test]
    fn test_find_highest_common_point_none() {
        let prop_th = TermHistory(vec![(0, Lsn(1)).into()]);
        let sk_th = TermHistory(vec![(1, Lsn(1)).into(), (2, Lsn(2)).into()]);
        assert_eq!(
            TermHistory::find_highest_common_point(&prop_th, &sk_th, Lsn(3),),
            None
        );
    }

    #[test]
    fn test_find_highest_common_point_middle() {
        let prop_th = TermHistory(vec![
            (1, Lsn(10)).into(),
            (2, Lsn(20)).into(),
            (4, Lsn(40)).into(),
        ]);
        let sk_th = TermHistory(vec![
            (1, Lsn(10)).into(),
            (2, Lsn(20)).into(),
            (3, Lsn(30)).into(), // sk ends last common term 2 at 30
        ]);
        assert_eq!(
            TermHistory::find_highest_common_point(&prop_th, &sk_th, Lsn(40),),
            Some(TermLsn {
                term: 2,
                lsn: Lsn(30),
            })
        );
    }

    #[test]
    fn test_find_highest_common_point_sk_end() {
        let prop_th = TermHistory(vec![
            (1, Lsn(10)).into(),
            (2, Lsn(20)).into(), // last common term 2, sk will end it at 32 sk_end_lsn
            (4, Lsn(40)).into(),
        ]);
        let sk_th = TermHistory(vec![(1, Lsn(10)).into(), (2, Lsn(20)).into()]);
        assert_eq!(
            TermHistory::find_highest_common_point(&prop_th, &sk_th, Lsn(32),),
            Some(TermLsn {
                term: 2,
                lsn: Lsn(32),
            })
        );
    }

    #[test]
    fn test_find_highest_common_point_walprop() {
        let prop_th = TermHistory(vec![(1, Lsn(10)).into(), (2, Lsn(20)).into()]);
        let sk_th = TermHistory(vec![(1, Lsn(10)).into(), (2, Lsn(20)).into()]);
        assert_eq!(
            TermHistory::find_highest_common_point(&prop_th, &sk_th, Lsn(32),),
            Some(TermLsn {
                term: 2,
                lsn: Lsn(32),
            })
        );
    }

    #[test]
    fn test_sk_state_bincode_serde_roundtrip() {
        use utils::Hex;
        let tenant_id = TenantId::from_str("cf0480929707ee75372337efaa5ecf96").unwrap();
        let timeline_id = TimelineId::from_str("112ded66422aa5e953e5440fa5427ac4").unwrap();
        let state = TimelinePersistentState {
            tenant_id,
            timeline_id,
            acceptor_state: AcceptorState {
                term: 42,
                term_history: TermHistory(vec![TermLsn {
                    lsn: Lsn(0x1),
                    term: 41,
                }]),
            },
            server: ServerInfo {
                pg_version: 14,
                system_id: 0x1234567887654321,
                wal_seg_size: 0x12345678,
            },
            proposer_uuid: {
                let mut arr = timeline_id.as_arr();
                arr.reverse();
                arr
            },
            timeline_start_lsn: Lsn(0x12345600),
            local_start_lsn: Lsn(0x12),
            commit_lsn: Lsn(1234567800),
            backup_lsn: Lsn(1234567300),
            peer_horizon_lsn: Lsn(9999999),
            remote_consistent_lsn: Lsn(1234560000),
            peers: PersistedPeers(vec![(
                NodeId(1),
                PersistedPeerInfo {
                    backup_lsn: Lsn(1234567000),
                    term: 42,
                    flush_lsn: Lsn(1234567800 - 8),
                    commit_lsn: Lsn(1234567600),
                },
            )]),
        };

        let ser = state.ser().unwrap();

        #[rustfmt::skip]
        let expected = [
            // tenant_id as length prefixed hex
            0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x63, 0x66, 0x30, 0x34, 0x38, 0x30, 0x39, 0x32, 0x39, 0x37, 0x30, 0x37, 0x65, 0x65, 0x37, 0x35, 0x33, 0x37, 0x32, 0x33, 0x33, 0x37, 0x65, 0x66, 0x61, 0x61, 0x35, 0x65, 0x63, 0x66, 0x39, 0x36,
            // timeline_id as length prefixed hex
            0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x31, 0x31, 0x32, 0x64, 0x65, 0x64, 0x36, 0x36, 0x34, 0x32, 0x32, 0x61, 0x61, 0x35, 0x65, 0x39, 0x35, 0x33, 0x65, 0x35, 0x34, 0x34, 0x30, 0x66, 0x61, 0x35, 0x34, 0x32, 0x37, 0x61, 0x63, 0x34,
            // term
            0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // length prefix
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // unsure why this order is swapped
            0x29, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // pg_version
            0x0e, 0x00, 0x00, 0x00,
            // systemid
            0x21, 0x43, 0x65, 0x87, 0x78, 0x56, 0x34, 0x12,
            // wal_seg_size
            0x78, 0x56, 0x34, 0x12,
            // pguuid as length prefixed hex
            0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x63, 0x34, 0x37, 0x61, 0x34, 0x32, 0x61, 0x35, 0x30, 0x66, 0x34, 0x34, 0x65, 0x35, 0x35, 0x33, 0x65, 0x39, 0x61, 0x35, 0x32, 0x61, 0x34, 0x32, 0x36, 0x36, 0x65, 0x64, 0x32, 0x64, 0x31, 0x31,

            // timeline_start_lsn
            0x00, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00,
            0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x78, 0x02, 0x96, 0x49, 0x00, 0x00, 0x00, 0x00,
            0x84, 0x00, 0x96, 0x49, 0x00, 0x00, 0x00, 0x00,
            0x7f, 0x96, 0x98, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0xe4, 0x95, 0x49, 0x00, 0x00, 0x00, 0x00,
            // length prefix for persistentpeers
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // nodeid
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // backuplsn
            0x58, 0xff, 0x95, 0x49, 0x00, 0x00, 0x00, 0x00,
            0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x70, 0x02, 0x96, 0x49, 0x00, 0x00, 0x00, 0x00,
            0xb0, 0x01, 0x96, 0x49, 0x00, 0x00, 0x00, 0x00,
        ];

        assert_eq!(Hex(&ser), Hex(&expected));

        let deser = TimelinePersistentState::des(&ser).unwrap();

        assert_eq!(deser, state);
    }
}
