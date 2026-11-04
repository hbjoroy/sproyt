//! Bounded validation for download-only documents. No archive extraction,
//! document renderer or uploaded executable/HTML/SVG serving is introduced.

const ZIP_TAIL: usize = 65_557;

pub(super) struct Validator {
    kind: Kind,
    carry: Vec<u8>,
    tail: Vec<u8>,
    length: u64,
}

enum Kind {
    None,
    Pdf,
    Zip,
    Text,
}

impl Validator {
    pub(super) fn new(content_type: &str) -> Self {
        Self {
            kind: match content_type {
                "application/pdf" => Kind::Pdf,
                "application/zip" => Kind::Zip,
                "text/plain" | "text/csv" => Kind::Text,
                _ => Kind::None,
            },
            carry: Vec::new(),
            tail: Vec::new(),
            length: 0,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> bool {
        self.length += bytes.len() as u64;
        if matches!(self.kind, Kind::Text) {
            let mut next = std::mem::take(&mut self.carry);
            next.extend_from_slice(bytes);
            let (valid, remaining) = match std::str::from_utf8(&next) {
                Ok(text) => (text, &[][..]),
                Err(error) if error.error_len().is_none() => (
                    std::str::from_utf8(&next[..error.valid_up_to()])
                        .expect("validated UTF-8 prefix"),
                    &next[error.valid_up_to()..],
                ),
                Err(_) => return false,
            };
            if valid
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            {
                return false;
            }
            self.carry.extend_from_slice(remaining);
        }
        if matches!(self.kind, Kind::Pdf | Kind::Zip) {
            let budget = if matches!(self.kind, Kind::Pdf) {
                1024
            } else {
                ZIP_TAIL
            };
            if bytes.len() >= budget {
                self.tail.clear();
                self.tail.extend_from_slice(&bytes[bytes.len() - budget..]);
            } else {
                let discard = (self.tail.len() + bytes.len()).saturating_sub(budget);
                self.tail.drain(..discard);
                self.tail.extend_from_slice(bytes);
            }
        }
        true
    }

    pub(super) fn finish(self) -> bool {
        match self.kind {
            Kind::None => true,
            Kind::Text => self.carry.is_empty(),
            Kind::Pdf => self.tail.trim_ascii_end().ends_with(b"%%EOF"),
            Kind::Zip => {
                let tail = &self.tail;
                // EOCD has a fixed 22-byte prefix and at most 65535 comment
                // bytes. Reject multi-disk/ZIP64 and inconsistent bounds, while
                // never inflating or interpreting archive entries.
                (0..tail.len().saturating_sub(21)).rev().any(|offset| {
                    if !tail[offset..].starts_with(b"PK\x05\x06") {
                        return false;
                    }
                    let end = &tail[offset..];
                    let word = |at| u16::from_le_bytes([end[at], end[at + 1]]);
                    let number = |at| {
                        u32::from_le_bytes(end[at..at + 4].try_into().expect("EOCD number")) as u64
                    };
                    let entries = word(10);
                    let directory_size = number(12);
                    let directory_at = number(16);
                    end.len() == 22 + usize::from(word(20))
                        && word(4) == 0
                        && word(6) == 0
                        && word(8) == entries
                        && entries != u16::MAX
                        && directory_at.saturating_add(directory_size)
                            <= self.length - end.len() as u64
                        && (entries == 0 || directory_size > 0)
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_utf8_crosses_chunks_and_rejects_binary_or_incomplete_text() {
        let mut text = Validator::new("text/plain");
        assert!(text.push(b"hello \xe2"));
        assert!(text.push(b"\x82\xac\n"));
        assert!(text.finish());
        let mut text = Validator::new("text/csv");
        assert!(!text.push(b"a,b\0"));
        let mut text = Validator::new("text/plain");
        assert!(text.push(b"\xe2"));
        assert!(!text.finish());
        let mut text = Validator::new("text/plain");
        assert!(!text.push(b"\xffbinary"));
    }
    #[test]
    fn attachment_pdf_and_zip_require_bounded_complete_terminators() {
        let mut pdf = Validator::new("application/pdf");
        assert!(pdf.push(b"%PDF-1.7\nbody\n%%EOF\n"));
        assert!(pdf.finish());
        let mut pdf = Validator::new("application/pdf");
        assert!(pdf.push(b"%PDF-1.7\ntruncated"));
        assert!(!pdf.finish());
        let mut zip = Validator::new("application/zip");
        let mut empty = b"PK\x05\x06".to_vec();
        empty.resize(22, 0);
        assert!(zip.push(&empty));
        assert!(zip.finish());
        let mut zip = Validator::new("application/zip");
        assert!(zip.push(b"PK\x03\x04fake"));
        assert!(!zip.finish());
    }
}
