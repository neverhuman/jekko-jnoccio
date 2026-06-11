use jekko_jnoccio::{identity, validate_identity};

#[test]
fn public_identity_contract_is_stable() {
    validate_identity().expect("identity validates");
    let (repo, role, profile) = identity();
    assert_eq!(repo, "jekko-jnoccio");
    assert_eq!(role, "router");
    assert_eq!(profile, "rust-router");
}
