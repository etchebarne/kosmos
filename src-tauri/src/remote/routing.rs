use std::sync::Arc;

use tauri::State;

use super::agent::RemoteAgent;
use super::router::BackendRouter;

/// Result of resolving a workspace path through the router.
pub enum Route {
    /// The path belongs to a remote agent; includes the agent handle and the
    /// Linux-side path that should be sent in the request.
    Remote(Arc<RemoteAgent>, String),
    /// The path is local — handle it with kosmos-core directly.
    Local,
}

/// Resolve a workspace path: remote (with agent + linux path) or local.
/// Returns `Err` if the path looks remote but no agent is connected.
pub async fn resolve(router: &State<'_, BackendRouter>, path: &str) -> Result<Route, String> {
    if let Some((agent, remote_path)) = router.resolve(path).await {
        Ok(Route::Remote(agent, remote_path))
    } else if BackendRouter::is_remote_path(path) {
        Err(format!("Remote agent not connected for path: {path}"))
    } else {
        Ok(Route::Local)
    }
}

/// Generate a `#[tauri::command]` that routes between a remote agent and a
/// local kosmos-core call.
///
/// The first parameter (`path`) is always a `String` used for routing. Extra
/// parameters follow after a comma. Both `path` and any extras are available
/// inside the `request` and `local` expressions.
///
/// Two forms:
///
/// ```ignore
/// routed_cmd!(val fn name(path, extra: Type) -> ReturnType {
///     request(p) => Request::Variant { path: p, extra },
///     local => kosmos_core::module::func(&path, extra),
/// });
///
/// routed_cmd!(void fn name(path, extra: Type) {
///     request(p) => Request::Variant { path: p, extra },
///     local => kosmos_core::module::func(&path, extra),
/// });
/// ```
macro_rules! routed_cmd {
    (val fn $fn_name:ident($path:ident $(, $arg:ident : $arg_ty:ty)*) -> $ret:ty {
        request($p:ident) => $request:expr,
        local => $local:expr $(,)?
    }) => {
        #[tauri::command]
        pub async fn $fn_name(
            router: tauri::State<'_, $crate::remote::router::BackendRouter>,
            $path: String,
            $($arg: $arg_ty,)*
        ) -> Result<$ret, String> {
            use kosmos_protocol::ToStringErr;
            match $crate::remote::routing::resolve(&router, &$path).await? {
                $crate::remote::routing::Route::Remote(agent, $p) => {
                    let val = agent.request($request).await?;
                    serde_json::from_value(val).str_err()
                }
                $crate::remote::routing::Route::Local => $local.await.str_err(),
            }
        }
    };

    (void fn $fn_name:ident($path:ident $(, $arg:ident : $arg_ty:ty)*) {
        request($p:ident) => $request:expr,
        local => $local:expr $(,)?
    }) => {
        #[tauri::command]
        pub async fn $fn_name(
            router: tauri::State<'_, $crate::remote::router::BackendRouter>,
            $path: String,
            $($arg: $arg_ty,)*
        ) -> Result<(), String> {
            use kosmos_protocol::ToStringErr;
            match $crate::remote::routing::resolve(&router, &$path).await? {
                $crate::remote::routing::Route::Remote(agent, $p) => {
                    agent.request($request).await?;
                    Ok(())
                }
                $crate::remote::routing::Route::Local => $local.await.str_err(),
            }
        }
    };
}
