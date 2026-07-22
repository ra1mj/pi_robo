use pi_tools::{truncate_head, truncate_tail};

#[test]
fn head_never_returns_a_partial_line() {
    let result = truncate_head("123456\nsecond", 10, 5);
    assert!(result.truncated);
    assert!(result.first_line_exceeds_limit);
    assert!(result.content.is_empty());
}

#[test]
fn trailing_newline_is_not_an_extra_logical_line() {
    let result = truncate_head("one\ntwo\n", 2, 100);
    assert!(!result.truncated);
    assert_eq!(result.total_lines, 2);
    assert_eq!(result.content, "one\ntwo\n");
}

#[test]
fn tail_keeps_a_utf8_safe_partial_suffix_of_an_oversized_last_line() {
    let result = truncate_tail("earlier\n甲乙丙丁", 20, 7);
    assert!(result.truncated);
    assert!(result.last_line_partial);
    assert!(result.content.is_char_boundary(0));
    assert!(result.output_bytes <= 7);
    assert!("甲乙丙丁".ends_with(&result.content));
}
