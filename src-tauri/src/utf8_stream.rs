#[derive(Debug, Default)]
pub struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, output: &mut String, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);

        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    output.push_str(valid);
                    self.pending.clear();
                    return;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        let valid = std::str::from_utf8(&self.pending[..valid_up_to])
                            .expect("UTF-8 validator returned an invalid prefix");
                        output.push_str(valid);
                    }

                    let Some(error_len) = error.error_len() else {
                        self.pending.drain(..valid_up_to);
                        return;
                    };

                    output.push('\u{fffd}');
                    self.pending.drain(..valid_up_to + error_len);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn finish(mut self, output: &mut String) {
        if !self.pending.is_empty() {
            output.push_str(&String::from_utf8_lossy(&self.pending));
            self.pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8StreamDecoder;

    #[test]
    fn preserves_multibyte_characters_split_across_chunks() {
        let input = "修改中文标题：时尚代码".as_bytes();
        let mut decoder = Utf8StreamDecoder::new();
        let mut output = String::new();

        for byte in input {
            decoder.push(&mut output, std::slice::from_ref(byte));
        }
        decoder.finish(&mut output);

        assert_eq!(output, "修改中文标题：时尚代码");
        assert!(!output.contains('\u{fffd}'));
    }

    #[test]
    fn replaces_only_genuinely_invalid_utf8() {
        let mut decoder = Utf8StreamDecoder::new();
        let mut output = String::new();

        decoder.push(&mut output, b"ok\xffdone");
        decoder.finish(&mut output);

        assert_eq!(output, "ok\u{fffd}done");
    }
}
