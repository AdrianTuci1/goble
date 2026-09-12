use super::*;

    #[test]
    fn cwd_resolution_falls_back_to_process_dir() {
        let cwd = resolve_cwd("/definitely/not/a/real/dir/xyz");
        assert!(cwd.is_absolute());
        let cwd2 = resolve_cwd("");
        assert!(cwd2.is_absolute());
    }
