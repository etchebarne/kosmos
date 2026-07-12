pub(crate) mod messages;
pub(crate) mod router;
pub mod schema;
mod transport;

pub fn run(socket_path: std::path::PathBuf, application: core::Application) -> std::io::Result<()> {
    transport::server::run(socket_path, application)
}
