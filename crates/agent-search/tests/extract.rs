use agent_search::extract::extract_url;
use agent_search::safety::strip_active_html;
use agent_search::ExtractorId;

#[test]
fn strips_active_html() {
    let text = strip_active_html(
        "<html><head><script>alert(1)</script></head><body>Hello <b>world</b></body></html>",
    );
    assert_eq!(text, "Hello world");
}

#[tokio::test]
async fn blocks_internal_urls_at_extraction_time() {
    let err = extract_url("http://127.0.0.1/private", &[ExtractorId::BuiltIn])
        .await
        .expect_err("blocked");
    assert!(err.to_string().contains("blocked"));
}
