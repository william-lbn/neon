use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;
use zstd::stream::raw::{DParameter, Decoder, Operation};

/// 限额只能来自本Compute原维护连接的原生只读统计；调用方不能从HTTP
/// Content-Length或压缩帧声明取得许可。旧扩展缺少统计时停止可选预热。
#[derive(Clone, Copy, Debug)]
pub(super) struct Limit {
    decoded: usize,
    encoded: usize,
}

impl Limit {
    pub(super) fn from_native(bytes: Option<i64>) -> Result<Option<Self>> {
        match bytes {
            None | Some(0) => Ok(None),
            Some(bytes) => {
                // PostgreSQL MaxAllocSize及bytea头的格式边界，不是独立配置额度。
                if !(1..=((1_i64 << 30) - 1 - 4)).contains(&bytes) {
                    bail!("invalid native LFC prewarm metadata bound");
                }
                let decoded = usize::try_from(bytes)?;
                Ok(Some(Self {
                    decoded,
                    encoded: zstd::zstd_safe::compress_bound(decoded),
                }))
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum Read {
    Ready(Vec<u8>),
    TooLarge,
    Cancelled,
}

pub(super) async fn response(
    response: reqwest::Response,
    limit: Limit,
    token: &CancellationToken,
) -> Result<Read> {
    let length = response.content_length();
    let stream = futures::stream::try_unfold(response, |mut response| async {
        response
            .chunk()
            .await
            .map(|chunk| chunk.map(|chunk| (chunk, response)))
            .map_err(std::io::Error::other)
    });
    collect(stream, length, limit.encoded, token).await
}

/// 长度和容量都服从同一原生包络；只在空间不足时有界倍增，避免恶意小
/// HTTP块或固定解码块使每次append都重新分配/搬迁。checked_add先拒绝
/// 越界，checked_mul溢出时只取已有maximum，绝不把扩容当成新额度。
fn append_bounded(output: &mut Vec<u8>, bytes: &[u8], maximum: usize) -> Result<bool> {
    let Some(needed) = output.len().checked_add(bytes.len()) else {
        return Ok(false);
    };
    if needed > maximum {
        return Ok(false);
    }
    if needed > output.capacity() {
        let next = output
            .capacity()
            .checked_mul(2)
            .unwrap_or(maximum)
            .max(needed)
            .min(maximum);
        output.try_reserve_exact(next - output.len())?;
    }
    output.extend_from_slice(bytes);
    Ok(true)
}

async fn collect<S>(
    stream: S,
    content_length: Option<u64>,
    maximum: usize,
    token: &CancellationToken,
) -> Result<Read>
where
    S: Stream<Item = std::io::Result<Bytes>>,
{
    if token.is_cancelled() {
        return Ok(Read::Cancelled);
    }
    if content_length.is_some_and(|length| length > maximum as u64) {
        return Ok(Read::TooLarge);
    }
    futures::pin_mut!(stream);
    let mut output = Vec::new();
    loop {
        let next = tokio::select! {
            biased;
            _ = token.cancelled() => return Ok(Read::Cancelled),
            next = stream.next() => next,
        };
        let Some(chunk) = next else {
            if content_length.is_some_and(|length| length != output.len() as u64) {
                bail!("truncated LFC metadata response");
            }
            return Ok(Read::Ready(output));
        };
        let chunk = chunk.context("reading LFC metadata response")?;
        if !append_bounded(&mut output, &chunk, maximum)? {
            return Ok(Read::TooLarge);
        }
        // 持续Ready的小片段也必须给原取消任务调度机会，不能假设网络
        // await必然挂起；仍在当前预热任务内等待，不派生后台读取任务。
        tokio::task::yield_now().await;
    }
}

pub(super) async fn decode(input: &[u8], limit: Limit, token: &CancellationToken) -> Result<Read> {
    if token.is_cancelled() {
        return Ok(Read::Cancelled);
    }
    if input.len() > limit.encoded {
        return Ok(Read::TooLarge);
    }
    let mut decoder = Decoder::new()?;
    // 旧生产者是默认level=3的Zstd流编码器（未知输入长度，windowLog=21）。
    // 保留其2MiB格式兼容窗，同时限制恶意帧声明；大输入只按原接收上界
    // 向上取整，绝不继承解码库默认128MiB窗口。原生边界单独限制输出。
    let window_log = (usize::BITS - (limit.decoded - 1).leading_zeros()).max(21);
    decoder.set_parameter(DParameter::WindowLogMax(window_log))?;
    let mut output = Vec::new();
    let mut consumed = 0;
    // 这里只是有界调度/取消粒度，不是独立的用户额度。
    let mut buffer = [0_u8; 8192];
    loop {
        if token.is_cancelled() {
            return Ok(Read::Cancelled);
        }
        let status = decoder
            .run_on_buffers(&input[consumed..], &mut buffer)
            .context("decoding LFC metadata")?;
        consumed += status.bytes_read;
        if !append_bounded(&mut output, &buffer[..status.bytes_written], limit.decoded)? {
            return Ok(Read::TooLarge);
        }
        if status.remaining == 0 {
            if consumed != input.len() {
                bail!("trailing data after LFC metadata frame");
            }
            return Ok(Read::Ready(output));
        }
        if status.bytes_read == 0 && status.bytes_written == 0 {
            bail!("truncated LFC metadata frame");
        }
        // 同一任务仍处于Prewarming；取消后不将尚在解码的工作移到后台。
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_compression::tokio::bufread::{ZstdDecoder, ZstdEncoder};
    use tokio::io::AsyncReadExt;

    fn limit(bytes: usize) -> Limit {
        Limit::from_native(Some(bytes as i64)).unwrap().unwrap()
    }

    fn bytes(read: Read) -> Vec<u8> {
        match read {
            Read::Ready(bytes) => bytes,
            other => panic!("expected complete bytes, got {other:?}"),
        }
    }

    #[test]
    fn original_native_limit_is_required() {
        assert!(Limit::from_native(None).unwrap().is_none());
        assert!(Limit::from_native(Some(0)).unwrap().is_none());
        for invalid in [-1, (1 << 30) - 4, i64::MAX] {
            assert!(Limit::from_native(Some(invalid)).is_err());
        }
        assert!(Limit::from_native(Some((1 << 30) - 5)).is_ok());
    }

    #[test]
    fn tiny_chunks_grow_amortized_without_exceeding_original_bound() {
        for maximum in [1, 3, 8193, 65537] {
            let mut output = Vec::new();
            let mut allocations = 0;
            let mut previous_capacity = 0;
            for _ in 0..maximum {
                assert!(append_bounded(&mut output, &[0x53], maximum).unwrap());
                assert!(output.capacity() <= maximum);
                if output.capacity() != previous_capacity {
                    allocations += 1;
                    previous_capacity = output.capacity();
                }
            }
            // 单字节分片最多首块+log2有界倍增，不退回每块扩容。
            assert!(allocations <= usize::BITS - (maximum - 1).leading_zeros() + 1);
            let before = output.clone();
            assert!(!append_bounded(&mut output, &[0x54], maximum).unwrap());
            assert_eq!(output, before);
            assert_eq!(output.capacity(), previous_capacity);
        }
    }

    #[tokio::test]
    async fn fragmented_network_retains_bounded_capacity() {
        let maximum = 65537;
        let stream = futures::stream::iter((0..maximum).map(|_| Ok(Bytes::from_static(b"m"))));
        let output = bytes(
            collect(stream, None, maximum, &CancellationToken::new())
                .await
                .unwrap(),
        );
        assert_eq!(output.len(), maximum);
        assert!(output.capacity() <= maximum);
    }

    #[tokio::test]
    async fn network_exact_limit_and_unknown_length() {
        for length in [None, Some(6)] {
            let stream = futures::stream::iter([
                Ok(Bytes::from_static(b"abc")),
                Ok(Bytes::from_static(b"def")),
            ]);
            assert_eq!(
                bytes(
                    collect(stream, length, 6, &CancellationToken::new())
                        .await
                        .unwrap()
                ),
                b"abcdef"
            );
        }
    }

    #[tokio::test]
    async fn response_adapter_checks_real_body_and_declared_length() {
        let cap = limit(1);
        let body = vec![0x53; cap.encoded];
        let response_body = http::Response::builder()
            .header(http::header::CONTENT_LENGTH, body.len())
            .body(body.clone())
            .unwrap();
        assert_eq!(
            bytes(
                response(response_body.into(), cap, &CancellationToken::new())
                    .await
                    .unwrap()
            ),
            body
        );
        let response_body = http::Response::builder()
            .body(vec![0x53; cap.encoded + 1])
            .unwrap();
        assert!(matches!(
            response(response_body.into(), cap, &CancellationToken::new())
                .await
                .unwrap(),
            Read::TooLarge
        ));
    }

    #[tokio::test]
    async fn network_declared_oversize_is_never_polled() {
        let stream =
            futures::stream::poll_fn(|_| -> std::task::Poll<Option<std::io::Result<Bytes>>> {
                panic!("body must not be polled after oversized Content-Length")
            });
        assert!(matches!(
            collect(stream, Some(7), 6, &CancellationToken::new())
                .await
                .unwrap(),
            Read::TooLarge
        ));
    }

    #[tokio::test]
    async fn chunked_body_cannot_bypass_bound() {
        let stream = futures::stream::iter([
            Ok(Bytes::from_static(b"abc")),
            Ok(Bytes::from_static(b"defg")),
        ]);
        assert!(matches!(
            collect(stream, None, 6, &CancellationToken::new())
                .await
                .unwrap(),
            Read::TooLarge
        ));
    }

    #[tokio::test]
    async fn truncated_or_failed_network_is_not_complete() {
        let stream = futures::stream::iter([Ok(Bytes::from_static(b"abc"))]);
        assert!(
            collect(stream, Some(4), 6, &CancellationToken::new())
                .await
                .is_err()
        );
        let stream =
            futures::stream::iter([Err(std::io::Error::other("original network failure"))]);
        assert!(
            collect(stream, None, 6, &CancellationToken::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn blocked_network_remains_cancellable() {
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancel.cancel();
        });
        assert!(matches!(
            collect(futures::stream::pending(), None, 6, &token)
                .await
                .unwrap(),
            Read::Cancelled
        ));
    }

    #[tokio::test]
    async fn continuously_ready_fragments_remain_cancellable() {
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            cancel.cancel();
        });
        let stream = futures::stream::iter((0..65537).map(|_| Ok(Bytes::from_static(b"m"))));
        assert!(matches!(
            collect(stream, None, 65537, &token).await.unwrap(),
            Read::Cancelled
        ));
    }

    async fn old_encoder(data: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        ZstdEncoder::new(data)
            .read_to_end(&mut encoded)
            .await
            .unwrap();
        encoded
    }

    #[tokio::test]
    async fn original_encoder_accepts_exact_output_and_small_target() {
        for size in [1, 8192, 65537] {
            let data = vec![0x36; size];
            let encoded = old_encoder(&data).await;
            assert_eq!(
                bytes(
                    decode(&encoded, limit(size), &CancellationToken::new())
                        .await
                        .unwrap()
                ),
                data
            );
        }
    }

    #[tokio::test]
    async fn original_unbounded_decoder_accepts_over_budget_bomb_but_new_reader_refuses() {
        let data = vec![0; 256 << 10];
        let encoded = old_encoder(&data).await;
        let cap = limit(1024);
        assert!(encoded.len() < cap.encoded);
        // 保留原行为反例：编码体很小，旧read_to_end仍完整分配越界输出。
        let mut old = Vec::new();
        ZstdDecoder::new(encoded.as_slice())
            .read_to_end(&mut old)
            .await
            .unwrap();
        assert_eq!(old.len(), data.len());
        assert!(old.len() > cap.decoded);
        assert!(matches!(
            decode(&encoded, cap, &CancellationToken::new())
                .await
                .unwrap(),
            Read::TooLarge
        ));
    }

    #[tokio::test]
    async fn incomplete_corrupt_or_trailing_frame_is_not_complete() {
        let data = vec![31; 16384];
        let encoded = old_encoder(&data).await;
        let cap = limit(data.len());
        for cut in [0, 1, encoded.len() - 1] {
            assert!(
                decode(&encoded[..cut], cap, &CancellationToken::new())
                    .await
                    .is_err()
            );
        }
        assert!(
            decode(b"not zstd", cap, &CancellationToken::new())
                .await
                .is_err()
        );
        let mut trailing = encoded;
        trailing.push(0);
        assert!(
            decode(&trailing, cap, &CancellationToken::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn oversized_window_is_rejected_before_output() {
        // 标准Zstd帧：未知内容长度、16MiB window、最后一个空raw块。
        // 输出为零也不能让帧头获取大于原生产兼容窗/目标上界的decoder内存。
        let frame = [0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x70, 0x01, 0x00, 0x00];
        let mut old = Vec::new();
        ZstdDecoder::new(frame.as_slice())
            .read_to_end(&mut old)
            .await
            .unwrap();
        assert!(old.is_empty());
        assert!(
            decode(&frame, limit(1024), &CancellationToken::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn decode_cancellation_is_checked_between_bounded_chunks() {
        let data = vec![63; 1 << 20];
        let encoded = old_encoder(&data).await;
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancel.cancel();
        });
        assert!(matches!(
            decode(&encoded, limit(data.len()), &token).await.unwrap(),
            Read::Cancelled
        ));
    }
}
