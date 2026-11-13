//! The version an MCP client sees in `initialize` as `serverInfo.version` (#1173).
//!
//! Releases are tagged (v0.1.N) without bumping Cargo.toml, so the Cargo version is always 0.1.0.
//! The Publish build passes the release tag in as `AGENTSHANGOUT_RELEASE`. A local build doesn't
//! set it, and then the Cargo version is what's reported.

/// The release number for a build tag: a leading `v` is dropped, and an empty or missing tag means
/// a local build, so the Cargo version is used.
pub fn from_tag<'a>(tag: Option<&'a str>, cargo: &'a str) -> &'a str {
    match tag.map(|t| t.trim().trim_start_matches('v')) {
        Some(t) if !t.is_empty() => t,
        _ => cargo,
    }
}

/// The version this binary reports to MCP clients.
pub fn server_version() -> &'static str {
    from_tag(option_env!("AGENTSHANGOUT_RELEASE"), env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_tag_is_reported_without_its_v() {
        assert_eq!(from_tag(Some("v0.1.738"), "0.1.0"), "0.1.738");
        assert_eq!(from_tag(Some("0.1.738"), "0.1.0"), "0.1.738");
    }

    #[test]
    fn a_local_build_falls_back_to_the_cargo_version() {
        assert_eq!(from_tag(None, "0.1.0"), "0.1.0");
        assert_eq!(from_tag(Some(""), "0.1.0"), "0.1.0");
        assert_eq!(from_tag(Some("  "), "0.1.0"), "0.1.0");
        assert_eq!(from_tag(Some("v"), "0.1.0"), "0.1.0");
    }

    #[test]
    fn a_local_build_reports_the_cargo_version() {
        // This test runs without AGENTSHANGOUT_RELEASE set, as a local build does.
        if option_env!("AGENTSHANGOUT_RELEASE").is_none() {
            assert_eq!(server_version(), env!("CARGO_PKG_VERSION"));
        }
    }
}
