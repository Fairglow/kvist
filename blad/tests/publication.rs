#[test]
fn standalone_package_identity_and_license_are_explicit() {
    assert_eq!(env!("CARGO_PKG_NAME"), "kvist-blad");
    assert_eq!(env!("CARGO_PKG_LICENSE"), "AGPL-3.0-or-later");
    assert_eq!(env!("CARGO_PKG_RUST_VERSION"), "1.95");
    let license = include_str!("../LICENSE");
    assert!(license.contains("GNU AFFERO GENERAL PUBLIC LICENSE"));
    assert!(license.contains("Version 3, 19 November 2007"));
}
