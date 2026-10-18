use fuchico_markdown::rulers::{column_x, format_rulers, parse_rulers, ruler_positions};

#[test]
fn parse_like_js_number() {
    assert_eq!(parse_rulers("80, 120"), vec![80, 120]);
    assert_eq!(parse_rulers("120;80 80\t100"), vec![80, 100, 120]);
    assert_eq!(parse_rulers(""), Vec::<u32>::new());
    assert_eq!(
        parse_rulers("0 -5 1001 abc 7.5 NaN Infinity"),
        Vec::<u32>::new()
    );
    // JS `Number` quirks the original accepted.
    assert_eq!(
        parse_rulers("1e2 +5 5.0 0x10 0b11 0o7 .5e1"),
        vec![3, 5, 7, 16, 100]
    );
    assert_eq!(parse_rulers("1000"), vec![1000]);
    assert_eq!(parse_rulers("1_0 0x 5e"), Vec::<u32>::new());
}

#[test]
fn format_and_positions() {
    assert_eq!(format_rulers(&[80, 120]), "80, 120");
    assert_eq!(column_x(10.0, 80, 7.5), 610.0);
    assert_eq!(ruler_positions(&[1, 2], 0.0, 8.0), vec![8.0, 16.0]);
}
