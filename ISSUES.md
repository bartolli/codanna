# Local Issues Tracker

## Issue #1: FileWalker gitignore support not working properly in temp directories

**Status**: Open  
**Priority**: Low  
**Labels**: bug, testing  
**Created**: 2025-07-24  

### Problem

The FileWalker's gitignore support doesn't work correctly when testing in temporary directories. The test `test_gitignore_respected` in `src/indexing/walker.rs` is currently marked as ignored.

### Expected Behavior

Files listed in .gitignore should be excluded from the walk results, even in temporary directories.

### Actual Behavior

The walker finds both ignored and included files, suggesting that the .gitignore file is not being respected.

### Steps to Reproduce

1. Create a temporary directory
2. Add a .gitignore file with a pattern
3. Create files matching and not matching the pattern
4. Use FileWalker to walk the directory
5. Observe that ignored files are still found

### Investigation Needed

- Check if the ignore crate requires special handling for temp directories
- Verify if .gitignore needs to be in a git repository to work
- Consider if we need to use WalkBuilder's add_ignore method differently

### Test Case

```rust
#[test]
#[ignore = "gitignore handling in temp directories needs investigation"]
fn test_gitignore_respected() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    
    // Create .gitignore
    fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
    
    // Create files
    fs::write(root.join("ignored.rs"), "fn ignored() {}").unwrap();
    fs::write(root.join("included.rs"), "fn included() {}").unwrap();
    
    let settings = create_test_settings();
    let walker = FileWalker::new(settings);
    
    let files: Vec<_> = walker.walk(root).collect();
    
    // Should only find the included file
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("included.rs"));
}
```

### Notes

The walker works correctly in real project directories, this only affects tests. The issue might be related to the ignore crate expecting a git repository context.