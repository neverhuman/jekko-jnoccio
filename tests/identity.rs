use jekko_jnoccio::{identity, validate_identity};

#[test]
fn public_identity_contract_is_stable() {
    validate_identity().expect("identity validates");
    let (repo, role, profile) = identity();
    assert_eq!(repo, "jekko-jnoccio");
    assert!(!role.is_empty());
    assert!(!profile.is_empty());
}

proptest::proptest! {
    #[test]
    fn identity_parts_are_never_empty(index in 0usize..3) {
        let parts = [identity().0, identity().1, identity().2];
        proptest::prop_assert!(!parts[index].is_empty());
    }
}
