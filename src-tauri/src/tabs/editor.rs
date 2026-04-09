use kosmos_protocol::requests::Request;

routed_cmd!(val fn read_file(path) -> String {
    request(p) => Request::ReadFile { path: p },
    local => kosmos_core::editor::read_file(&path),
});

routed_cmd!(void fn write_file(path, content: String) {
    request(p) => Request::WriteFile { path: p, content },
    local => kosmos_core::editor::write_file(&path, &content),
});
