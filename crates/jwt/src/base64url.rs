//! Pure, dependency-free Base64 and Base64URL decoder (RFC 4648).

/// Decode a Base64 or Base64URL string (with or without `=` padding) into bytes.
pub fn decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut buf = Vec::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            'A'..='Z' => buf.push(ch as u8 - b'A'),
            'a'..='z' => buf.push(ch as u8 - b'a' + 26),
            '0'..='9' => buf.push(ch as u8 - b'0' + 52),
            '+' | '-' => buf.push(62),
            '/' | '_' => buf.push(63),
            '=' | '\r' | '\n' | ' ' => continue,
            _ => return Err("invalid character in base64 string"),
        }
    }

    let mut out = Vec::with_capacity((buf.len() * 3) / 4);
    let (chunks, remainder) = buf.as_chunks::<4>();

    for chunk in chunks {
        let b0 = chunk[0];
        let b1 = chunk[1];
        let b2 = chunk[2];
        let b3 = chunk[3];

        out.push((b0 << 2) | (b1 >> 4));
        out.push((b1 << 4) | (b2 >> 2));
        out.push((b2 << 6) | b3);
    }

    match remainder.len() {
        0 => {}
        1 => return Err("truncated base64 sequence"),
        2 => {
            let b0 = remainder[0];
            let b1 = remainder[1];
            out.push((b0 << 2) | (b1 >> 4));
        }
        3 => {
            let b0 = remainder[0];
            let b1 = remainder[1];
            let b2 = remainder[2];
            out.push((b0 << 2) | (b1 >> 4));
            out.push((b1 << 4) | (b2 >> 2));
        }
        _ => unreachable!(),
    }

    Ok(out)
}

/// Decode Base64/Base64URL into a UTF-8 String.
pub fn decode_to_string(input: &str) -> Result<String, String> {
    let bytes = decode(input).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8 sequence: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_standard_base64() {
        assert_eq!(decode_to_string("aGVsbG8=").unwrap(), "hello");
        assert_eq!(decode_to_string("aGVsbG8").unwrap(), "hello");
        assert_eq!(decode_to_string("d29ybGQ=").unwrap(), "world");
    }

    #[test]
    fn decode_base64url_with_special_chars() {
        // {"alg":"HS256","typ":"JWT"} -> eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9
        let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let decoded = decode_to_string(header).unwrap();
        assert_eq!(decoded, r#"{"alg":"HS256","typ":"JWT"}"#);

        // Test URL safe chars '-' and '_'
        assert!(decode("-_--").is_ok());
    }

    #[test]
    fn decode_invalid_characters() {
        assert!(decode("not!valid!").is_err());
    }
}
