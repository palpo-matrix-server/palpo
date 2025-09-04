
use std::collections::{BTreeMap, BTreeSet};

use assert_matches2::assert_matches;
use http::Method;

use super::{
    AuthScheme,
    MatrixVersion::{self, V1_0, V1_1, V1_2, V1_3},
    Metadata, StablePathSelector, SupportedVersions, VersionHistory,
};
use crate::api::error::IntoHttpError;

fn stable_only_metadata(stable_paths: &'static [(StablePathSelector, &'static str)]) -> Metadata {
    Metadata {
        method: Method::GET,
        rate_limited: false,
        authentication: AuthScheme::None,
        history: VersionHistory {
            unstable_paths: &[],
            stable_paths,
            deprecated: None,
            removed: None,
        },
    }
}

fn version_only_supported(versions: &[MatrixVersion]) -> SupportedVersions {
    SupportedVersions {
        versions: versions.iter().copied().collect(),
        features: BTreeSet::new(),
    }
}

// TODO add test that can hook into tracing and verify the deprecation warning is emitted

#[test]
fn make_simple_endpoint_url() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s")]);
    let url = meta
        .make_endpoint_url(
            &version_only_supported(&[V1_0]),
            "https://example.org",
            &[],
            "",
        )
        .unwrap();
    assert_eq!(url, "https://example.org/s");
}

#[test]
fn make_endpoint_url_with_path_args() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s/{x}")]);
    let url = meta
        .make_endpoint_url(
            &version_only_supported(&[V1_0]),
            "https://example.org",
            &[&"123"],
            "",
        )
        .unwrap();
    assert_eq!(url, "https://example.org/s/123");
}

#[test]
fn make_endpoint_url_with_path_args_with_dash() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s/{x}")]);
    let url = meta
        .make_endpoint_url(
            &version_only_supported(&[V1_0]),
            "https://example.org",
            &[&"my-path"],
            "",
        )
        .unwrap();
    assert_eq!(url, "https://example.org/s/my-path");
}

#[test]
fn make_endpoint_url_with_path_args_with_reserved_char() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s/{x}")]);
    let url = meta
        .make_endpoint_url(
            &version_only_supported(&[V1_0]),
            "https://example.org",
            &[&"#path"],
            "",
        )
        .unwrap();
    assert_eq!(url, "https://example.org/s/%23path");
}

#[test]
fn make_endpoint_url_with_query() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s/")]);
    let url = meta
        .make_endpoint_url(
            &version_only_supported(&[V1_0]),
            "https://example.org",
            &[],
            "foo=bar",
        )
        .unwrap();
    assert_eq!(url, "https://example.org/s/?foo=bar");
}

#[test]
#[should_panic]
fn make_endpoint_url_wrong_num_path_args() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s/{x}")]);
    _ = meta.make_endpoint_url(
        &version_only_supported(&[V1_0]),
        "https://example.org",
        &[],
        "",
    );
}

const EMPTY: VersionHistory = VersionHistory {
    unstable_paths: &[],
    stable_paths: &[],
    deprecated: None,
    removed: None,
};

#[test]
fn select_version() {
    let version_supported = version_only_supported(&[V1_0, V1_1]);
    let superset_supported = version_only_supported(&[V1_1]);

    // With version only.
    let hist = VersionHistory {
        stable_paths: &[(StablePathSelector::Version(V1_0), "/s")],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&version_supported), Ok("/s"));
    assert!(hist.is_supported(&version_supported));
    assert_matches!(hist.select_path(&superset_supported), Ok("/s"));
    assert!(hist.is_supported(&superset_supported));

    // With feature and version.
    let hist = VersionHistory {
        stable_paths: &[(
            StablePathSelector::FeatureAndVersion {
                feature: "org.boo.stable",
                version: V1_0,
            },
            "/s",
        )],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&version_supported), Ok("/s"));
    assert!(hist.is_supported(&version_supported));
    assert_matches!(hist.select_path(&superset_supported), Ok("/s"));
    assert!(hist.is_supported(&superset_supported));

    // Select latest stable version.
    let hist = VersionHistory {
        stable_paths: &[
            (StablePathSelector::Version(V1_0), "/s_v1"),
            (StablePathSelector::Version(V1_1), "/s_v2"),
        ],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&version_supported), Ok("/s_v2"));
    assert!(hist.is_supported(&version_supported));

    // With unstable feature.
    let unstable_supported = SupportedVersions {
        versions: [V1_0].into(),
        features: ["org.boo.unstable".into()].into(),
    };
    let hist = VersionHistory {
        unstable_paths: &[(Some("org.boo.unstable"), "/u")],
        stable_paths: &[(StablePathSelector::Version(V1_0), "/s")],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&unstable_supported), Ok("/s"));
    assert!(hist.is_supported(&unstable_supported));
}

