    use super::*;

    #[test]
    fn web_scheme_accepts_only_http_and_https() {
        assert_eq!(web_scheme("https://goble.dev"), Some(WebScheme::Https));
        assert_eq!(
            web_scheme("http://example.com/a?b#c"),
            Some(WebScheme::Http)
        );
        assert_eq!(web_scheme("HTTPS://EXAMPLE.COM"), Some(WebScheme::Https));
        assert_eq!(web_scheme("file:///etc/passwd"), None);
        assert_eq!(web_scheme("javascript:alert(1)"), None);
        assert_eq!(web_scheme("rdp://host:3389"), None);
        assert_eq!(web_scheme("goble://desktop?host=h"), None);
        assert_eq!(web_scheme("https://"), None, "a web URL needs a host");
        assert_eq!(web_scheme("/not/a/url"), None);
    }

    /// The opener hands an accepted URL to the launcher and refuses every other
    /// scheme before the launcher is reached — so a `file:`/`javascript:` link
    /// can never become an OS launch.
    #[test]
    fn open_external_url_refuses_non_http_schemes() {
        let launched = std::cell::RefCell::new(Vec::new());
        let launch = |url: &str| {
            launched.borrow_mut().push(url.to_string());
            Ok(())
        };
        assert!(open_external_url_with("https://goble.dev", &launch).is_ok());
        assert_eq!(launched.borrow().as_slice(), ["https://goble.dev"]);

        assert!(open_external_url_with("file:///etc/passwd", &launch).is_err());
        assert!(open_external_url_with("javascript:alert(1)", &launch).is_err());
        assert_eq!(
            launched.borrow().as_slice(),
            ["https://goble.dev"],
            "a refused scheme must never reach the opener"
        );
    }
