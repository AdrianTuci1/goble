use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

pub(super) fn offset_to_chrono(dt: ::time::OffsetDateTime) -> DateTime<Utc> {
    DateTime::from_timestamp(dt.unix_timestamp(), dt.nanosecond()).expect("valid timestamp")
}

pub(super) fn parse_utf8_der(bytes: &[u8]) -> Result<String> {
    if bytes.len() < 2 || bytes[0] != 0x0c {
        anyhow::bail!("expected DER UTF8String");
    }
    let (len, consumed) = parse_ber_length(&bytes[1..])?;
    let start = 1 + consumed;
    if start + len > bytes.len() {
        anyhow::bail!("DER UTF8String length out of bounds");
    }
    String::from_utf8(bytes[start..start + len].to_vec())
        .map_err(|e| anyhow::anyhow!("invalid UTF8String: {e}"))
}

fn parse_ber_length(bytes: &[u8]) -> Result<(usize, usize)> {
    if bytes.is_empty() {
        anyhow::bail!("missing length octet");
    }
    let first = bytes[0];
    if first & 0x80 == 0 {
        return Ok((first as usize, 1));
    }
    let num_bytes = (first & 0x7f) as usize;
    if num_bytes == 0 || num_bytes > 8 || bytes.len() < 1 + num_bytes {
        anyhow::bail!("invalid long-form length");
    }
    let mut len = 0usize;
    for b in &bytes[1..1 + num_bytes] {
        len = len
            .checked_shl(8)
            .and_then(|l| l.checked_add(*b as usize))
            .context("length overflow")?;
    }
    Ok((len, 1 + num_bytes))
}

/// Parse a PEM string into DER bytes.
pub fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let (_, parsed) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to parse PEM: {e}"))?;
    Ok(parsed.contents)
}
