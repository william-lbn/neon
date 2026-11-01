//! 原Rust创建的PG stderr及有界日志收集。此模块本身不授normal来源资格。
//! Root注册/原NativeFinal/原Child Wait与Kernel EOF是独立必需事实。

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use regex::Regex;
use tokio::io::AsyncReadExt;

pub(crate) const PIPE_CAPACITY_BYTES: i32 = 64 * 1024;
const LINE_BYTES: usize = 16 * 1024;
const GROUP_BYTES: usize = 64 * 1024;
const READ_BYTES: usize = 8 * 1024;

/// 只由本构造器pipe2取得两个原端点；不能由raw fd号/路径/继承stdio补建。
#[cfg(target_os = "linux")]
pub(crate) struct OriginalPgStderr {
    reader: File,
    writer: OwnedFd,
    capacity: i32,
}

#[cfg(target_os = "linux")]
impl OriginalPgStderr {
    pub(crate) fn new() -> Result<Self> {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        let (reader, writer) = nix::unistd::pipe2(OFlag::O_CLOEXEC)?;
        // 唯一创建期resize。注册之后原Rust代码不再改变容量；PG原filter另硬拒SET。
        let set = fcntl(&writer, FcntlArg::F_SETPIPE_SZ(PIPE_CAPACITY_BYTES))?;
        let get = fcntl(&writer, FcntlArg::F_GETPIPE_SZ)?;
        ensure!(set == PIPE_CAPACITY_BYTES && get == set, "new PG stderr capacity differs");
        Ok(Self { reader: File::from(reader), writer, capacity: get })
    }

    pub(crate) fn endpoints(&self) -> (&File, &OwnedFd) { (&self.reader, &self.writer) }

    pub(crate) fn capacity(&self) -> i32 { self.capacity }

    /// 报文只有固定版本和两个新pipe原端点，不能选择PID/路径/预算/nonce。
    /// 选择器不是来源证明；Root从同g原Compute lease及kernel peer独立认定。
    pub(crate) fn register_with_root(&self) -> Result<()> {
        use std::io::{IoSlice, IoSliceMut};
        use std::os::fd::{AsRawFd, AsFd};
        use nix::sys::socket::{socket, connect, getsockopt, setsockopt, recvmsg, sendmsg, AddressFamily, SockType, SockFlag, UnixAddr, MsgFlags, ControlMessage, sockopt};
        use nix::sys::time::{TimeVal, TimeValLike};
        let socket=socket(AddressFamily::Unix,SockType::SeqPacket,SockFlag::SOCK_CLOEXEC,None)?;
        let timeout=TimeVal::seconds(30);
        setsockopt(&socket,sockopt::ReceiveTimeout,&timeout)?;
        setsockopt(&socket,sockopt::SendTimeout,&timeout)?;
        connect(socket.as_raw_fd(),&UnixAddr::new("/run/neon-dsga/pg-stderr.sock")?)?;
        let peer=getsockopt(&socket,sockopt::PeerCredentials)?;
        ensure!(peer.uid()==0&&peer.pid()>1,"PG stderr Root peer differs");
        let request=[b'N',b'D',b'S',b'G',b'P',b'S',b'0',b'1',0,0,0,1,0,0,0,0];
        let endpoints=[self.reader.as_raw_fd(),self.writer.as_raw_fd()];
        let sent=sendmsg::<()>(socket.as_raw_fd(),&[IoSlice::new(&request)],&[ControlMessage::ScmRights(&endpoints)],MsgFlags::MSG_NOSIGNAL,None)?;
        ensure!(sent==request.len(),"PG stderr registration short send; no retry");
        // 不提供ancillary buffer：任何附带rights均由kernel丢弃并置CTRUNC，绝不收养。
        let mut bytes=[0u8;17];let mut iov=[IoSliceMut::new(&mut bytes)];
        let message=recvmsg::<()>(socket.as_raw_fd(),&mut iov,None,MsgFlags::empty())?;
        let size=message.bytes;let flags=message.flags;
        ensure!(size==16&&!flags.intersects(MsgFlags::MSG_TRUNC|MsgFlags::MSG_CTRUNC),"PG stderr registration invalid reply; no retry");
        ensure!(bytes[..8]==*b"NDSGPS1A"&&bytes[8..16]==[0,0,0,1,0,0,0,0],"PG stderr registration unconfirmed; no retry");
        // 保持owned对象直到configure，传出的SCM副本不会改变原Rust所有权。
        let _=socket.as_fd();
        Ok(())
    }