#[test]
fn select_stable_feature() {
    let supported = SupportedVersions {
        versions: [V1_1].into(),
        features: ["org.boo.unstable".into(), "org.boo.stable".into()].into(),
    };

    // With feature only.
    let hist = VersionHistory {
        unstable_paths: &[(Some("org.boo.unstable"), "/u")],
        stable_paths: &[(StablePathSelector::Feature("org.boo.stable"), "/s")],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&supported), Ok("/s"));
    assert!(hist.is_supported(&supported));

    // With feature and version.
    let hist = VersionHistory {
        unstable_paths: &[(Some("org.boo.unstable"), "/u")],
        stable_paths: &[(
            StablePathSelector::FeatureAndVersion {
                feature: "org.boo.stable",
                version: V1_3,
            },
            "/s",
        )],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&supported), Ok("/s"));
    assert!(hist.is_supported(&supported));
}

#[test]
fn select_unstable_feature() {
    let supported = SupportedVersions {
        versions: [V1_1].into(),
        features: ["org.boo.unstable".into()].into(),
    };

    let hist = VersionHistory {
        unstable_paths: &[(Some("org.boo.unstable"), "/u")],
        stable_paths: &[(
            StablePathSelector::FeatureAndVersion {
                feature: "org.boo.stable",
                version: V1_3,
            },
            "/s",
        )],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&supported), Ok("/u"));
    assert!(hist.is_supported(&supported));
}

#[test]
fn select_unstable_fallback() {
    let supported = version_only_supported(&[V1_0]);
    let hist = VersionHistory {
        unstable_paths: &[(None, "/u")],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&supported), Ok("/u"));
    assert!(!hist.is_supported(&supported));
}

#[test]
fn select_r0() {
    let supported = version_only_supported(&[V1_0]);
    let hist = VersionHistory {
        stable_paths: &[(StablePathSelector::Version(V1_0), "/r")],
        ..EMPTY
    };
    assert_matches!(hist.select_path(&supported), Ok("/r"));
    assert!(hist.is_supported(&supported));
}

#[test]
fn select_removed_err() {
    let supported = version_only_supported(&[V1_3]);
    let hist = VersionHistory {
        stable_paths: &[
            (StablePathSelector::Version(V1_0), "/r"),
            (StablePathSelector::Version(V1_1), "/s"),
        ],
        unstable_paths: &[(None, "/u")],
        deprecated: Some(V1_2),
        removed: Some(V1_3),
    };
    assert_matches!(
        hist.select_path(&supported),
        Err(IntoHttpError::EndpointRemoved(V1_3))
    );
    assert!(!hist.is_supported(&supported));
}

#[test]
fn partially_removed_but_stable() {
    let supported = version_only_supported(&[V1_2]);
    let hist = VersionHistory {
        stable_paths: &[
            (StablePathSelector::Version(V1_0), "/r"),
            (StablePathSelector::Version(V1_1), "/s"),
        ],
        unstable_paths: &[],
        deprecated: Some(V1_2),
        removed: Some(V1_3),
    };
    assert_matches!(hist.select_path(&supported), Ok("/s"));
    assert!(hist.is_supported(&supported));
}

#[test]
fn no_unstable() {
    let supported = version_only_supported(&[V1_0]);
    let hist = VersionHistory {
        stable_paths: &[(StablePathSelector::Version(V1_1), "/s")],
        ..EMPTY
    };
    assert_matches!(
        hist.select_path(&supported),
        Err(IntoHttpError::NoUnstablePath)
    );
    assert!(!hist.is_supported(&supported));
}

#[test]
fn version_literal() {
    const LIT: MatrixVersion = MatrixVersion::from_lit("1.0");

    assert_eq!(LIT, V1_0);
}

