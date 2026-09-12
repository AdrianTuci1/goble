use super::*;
use crate::harness::Harness;

use crate::harness::web::decode_ddg_url;

#[test]
fn test_web_search_tool_schema() {
    let store = Store::open_in_memory().unwrap();
    let harness = Harness::new(store);
    let tool = harness
        .list_tools()
        .into_iter()
        .find(|t| t.name == "web_search")
        .expect("web_search tool should exist");
    let props = tool
        .parameters
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("web_search schema should have properties");
    assert!(props.contains_key("query"));
    assert!(
        props.contains_key("max_results"),
        "web_search should expose max_results"
    );
    assert!(
        props.contains_key("advanced"),
        "web_search should expose advanced"
    );
    assert_eq!(props["max_results"]["maximum"], 30);
    let required = tool
        .parameters
        .get("required")
        .and_then(|r| r.as_array())
        .expect("web_search schema should list required");
    assert!(required.iter().any(|v| v == "query"));
}

#[test]
fn test_decode_ddg_url() {
    assert_eq!(
        decode_ddg_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Fcyanheads%2Ffilesystem-mcp-server&rut=abc"),
        "https://github.com/cyanheads/filesystem-mcp-server"
    );
    assert_eq!(
        decode_ddg_url("https://example.com/page"),
        "https://example.com/page"
    );
    // Non-http decoded values are not trusted; the original href is kept.
    assert_eq!(
        decode_ddg_url("//duckduckgo.com/l/?uddg=not-a-url"),
        "//duckduckgo.com/l/?uddg=not-a-url"
    );
}