    /// Root必须先登记这两个原对象。Command配置持有write端，因此spawn后须立即drop
    /// 原Command；不能在等待PG/EOF时让它继续保活write端。
    pub(crate) fn configure(self, command: &mut Command) -> File {
        command.stderr(Stdio::from(File::from(self.writer)));
        self.reader
    }
}


/// 此读取只选择新管道协议；不签来源。缺文件/未知版本/坏JSON不得降回旧stdio。
#[cfg(target_os="linux")]
pub(crate) fn root_registration_required(managed: bool) -> Result<bool> {
    if !managed { return Ok(false); }
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let file=std::fs::OpenOptions::new().read(true)
        .custom_flags(nix::libc::O_NOFOLLOW|nix::libc::O_CLOEXEC)
        .open("/neonvm/runtime/dsga-native-birth.json")?;
    let mut raw=Vec::new();file.take(16*1024+1).read_to_end(&mut raw)?;
    ensure!(raw.len()<=16*1024,"PG stderr policy exceeds fixed bound");
    parse_protocol_selection(&raw)

}

fn parse_protocol_selection(raw: &[u8]) -> Result<bool> {
    ensure!(raw.iter().find(|b| !b.is_ascii_whitespace())==Some(&b'{'),"PG stderr policy must be an object");
    #[derive(serde::Deserialize)]
    struct Selection {
        #[serde(default)]
        array_helper_protocol_version: u32,
    }
    let policy: Selection=serde_json::from_slice(raw)?;
    match policy.array_helper_protocol_version {
        0=>Ok(false),1=>Ok(true),_=>anyhow::bail!("unsupported PG stderr protocol selection"),
    }
}

#[derive(Debug)]
pub(crate) struct DrainReceipt {
    pub(crate) received_bytes: u64,
    pub(crate) emitted_groups: u64,
    pub(crate) max_line_bytes: usize,
    pub(crate) max_group_bytes: usize,
    pub(crate) eof_observed: bool,
}

struct BoundedLogs {
    line: Vec<u8>,
    group: Vec<u8>,
    timestamp: Regex,
    fragmented: bool,
    has_group: bool,
    receipt: DrainReceipt,
}

impl BoundedLogs {
    fn new() -> Self {
        Self {
            line: Vec::with_capacity(LINE_BYTES), group: Vec::with_capacity(GROUP_BYTES),
            timestamp: Regex::new(r"^\d+-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").expect("fixed timestamp regex"),
            fragmented: false, has_group: false,
            receipt: DrainReceipt { received_bytes: 0, emitted_groups: 0, max_line_bytes: 0, max_group_bytes: 0, eof_observed: false },
        }
    }

    fn flush(&mut self, output: &mut impl Write) -> io::Result<()> {
        if self.has_group {
            output.write_all(b"PG:")?;
            output.write_all(&self.group)?;
            output.write_all(b"\n")?;
            self.group.clear();
            self.has_group = false;
            self.receipt.emitted_groups = self.receipt.emitted_groups.checked_add(1).ok_or_else(|| io::Error::other("PG log counter overflow"))?;
        }
        Ok(())
    }

    fn append_line(&mut self, output: &mut impl Write, fragment: bool) -> io::Result<()> {
        let line = String::from_utf8_lossy(&self.line).into_owned();
        if self.timestamp.is_match(&line) || self.group.len() + line.len() + 3 > GROUP_BYTES {
            self.flush(output)?;
        }
        if self.has_group { self.group.extend_from_slice("\u{200B}".as_bytes()); }
        self.group.extend_from_slice(line.as_bytes());
        self.has_group = true;
        self.receipt.max_group_bytes = self.receipt.max_group_bytes.max(self.group.len());
        self.line.clear();
        if fragment { self.flush(output)?; }
        Ok(())
    }

    fn feed(&mut self, bytes: &[u8], output: &mut impl Write) -> io::Result<()> {
        self.receipt.received_bytes = self.receipt.received_bytes.checked_add(bytes.len() as u64).ok_or_else(|| io::Error::other("PG log byte overflow"))?;
        for &byte in bytes {
            if byte == b'\n' {
                if self.line.last() == Some(&b'\r') { self.line.pop(); }
                if !self.line.is_empty() || !self.fragmented { self.append_line(output, false)?; }
                self.fragmented = false;
                continue;
            }
            self.line.push(byte);
            self.receipt.max_line_bytes = self.receipt.max_line_bytes.max(self.line.len());
            if self.line.len() == LINE_BYTES {
                // 保留跨fragment边界的最多3个UTF8未完成字节，不把正常字符截成替换符。
                let keep = match std::str::from_utf8(&self.line) {
                    Err(e) if e.error_len().is_none() => self.line.len() - e.valid_up_to(),
                    _ => 0,
                };
                let tail = self.line[self.line.len()-keep..].to_vec();
                self.line.truncate(self.line.len()-keep);
                self.append_line(output, true)?;
                self.line.extend_from_slice(&tail);
                self.fragmented = true;
            }
        }
        Ok(())
    }

