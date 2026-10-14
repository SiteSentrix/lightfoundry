//! Assembling lines from a byte stream, with a bound on how much is held.

/// Longest line kept while waiting for its newline.
pub const MAX_LINE_BUFFER: usize = 4096;

#[derive(Debug, Default)]
pub struct LineBuffer {
    bytes: Vec<u8>,
}

impl LineBuffer {
    /// Adds a byte; returns a finished line (without its terminator) when `byte` ends one. A line
    /// that runs on past [`MAX_LINE_BUFFER`] (a wrong baud rate, line noise) is dropped and
    /// collection starts over, so the next newline still ends a line.
    pub fn push(&mut self, byte: u8) -> Option<String> {
        if byte == b'\n' {
            let line = String::from_utf8_lossy(&self.bytes).trim().to_string();
            self.bytes.clear();
            return Some(line);
        }
        if self.bytes.len() >= MAX_LINE_BUFFER {
            self.bytes.clear();
        }
        self.bytes.push(byte);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(buf: &mut LineBuffer, data: &[u8]) -> Vec<String> {
        data.iter().filter_map(|b| buf.push(*b)).collect()
    }

    #[test]
    fn lines_split_on_newlines_and_are_trimmed() {
        let mut b = LineBuffer::default();
        assert_eq!(feed(&mut b, b"ok\r\n<Idle>\nerr"), vec!["ok", "<Idle>"]);
        assert_eq!(feed(&mut b, b"or:3\n"), vec!["error:3"], "a line may arrive in pieces");
        assert_eq!(feed(&mut b, b"\n\n"), vec!["", ""]);
    }

    #[test]
    fn noise_without_newlines_cannot_silence_the_link_for_good() {
        let mut b = LineBuffer::default();
        let noise = vec![b'x'; MAX_LINE_BUFFER * 3 + 17];
        assert!(feed(&mut b, &noise).is_empty());
        let lines = feed(&mut b, b"\nok\n");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "valid lines after the noise still arrive: {lines:?}");
        assert!(lines[0].len() <= MAX_LINE_BUFFER);
    }

    #[test]
    fn invalid_utf8_is_replaced_not_fatal() {
        let mut b = LineBuffer::default();
        assert_eq!(feed(&mut b, &[b'o', 0xff, b'k', b'\n']).len(), 1);
    }
}
