use std::borrow::Cow;

/// Borrowed `?v=` / `?encoding=` query parameters from a websocket request URI.
///
/// Only the two parameters the websocket handshake cares about are extracted;
/// everything else is ignored so callers never pay for a full map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UrlParams<'a> {
    /// Value of the `v` parameter (protocol version, e.g. `"1"`).
    pub v: Option<Cow<'a, str>>,
    /// Value of the `encoding` parameter (e.g. `"json"`).
    pub encoding: Option<Cow<'a, str>>,
}

/// Extract `v` and `encoding` from a request URI such as `/?v=1&encoding=json`.
///
/// Values are percent-decoded (`%XX`). A parameter that is missing, has no
/// `=`, or has an empty value yields `None`.
#[must_use]
pub fn get_url_params(uri: &str) -> UrlParams<'_> {
    let mut params = UrlParams::default();
    let Some((_, query)) = uri.split_once('?') else {
        return params;
    };

    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        match key {
            "v" => params.v = Some(percent_decode(value)),
            "encoding" => params.encoding = Some(percent_decode(value)),
            _ => {}
        }
    }

    params
}

const fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(byte.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(byte.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// Decode `%XX` sequences. Returns a borrowed slice when there is nothing
/// to decode so the common case allocates nothing.
fn percent_decode(segment: &str) -> Cow<'_, str> {
    if !segment.contains('%') {
        return Cow::Borrowed(segment);
    }
    let mut bytes = segment.as_bytes().iter().copied().peekable();
    let mut out: Vec<u8> = Vec::with_capacity(segment.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let mut lookahead = bytes.clone();
            if let (Some(hi), Some(lo)) = (
                lookahead.next().and_then(hex_val),
                lookahead.next().and_then(hex_val),
            ) {
                out.push(hi << 4 | lo);
                bytes = lookahead;
                continue;
            }
        }
        out.push(byte);
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}
