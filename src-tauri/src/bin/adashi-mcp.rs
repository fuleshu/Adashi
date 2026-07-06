fn main() {
    if let Err(err) = adashi_lib::run_mcp() {
        eprintln!("Adashi MCP server failed: {err}");
        std::process::exit(1);
    }
}
