//! Wire format on the Unix socket: one request and one response per
//! connection, each a frame of
//!
//! ```text
//! u32 BE header length | header JSON | u64 BE payload length | payload bytes
//! ```
//!
//! The request header is an [`Op`](crate::ops::Op); the payload is the
//! document. The response header is a [`Reply`]; the payload is the tool
//! output (XHTML, PNG, hOCR or text).

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest accepted header (an op is a few dozen bytes).
pub const MAX_HEADER: u32 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    header: &[u8],
    payload: &[u8],
) -> Result<()> {
    let hl = u32::try_from(header.len()).context("header too large")?;
    ensure!(hl <= MAX_HEADER, "header too large");
    w.write_all(&hl.to_be_bytes()).await?;
    w.write_all(header).await?;
    w.write_all(&(payload.len() as u64).to_be_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await?;
    Ok(())
}

/// Reads one frame, refusing payloads over `max_payload` bytes before
/// allocating them.
pub async fn read_frame<R: AsyncRead + Unpin>(
    r: &mut R,
    max_payload: u64,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4)
        .await
        .context("reading header length")?;
    let hl = u32::from_be_bytes(b4);
    ensure!(hl <= MAX_HEADER, "header of {hl} bytes refused");
    let mut header = vec![0u8; hl as usize];
    r.read_exact(&mut header).await.context("reading header")?;
    let mut b8 = [0u8; 8];
    r.read_exact(&mut b8)
        .await
        .context("reading payload length")?;
    let pl = u64::from_be_bytes(b8);
    ensure!(
        pl <= max_payload,
        "payload of {pl} bytes exceeds {max_payload}"
    );
    let mut payload = vec![0u8; usize::try_from(pl)?];
    r.read_exact(&mut payload)
        .await
        .context("reading payload")?;
    Ok((header, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip_and_caps_hold() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"{\"op\":\"pdf_info\"}", b"%PDF-1.4")
            .await
            .expect("write");
        let (h, p) = read_frame(&mut buf.as_slice(), 100).await.expect("read");
        assert_eq!(h, b"{\"op\":\"pdf_info\"}");
        assert_eq!(p, b"%PDF-1.4");
        assert!(read_frame(&mut buf.as_slice(), 4).await.is_err());
        let mut huge = (MAX_HEADER + 1).to_be_bytes().to_vec();
        huge.extend([0; 16]);
        assert!(read_frame(&mut huge.as_slice(), 100).await.is_err());
    }
}