    fn finish(mut self, output: &mut impl Write) -> io::Result<DrainReceipt> {
        if !self.line.is_empty() { self.append_line(output, false)?; }
        self.flush(output)?;
        self.receipt.eof_observed = true;
        Ok(self.receipt)
    }
}

/// 只有真实read返回0才产生EOF receipt；错误/取消/输出失败不补成功。
/// 100ms只刷新已完成的组，不创建无界line/String/Vec<String>队列。
pub(crate) async fn drain_original_pg_stderr(file: File) -> Result<DrainReceipt> {
    let mut input = tokio::fs::File::from_std(file);
    let mut logs = BoundedLogs::new();
    let mut bytes = [0u8; READ_BYTES];
    loop {
        let read = tokio::time::timeout(Duration::from_millis(100), input.read(&mut bytes)).await;
        match read {
            Err(_) => logs.flush(&mut io::stderr().lock())?,
            Ok(result) => match result.context("original PG stderr read")? {
                0 => return Ok(logs.finish(&mut io::stderr().lock())?),
                n => logs.feed(&bytes[..n], &mut io::stderr().lock())?,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_protocol_selection_rejects_malformed_or_duplicate() -> Result<()> {
        assert!(!parse_protocol_selection(br#"{}"#)?);
        assert!(!parse_protocol_selection(br#"{"array_helper_protocol_version":0}"#)?);
        assert!(parse_protocol_selection(br#"{"array_helper_protocol_version":1}"#)?);
        for raw in [br#"{"array_helper_protocol_version":2}"#.as_slice(), br#"{"array_helper_protocol_version":null}"#, br#"{"array_helper_protocol_version":1,"array_helper_protocol_version":0}"#, br#"[]"#, br#"{"array_helper_protocol_version":"1"}"#] {
            assert!(parse_protocol_selection(raw).is_err());
        }
        Ok(())
    }

    #[test]
    fn unit_bounded_single_line_and_group() -> Result<()> {
        let mut logs = BoundedLogs::new();
        let mut output = Vec::new();
        for _ in 0..1024 { logs.feed(&[b'x'; 8192], &mut output)?; }
        logs.feed(b"\n", &mut output)?;
        for _ in 0..65536 { logs.feed(b"continuation\n", &mut output)?; }
        let result = logs.finish(&mut output)?;
        assert!(result.eof_observed);
        assert!(result.max_line_bytes <= LINE_BYTES);
        assert!(result.max_group_bytes <= GROUP_BYTES);
        assert_eq!(output.iter().filter(|&&x| x == b'x').count(), 8<<20);
        Ok(())
    }

    #[test]
    fn unit_ordinary_multiline_format_and_crlf() -> Result<()> {
        let mut logs = BoundedLogs::new();let mut output=Vec::new();
        logs.feed(b"2026-11-01 12:00:00 LOG: first\r\nDETAIL: detail\n2026-11-01 12:00:01 LOG: second\n", &mut output)?;
        logs.finish(&mut output)?;
        assert_eq!(String::from_utf8(output)?, "PG:2026-11-01 12:00:00 LOG: first\u{200B}DETAIL: detail\nPG:2026-11-01 12:00:01 LOG: second\n");
        Ok(())
    }

    #[test]
    fn unit_empty_lines_are_preserved() -> Result<()> {
        let mut logs=BoundedLogs::new();let mut output=Vec::new();
        logs.feed(b"\n\nline\n",&mut output)?;logs.finish(&mut output)?;
        assert_eq!(String::from_utf8(output)?,"PG:\u{200B}\u{200B}line\n");Ok(())
    }

    #[test]
    fn unit_utf8_fragment_boundary_and_output_failure() -> Result<()> {
        let mut logs=BoundedLogs::new();let mut output=Vec::new();
        let text="€".repeat(20000);
        for chunk in text.as_bytes().chunks(7) { logs.feed(chunk,&mut output)?; }
        logs.finish(&mut output)?;
        assert!(!String::from_utf8(output)?.contains('�'));
        struct Broken;
        impl Write for Broken { fn write(&mut self,_:&[u8])->io::Result<usize>{Err(io::Error::other("TEST_ONLY sink failure"))} fn flush(&mut self)->io::Result<()>{Ok(())} }
        let mut logs=BoundedLogs::new();logs.feed(b"unfinished",&mut Vec::new())?;
        assert!(logs.finish(&mut Broken).is_err());
        Ok(())
    }
}
