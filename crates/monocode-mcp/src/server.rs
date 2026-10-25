//! The `rmcp` server handler: capabilities, instructions, tools, read-only
//! resources and their change subscriptions on both protocol versions.
use crate::{
    Options,
    backend::Connect,
    errors, identity, launch,
    link::Link,
    log, resources,
    socket::Socket,
    tools,
    watch::{Inbox, Watch},
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, Implementation,
        ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ServerCapabilities,
        ServerConfig, SubscribeRequestParams, SubscriptionFilter, Tool, UnsubscribeRequestParams,
    },
    service::{MaybeSendFuture, RequestContext, SubscriptionContext},
};
use std::sync::Arc;

/// What the model reads before using the tools (MCP `instructions`).
pub(crate) const INSTRUCTIONS: &str = "MonoCode is a desktop app that runs coding agents (Claude Code, Codex and others) in projects, tabs and panes. These tools see and drive the running app; changes appear in its window as if the person made them. Projects, tabs, sessions, notes, worktrees, settings and the rest of MonoCode's state are managed through these tools only: never edit MonoCode's database or files directly. Call app_info first: it shows the scopes this client holds. Work is dispatched with session_start and observed with session_wait where those tools are offered. Failures are tool results with {code, message, retryable}: app_not_running means MonoCode is closed, control_disabled means the person must turn on Settings → General → MCP server, scope_denied means the person has not granted that scope to this client, and user_declined means the person refused, or did not answer within two minutes, the confirmation MonoCode shows before deleting, discarding or pushing; nothing was changed.";

/// The instructions of a relay that speaks for an agent session the app
/// runs itself.
pub(crate) const SESSION_INSTRUCTIONS: &str = "You are running inside MonoCode, the desktop app that runs this coding agent session in one of its projects, tabs and panes. These tools see and drive that running app; changes appear in its window as if the person made them. Projects, tabs, panes, sessions, notes, worktrees, automations, settings and the rest of MonoCode's state are managed through these tools only: when the person asks for something in MonoCode (for example \"add ~/code/app to MonoCode\" is project_add, \"open a new session for this\" is session_start), call the tool, and never read or edit MonoCode's database, settings or data files directly. This session holds every scope. Whether a call runs is decided by your own permission mode, as for any other tool; MonoCode asks no confirmation of its own. A session cannot stop, archive, delete, reconfigure or wait on itself. Failures are tool results with {code, message, retryable}: invalid_input names what to change, not_found means the target is gone.";

/// How long a client may cache the tool list: it changes only with the app.
const TOOLS_TTL_MS: u64 = 5 * 60 * 1000;

/// The MCP server one session talks to, whichever transport carries it.
#[derive(Clone)]
pub(crate) struct Relay {
    tools: Arc<Vec<Tool>>,
    link: Arc<Link>,
    watch: Arc<Watch>,
    /// The stdio relay's options: it alone answers `app_launch`, and it may
    /// speak for one of the app's own sessions.
    stdio: Option<Arc<Options>>,
}

impl Relay {
    /// The stdio relay: the app's control socket, and `app_launch`.
    pub(crate) fn stdio(options: Options) -> Self {
        let socket = options.clone();
        let connect: Connect = Arc::new(move |sink| Box::new(Socket::new(socket.clone(), sink)));
        Self::new(&connect, Some(options))
    }

    /// The app's own HTTP server: each session's calls go to the backend
    /// `connect` makes. Without a relay process there is nothing to launch.
    pub(crate) fn http(connect: &Connect) -> Self {
        Self::new(connect, None)
    }

    fn new(connect: &Connect, stdio: Option<Options>) -> Self {
        let inbox = Arc::new(Inbox::default());
        let link = Arc::new(Link::new(connect(inbox.sink())));
        let mut tools = tools::tools();
        if stdio.is_some() {
            tools.push(launch::tool());
        }
        Self {
            tools: Arc::new(tools),
            watch: Arc::new(Watch::new(Arc::clone(&link), inbox)),
            link,
            stdio: stdio.map(Arc::new),
        }
    }
}

