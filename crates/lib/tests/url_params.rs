use rsrpc::url_params::get_url_params;

#[test]
fn extracts_v_and_encoding() {
  let params = get_url_params("/path?v=1&encoding=json");
  assert_eq!(params.v.as_deref(), Some("1"));
  assert_eq!(params.encoding.as_deref(), Some("json"));
}

#[test]
fn percent_encoded_values_decode() {
  let params = get_url_params("/?v=%31&encoding=j%20s");
  assert_eq!(params.v.as_deref(), Some("1"));
  assert_eq!(params.encoding.as_deref(), Some("j s"));
}

#[test]
fn missing_query_yields_none() {
  let params = get_url_params("/path");
  assert_eq!(params.v, None);
  assert_eq!(params.encoding, None);
}

#[test]
fn valueless_params_ignored() {
  let params = get_url_params("/?v&encoding");
  assert_eq!(params.v, None);
  assert_eq!(params.encoding, None);
}

#[test]
fn ignores_unrelated_params_and_empty_values() {
  let params = get_url_params("/?foo=bar&v=&encoding=json");
  assert_eq!(params.v, None);
  assert_eq!(params.encoding.as_deref(), Some("json"));
}
