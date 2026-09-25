mod common;

#[test]
fn temp_dir_writes_nested_files_and_cleans_up() {
    let root;
    {
        let t = common::TempDir::new("scaffold");
        let p = t.write("a/b/c.txt", "hello");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
        root = t.path().to_path_buf();
        assert!(root.exists());
    }
    assert!(!root.exists(), "TempDir must delete itself on drop");
}
