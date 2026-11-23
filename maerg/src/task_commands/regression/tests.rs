use super::*;

#[test]
fn red_requires_the_exact_failed_cases_and_no_unrelated_failures() {
    let expected = vec!["tests::empty_queue".to_owned()];
    let red = "running 2 tests\ntest tests::empty_queue ... FAILED\ntest tests::existing ... ok\n\nfailures:\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored;";
    assert!(matches_result(red, &expected, true));
    assert!(!matches_result(red, &expected, false));
    assert!(!matches_result(
        &red.replace("tests::existing ... ok", "tests::existing ... FAILED"),
        &expected,
        true
    ));
    assert!(!matches_result("error: could not compile", &expected, true));
    assert!(!matches_result(
        "test tests::empty_queue ... ignored",
        &expected,
        false
    ));
    assert!(matches_result(
        "running 1 test\ntest tests::empty_queue ... ok\n\ntest result: ok. 1 passed; 0 failed;",
        &expected,
        false
    ));
}

#[test]
fn ambiguous_case_names_across_test_binaries_are_not_evidence() {
    let case = "test tests::empty_queue ... ok\n";
    let stdout = format!("{case}{case}\ntest result: ok. 2 passed; 0 failed;");
    assert!(!matches_result(
        &stdout,
        &["tests::empty_queue".into()],
        false
    ));
}
