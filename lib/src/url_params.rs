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
pub fn get_url_params(uri: &str) -> UrlParams<'_> {
  let mut params = UrlParams::default();
  let query = match uri.split_once('?') {
    Some((_, query)) => query,
    None => return params,
  };

  for pair in query.split('&') {
    let (key, value) = match pair.split_once('=') {
      Some(parts) => parts,
      None => continue,
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

fn hex_val(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    b'A'..=b'F' => Some(byte - b'A' + 10),
    _ => None,
  }
}

/// Decode `%XX` sequences. Returns a borrowed slice when there is nothing
/// to decode so the common case allocates nothing.
fn percent_decode(segment: &str) -> Cow<'_, str> {
  if !segment.contains('%') {
    return Cow::Borrowed(segment);
  }
  let bytes = segment.as_bytes();
  let mut out: Vec<u8> = Vec::with_capacity(segment.len());
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i] == b'%' && i + 3 <= bytes.len() {
      if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
        out.push(hi << 4 | lo);
        i += 3;
        continue;
      }
    }
    out.push(bytes[i]);
    i += 1;
  }
  Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}
