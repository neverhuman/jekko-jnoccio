use std::error::Error;

#[test]
fn domain_error_metadata_is_actionable() {
    let err = domain::DomainError::IdentityDrift;
    assert!(err.purpose().contains("identity"));
    assert!(err.reason().contains("agents"));
    assert!(err.repair_hint().contains("split-family"));
    assert!(err.docs_url().starts_with("docs/"));
    assert!(!err.common_fixes().is_empty());
    let boxed: &dyn Error = &err;
    assert!(boxed.to_string().contains("identity"));
}
