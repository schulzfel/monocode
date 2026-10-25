//! Who an MCP request comes from: the client's own `clientInfo` (from
//! `initialize` on 2025-11-25, from each request's `_meta` on 2026-07-28),
//! else, over HTTP, a stable name taken from its `User-Agent`, else
//! [`UNNAMED_CLIENT`].
use monocode_control::{ClientInfo, valid_client_name};
use rmcp::{RoleServer, service::RequestContext};

/// The name used when neither the client nor its `User-Agent` gives one.
pub(crate) const UNNAMED_CLIENT: &str = "MCP client";

/// The client a request comes from. A request that carries its own
/// protocol metadata (2026-07-28) is named by its `_meta` alone: rmcp gives
/// a stateless request a stand-in handshake that names rmcp itself.
pub(crate) fn of(context: &RequestContext<RoleServer>) -> ClientInfo {
    let info = if context.meta.protocol_version().is_some() {
        context.meta.client_info()
    } else {
        context.client_info()
    };
    let named = info
        .filter(|info| !info.name.trim().is_empty())
        .map(|info| ClientInfo {
            name: info.name,
            version: info.version,
        });
    named
        .or_else(|| {
            let parts = context.extensions.get::<http::request::Parts>()?;
            let agent = parts.headers.get(http::header::USER_AGENT)?.to_str().ok()?;
            from_user_agent(agent)
        })
        .unwrap_or_else(unnamed)
}

/// [`UNNAMED_CLIENT`], version unknown.
pub(crate) fn unnamed() -> ClientInfo {
    ClientInfo {
        name: UNNAMED_CLIENT.into(),
        version: "unknown".into(),
    }
}

/// The client a `User-Agent` names: its first product, without the
/// version, so the name stays the same across updates (`curl/8.7.1` is
/// `curl`). `None` when that is not a usable client name.
#[must_use]
pub(crate) fn from_user_agent(agent: &str) -> Option<ClientInfo> {
    let product = agent.split_whitespace().next()?;
    let (name, version) = product.split_once('/').unwrap_or((product, "unknown"));
    let name: String = name.chars().take(64).collect();
    valid_client_name(&name).then(|| ClientInfo {
        name,
        version: if version.is_empty() {
            "unknown".into()
        } else {
            version.chars().take(64).collect()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_agent_names_its_first_product_without_the_version() {
        let name = |agent: &str| from_user_agent(agent).map(|info| (info.name, info.version));
        assert_eq!(name("curl/8.7.1"), Some(("curl".into(), "8.7.1".into())));
        assert_eq!(
            name("node-fetch/1.0 (+https://github.com/bitinn/node-fetch)"),
            Some(("node-fetch".into(), "1.0".into()))
        );
        assert_eq!(name("Bun"), Some(("Bun".into(), "unknown".into())));
        assert_eq!(name("   "), None);
        assert_eq!(name("/1.0"), None);
    }
}
