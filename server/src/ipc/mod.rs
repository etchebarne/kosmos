pub(crate) mod messages;
pub(crate) mod router;
pub mod schema;
mod transport;

pub fn run(
    socket_path: std::path::PathBuf,
    state: core::State,
    store: core::persistence::StateStore,
) -> std::io::Result<()> {
    transport::server::run(socket_path, state, store)
}
