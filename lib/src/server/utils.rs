/// Payload sent to clients right after a successful handshake.
///
/// The `data.user` block is a fixed placeholder (arRPC); per-connection
/// fields are filled in by the client connector when forwarding activity.
/// Keep this string byte-stable: `ipc_framing` asserts the READY reply
/// contains `"READY"` and the pre-encoded frame in `ipc_utils` must match.
pub(crate) static CONNECTION_RESPONSE: &str = r#"
{
  "cmd": "DISPATCH",
  "evt": "READY",
  "data": {
    "v": 1,
    "user": {
      "id": "1045800378228281345",
      "username": "arRPC",
      "discriminator": "0000",
      "avatar": "cfefa4d9839fb4bdf030f91c2a13e95c",
      "flags": 0,
      "premium_type": 0
    },
    "config": {
      "api_endpoint": "//discord.com/api",
      "cdn_host": "cdn.discordapp.com",
      "environment": "production"
    }
  }
}
"#;

/// Historical misspelling of [`CONNECTION_RESPONSE`], kept so out-of-slice
/// callers (`lib.rs`, `websocket.rs`) keep compiling until they migrate.
/// New code MUST use [`CONNECTION_RESPONSE`].
#[deprecated(since = "0.28.0", note = "use CONNECTION_RESPONSE instead")]
#[allow(dead_code)]
pub(crate) static CONNECTION_REPONSE: &str = CONNECTION_RESPONSE;
