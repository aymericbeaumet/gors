use super::*;

#[test]
fn remap_preserves_original_source_and_name_tables() {
    let mut builder = SourceMapBuilder::new(Some("generated.rs"));
    let main_source = builder.add_source("entry/main.go");
    builder.set_source_contents(main_source, Some("package main\nfunc first() {}\n"));
    let helper_source = builder.add_source("entry/helper.go");
    builder.set_source_contents(helper_source, Some("package main\nfunc downstream() {}\n"));
    builder.add_to_ignore_list(helper_source);
    let first_name = builder.add_name("first");
    let downstream_name = builder.add_name("downstream");
    builder.add_raw(0, 3, 1, 5, Some(main_source), Some(first_name), false);
    builder.add_raw(1, 3, 3, 5, Some(helper_source), Some(downstream_name), true);
    let source_map = builder.into_sourcemap();
    let comments = Comments {
        source_name: "entry/helper.go".to_string(),
        comments: vec![Comment {
            go_line: 4,
            go_column: 0,
            text: "/* first\nsecond\nthird */".to_string(),
            is_doc: false,
        }],
    };

    let (_, remapped) = insert_and_remap(
        "fn first() {}\nfn downstream() {}\n",
        &comments,
        &source_map,
    );

    assert_eq!(remapped.get_file(), Some("generated.rs"));
    assert_eq!(remapped.get_source_count(), 2);
    assert_eq!(remapped.get_source(0), Some("entry/main.go"));
    assert_eq!(remapped.get_source(1), Some("entry/helper.go"));
    assert_eq!(
        remapped.get_source_contents(0),
        Some("package main\nfunc first() {}\n")
    );
    assert_eq!(
        remapped.get_source_contents(1),
        Some("package main\nfunc downstream() {}\n")
    );
    assert_eq!(remapped.get_name(0), Some("first"));
    assert_eq!(remapped.get_name(1), Some("downstream"));
    assert_eq!(remapped.get_name(2), Some("/* first\nsecond\nthird */"));
    assert_eq!(remapped.ignore_list().copied().collect::<Vec<_>>(), vec![1]);

    let downstream = remapped
        .tokens()
        .find(|token| token.get_name() == Some("downstream"))
        .unwrap();
    assert_eq!(downstream.get_src_id(), 1);
    assert_eq!(downstream.get_dst_line(), 4);
    assert!(downstream.is_range());
    let inserted_comment = remapped
        .tokens()
        .find(|token| token.get_name() == Some("/* first\nsecond\nthird */"))
        .unwrap();
    assert_eq!(inserted_comment.get_src_id(), 1);
}