impl ServerHandler for Relay {
    fn get_info(&self) -> ServerConfig {
        // The resource and tool lists are fixed for a relay build, so there
        // is no list_changed; resource contents push updates.
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_resources_subscribe()
                .enable_tools()
                .build(),
        )
        .with_server_info(
            Implementation::new("monocode", env!("CARGO_PKG_VERSION")).with_title("MonoCode"),
        )
        .with_instructions(
            if self.stdio.as_ref().is_some_and(|o| o.session.is_some()) {
                SESSION_INSTRUCTIONS
            } else {
                INSTRUCTIONS
            },
        )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: self.tools.as_ref().clone(),
            ttl_ms: Some(TOOLS_TTL_MS),
            cache_scope: Some(CacheScope::Private),
            ..ListToolsResult::default()
        }))
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(ListResourcesResult {
            resources: resources::resources(),
            ttl_ms: Some(TOOLS_TTL_MS),
            cache_scope: Some(CacheScope::Private),
            ..ListResourcesResult::default()
        }))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + MaybeSendFuture + '_
    {
        std::future::ready(Ok(ListResourceTemplatesResult {
            resource_templates: resources::templates(),
            ttl_ms: Some(TOOLS_TTL_MS),
            cache_scope: Some(CacheScope::Private),
            ..ListResourceTemplatesResult::default()
        }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = request.uri;
        let read = match resources::resolve(&uri) {
            Ok((mapping, arguments)) => match tools::command(mapping.operation, Some(arguments)) {
                Ok(call) => self
                    .link
                    .call(call, identity::of(&context))
                    .await
                    .map(|output| (mapping, output)),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        match read {
            Ok((mapping, output)) => Ok(ReadResourceResult::new(vec![resources::contents(
                &uri, &output,
            )])
            .with_ttl_ms(mapping.ttl_ms)
            .with_cache_scope(CacheScope::Private)
            .into()),
            Err(error) => {
                log::line(&format!("reading {uri} failed: {error}"));
                Err(errors::protocol(&error))
            }
        }
    }

    /// `resources/subscribe` (2025-11-25).
    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let uri = request.uri;
        let client = identity::of(&context);
        self.watch
            .subscribe(uri.clone(), context.peer.clone(), client)
            .await
            .map_err(|error| {
                log::line(&format!("subscribing to {uri} failed: {error}"));
                errors::protocol(&error)
            })
    }

    /// `resources/unsubscribe` (2025-11-25).
    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.watch.unsubscribe(&request.uri).await;
        Ok(())
    }

    /// `subscriptions/listen` (2026-07-28): resource URIs that can be
    /// followed. List changes are never offered; the lists are fixed.
    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        let uris: Vec<String> = requested
            .resource_subscriptions
            .iter()
            .flatten()
            .filter(|uri| resources::topic(uri).is_ok())
            .cloned()
            .collect();
        let filter = SubscriptionFilter::builder();
        Some(if uris.is_empty() {
            filter.build()
        } else {
            filter.resource_subscriptions(uris).build()
        })
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let uris = context
            .accepted()
            .resource_subscriptions
            .clone()
            .unwrap_or_default();
        if uris.is_empty() {
            context.cancelled().await;
            return Ok(());
        }
        let client = identity::of(context.request_context());
        let id = self
            .watch
            .listen(uris, context.sink().clone(), client)
            .await
            .map_err(|error| {
                log::line(&format!("listening failed: {error}"));
                errors::protocol(&error)
            })?;
        context.cancelled().await;
        self.watch.stop(id).await;
        Ok(())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if let Some(options) = self.stdio.as_ref().filter(|_| name == launch::NAME) {
            let launched = launch::run(options.as_ref().clone()).await;
            return Ok(match launched {
                Ok(output) => errors::structured(launch::summary(&output), output),
                Err(error) => {
                    log::line(&format!("{name} failed: {error}"));
                    errors::failure(&error)
                }
            }
            .into());
        }
        let result = match tools::command(&name, request.arguments) {
            Ok(call) => self.link.call(call, identity::of(&context)).await,
            Err(error) => Err(error),
        };
        Ok(match result {
            Ok(output) => errors::success(&name, output),
            Err(error) => {
                log::line(&format!("{name} failed: {error}"));
                errors::failure(&error)
            }
        }
        .into())
    }
}