#[test]
fn parse_as_str_sanity() {
    let version = MatrixVersion::try_from("r0.5.0").unwrap();
    assert_eq!(version, V1_0);
    assert_eq!(version.as_str(), None);

    let version = MatrixVersion::try_from("v1.1").unwrap();
    assert_eq!(version, V1_1);
    assert_eq!(version.as_str(), Some("v1.1"));
}

#[test]
fn supported_versions_from_parts() {
    let empty_features = BTreeMap::new();

    let none = &[];
    let none_supported = SupportedVersions::from_parts(none, &empty_features);
    assert_eq!(none_supported.versions, BTreeSet::new());
    assert_eq!(none_supported.features, BTreeSet::new());

    let single_known = &["r0.6.0".to_owned()];
    let single_known_supported = SupportedVersions::from_parts(single_known, &empty_features);
    assert_eq!(single_known_supported.versions, BTreeSet::from([V1_0]));
    assert_eq!(single_known_supported.features, BTreeSet::new());

    let multiple_known = &["v1.1".to_owned(), "r0.6.0".to_owned(), "r0.6.1".to_owned()];
    let multiple_known_supported = SupportedVersions::from_parts(multiple_known, &empty_features);
    assert_eq!(
        multiple_known_supported.versions,
        BTreeSet::from([V1_0, V1_1])
    );
    assert_eq!(multiple_known_supported.features, BTreeSet::new());

    let single_unknown = &["v0.0".to_owned()];
    let single_unknown_supported = SupportedVersions::from_parts(single_unknown, &empty_features);
    assert_eq!(single_unknown_supported.versions, BTreeSet::new());
    assert_eq!(single_unknown_supported.features, BTreeSet::new());

    let mut features = BTreeMap::new();
    features.insert("org.bar.enabled_1".to_owned(), true);
    features.insert("org.bar.disabled".to_owned(), false);
    features.insert("org.bar.enabled_2".to_owned(), true);

    let features_supported = SupportedVersions::from_parts(single_known, &features);
    assert_eq!(features_supported.versions, BTreeSet::from([V1_0]));
    assert_eq!(
        features_supported.features,
        ["org.bar.enabled_1".into(), "org.bar.enabled_2".into()].into()
    );
}

#[test]
fn supported_versions_from_parts_order() {
    let empty_features = BTreeMap::new();

    let sorted = &[
        "r0.0.1".to_owned(),
        "r0.5.0".to_owned(),
        "r0.6.0".to_owned(),
        "r0.6.1".to_owned(),
        "v1.1".to_owned(),
        "v1.2".to_owned(),
    ];
    let sorted_supported = SupportedVersions::from_parts(sorted, &empty_features);
    assert_eq!(
        sorted_supported.versions,
        BTreeSet::from([V1_0, V1_1, V1_2])
    );

    let sorted_reverse = &[
        "v1.2".to_owned(),
        "v1.1".to_owned(),
        "r0.6.1".to_owned(),
        "r0.6.0".to_owned(),
        "r0.5.0".to_owned(),
        "r0.0.1".to_owned(),
    ];
    let sorted_reverse_supported = SupportedVersions::from_parts(sorted_reverse, &empty_features);
    assert_eq!(
        sorted_reverse_supported.versions,
        BTreeSet::from([V1_0, V1_1, V1_2])
    );

    let random_order = &[
        "v1.1".to_owned(),
        "r0.6.1".to_owned(),
        "r0.5.0".to_owned(),
        "r0.6.0".to_owned(),
        "r0.0.1".to_owned(),
        "v1.2".to_owned(),
    ];
    let random_order_supported = SupportedVersions::from_parts(random_order, &empty_features);
    assert_eq!(
        random_order_supported.versions,
        BTreeSet::from([V1_0, V1_1, V1_2])
    );
}

#[test]
#[should_panic]
fn make_endpoint_url_with_path_args_old_syntax() {
    let meta = stable_only_metadata(&[(StablePathSelector::Version(V1_0), "/s/:x")]);
    let url = meta
        .make_endpoint_url(
            &version_only_supported(&[V1_0]),
            "https://example.org",
            &[&"123"],
            "",
        )
        .unwrap();
    assert_eq!(url, "https://example.org/s/123");
}
