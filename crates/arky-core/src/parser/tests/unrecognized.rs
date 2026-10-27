use super::check;

#[test]
fn a_heading_or_a_fence_is_not_recognized_inside_a_block_quote() {
    check(
        "\
> # quoted heading
> ```
> quoted code
",
        r##"Document
  MarkdownBlock
    Section
      BlockQuote "# quoted heading\n```\nquoted code"
"##,
    );
}
