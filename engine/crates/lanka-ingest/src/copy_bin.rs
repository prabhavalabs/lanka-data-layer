//! Postgres binary COPY encoding (`COPY … FROM STDIN (FORMAT binary)`).
//!
//! Faster than CSV for numeric rows (no float formatting or parsing, no
//! quoting) and exact: floats travel as IEEE-754 bits. Only the types the
//! staging tables use are implemented.
//!
//! Format: 19-byte signature + flags + header extension, then per tuple an
//! `int16` field count and per field an `int32` length (−1 = NULL) followed
//! by the value in network byte order; trailer `int16 −1`.

use chrono::NaiveDate;

const SIGNATURE: &[u8] = b"PGCOPY\n\xff\r\n\0";

/// Days from 1970-01-01 to the Postgres epoch 2000-01-01.
const PG_EPOCH_DAYS: i32 = 10_957;

/// Builds a binary COPY stream in memory, flushed in chunks by the caller.
#[derive(Debug, Default)]
pub struct BinCopy {
    pub buf: Vec<u8>,
}

impl BinCopy {
    #[must_use]
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(1 << 20);
        buf.extend_from_slice(SIGNATURE);
        buf.extend_from_slice(&0i32.to_be_bytes()); // flags
        buf.extend_from_slice(&0i32.to_be_bytes()); // header extension length
        Self { buf }
    }

    pub fn row(&mut self, fields: i16) {
        self.buf.extend_from_slice(&fields.to_be_bytes());
    }

    pub fn null(&mut self) {
        self.buf.extend_from_slice(&(-1i32).to_be_bytes());
    }

    fn field(&mut self, bytes: &[u8]) {
        let len = i32::try_from(bytes.len()).unwrap_or(i32::MAX);
        self.buf.extend_from_slice(&len.to_be_bytes());
        self.buf.extend_from_slice(bytes);
    }

    pub fn i64(&mut self, v: i64) {
        self.field(&v.to_be_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.field(&v.to_be_bytes());
    }

    pub fn i16(&mut self, v: i16) {
        self.field(&v.to_be_bytes());
    }

    pub fn f64(&mut self, v: Option<f64>) {
        match v {
            Some(x) => self.field(&x.to_be_bytes()),
            None => self.null(),
        }
    }

    pub fn f32(&mut self, v: Option<f32>) {
        match v {
            Some(x) => self.field(&x.to_be_bytes()),
            None => self.null(),
        }
    }

    pub fn opt_i64(&mut self, v: Option<i64>) {
        match v {
            Some(x) => self.i64(x),
            None => self.null(),
        }
    }

    pub fn opt_i32(&mut self, v: Option<i32>) {
        match v {
            Some(x) => self.i32(x),
            None => self.null(),
        }
    }

    /// `"char"`: one byte.
    pub fn char1(&mut self, c: u8) {
        self.field(&[c]);
    }

    pub fn date(&mut self, d: NaiveDate) {
        let days = i32::try_from(
            d.signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default())
                .num_days(),
        )
        .unwrap_or(i32::MAX);
        self.i32(days - PG_EPOCH_DAYS);
    }

    pub fn text(&mut self, s: Option<&str>) {
        match s {
            Some(v) => self.field(v.as_bytes()),
            None => self.null(),
        }
    }

    /// `jsonb`: version byte 1 + JSON text.
    pub fn jsonb(&mut self, json: &str) {
        let len = i32::try_from(json.len() + 1).unwrap_or(i32::MAX);
        self.buf.extend_from_slice(&len.to_be_bytes());
        self.buf.push(1);
        self.buf.extend_from_slice(json.as_bytes());
    }

    pub fn bytea(&mut self, b: &[u8]) {
        self.field(b);
    }

    /// Appends the trailer.
    pub fn finish(&mut self) {
        self.buf.extend_from_slice(&(-1i16).to_be_bytes());
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Hands out the buffered bytes and keeps an empty buffer (the
    /// signature is written once, at the start of the stream).
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_header_fields_and_dates() {
        let mut c = BinCopy::new();
        assert_eq!(&c.buf[..11], SIGNATURE);
        assert_eq!(c.len(), 19);
        c.row(2);
        c.date(NaiveDate::from_ymd_opt(2000, 1, 2).expect("date"));
        c.f64(None);
        c.finish();
        assert_eq!(
            &c.buf[19..],
            &[
                0, 2, 0, 0, 0, 4, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff
            ]
        );
    }
}
