//! Unit tests covering `Display` formatting and Serde JSON/TOML roundtrips
//! for all core domain models in `canary-core`.

use std::path::PathBuf;

use canary_core::{
    Capability, CompatibilityResult, ErrorCategory, ExitCode, FixtureMetadata, FixtureStore,
    GitContext, NetworkContext, NetworkName, Policy, ProjectContext, ProjectType, ProtocolVersion,
    ResultSource, RunOptions, SkipCode, Status, Surface,
};

#[test]
fn protocol_version_surface_status_and_result_source_display_and_serde_roundtrip() {
    let pv = ProtocolVersion::from(28);
    assert_eq!(pv.to_string(), "28");
    let pv_json = serde_json::to_string(&pv).unwrap();
    assert_eq!(pv_json, "28");
    assert_eq!(
        serde_json::from_str::<ProtocolVersion>(&pv_json).unwrap(),
        pv
    );

    for (surface, expected) in [
        (Surface::Xdr, "xdr"),
        (Surface::Rpc, "rpc"),
        (Surface::Soroban, "soroban"),
    ] {
        assert_eq!(surface.to_string(), expected);
        let json = serde_json::to_string(&surface).unwrap();
        assert_eq!(json, format!("\"{expected}\""));
        assert_eq!(serde_json::from_str::<Surface>(&json).unwrap(), surface);
    }

    for (status, expected) in [
        (Status::Pass, "pass"),
        (Status::Warning, "warning"),
        (Status::Fail, "fail"),
        (Status::Skipped, "skipped"),
        (Status::Error, "error"),
    ] {
        assert_eq!(status.to_string(), expected);
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, format!("\"{expected}\""));
        assert_eq!(serde_json::from_str::<Status>(&json).unwrap(), status);
    }

    for (source, expected) in [
        (ResultSource::Live, "live"),
        (ResultSource::Cache, "cache"),
        (ResultSource::Unknown, "unknown"),
    ] {
        assert_eq!(source.to_string(), expected);
        assert_eq!(source.as_str(), expected);
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, format!("\"{expected}\""));
        assert_eq!(serde_json::from_str::<ResultSource>(&json).unwrap(), source);
    }
}

#[test]
fn project_type_capability_and_network_name_display_and_serde_roundtrip() {
    for (pt, expected) in [
        (ProjectType::Soroban, "soroban"),
        (ProjectType::RpcConsumer, "rpc-consumer"),
        (ProjectType::StellarSdk, "stellar-sdk"),
        (ProjectType::GenericStellar, "generic-stellar"),
        (ProjectType::Unknown, "unknown"),
    ] {
        assert_eq!(pt.to_string(), expected);
        let json = serde_json::to_string(&pt).unwrap();
        assert_eq!(json, format!("\"{expected}\""));
        assert_eq!(serde_json::from_str::<ProjectType>(&json).unwrap(), pt);
    }

    for cap in Capability::all() {
        let json = serde_json::to_string(cap).unwrap();
        let decoded: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, cap);
    }

    for (net, expected_display) in [
        (NetworkName::Testnet, "testnet"),
        (NetworkName::Mainnet, "mainnet"),
        (NetworkName::Futurenet, "futurenet"),
        (
            NetworkName::Custom("local-standalone".into()),
            "local-standalone",
        ),
    ] {
        assert_eq!(net.to_string(), expected_display);
        let json = serde_json::to_string(&net).unwrap();
        let decoded: NetworkName = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, net);
    }
}

#[test]
fn compatibility_result_and_contexts_serde_roundtrip() {
    let result = CompatibilityResult {
        test_id: "p28-xdr-roundtrip".into(),
        protocol: ProtocolVersion(28),
        surface: Surface::Xdr,
        status: Status::Fail,
        summary: "roundtrip mismatch".into(),
        details: Some("byte 4 differed".into()),
        duration_ms: 17,
        fixture_id: Some("p28-xdr-roundtrip".into()),
        source: ResultSource::Live,
    };
    assert!(result.is_required_failure());
    let json = serde_json::to_string(&result).unwrap();
    let decoded: CompatibilityResult = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, result);

    let project = ProjectContext {
        root: PathBuf::from("/workspace/demo"),
        name: "demo".into(),
        project_type: ProjectType::Soroban,
        capabilities: vec![Capability::SorobanContract, Capability::RpcClient],
    };
    assert!(project.has_capability(&Capability::SorobanContract));
    assert!(!project.has_capability(&Capability::WasmArtifact));
    let project_json = serde_json::to_string(&project).unwrap();
    assert_eq!(
        serde_json::from_str::<ProjectContext>(&project_json).unwrap(),
        project
    );

    let network = NetworkContext {
        name: NetworkName::Testnet,
        rpc_url: "https://soroban-testnet.stellar.org".into(),
        passphrase: "Test SDF Network ; September 2015".into(),
        observed_protocol: Some(ProtocolVersion(28)),
    };
    let net_json = serde_json::to_string(&network).unwrap();
    assert_eq!(
        serde_json::from_str::<NetworkContext>(&net_json).unwrap(),
        network
    );

    let git = GitContext {
        commit: Some("abc1234".into()),
        branch: Some("main".into()),
        is_dirty: Some(false),
    };
    let git_json = serde_json::to_string(&git).unwrap();
    assert_eq!(serde_json::from_str::<GitContext>(&git_json).unwrap(), git);

    let opts = RunOptions::default();
    let opts_json = serde_json::to_string(&opts).unwrap();
    assert_eq!(
        serde_json::from_str::<RunOptions>(&opts_json).unwrap(),
        opts
    );
}

#[test]
fn fixture_store_skip_code_policy_and_error_categories_roundtrip() {
    let meta = FixtureMetadata {
        id: "p28-rpc-health".into(),
        protocol: ProtocolVersion(28),
        surface: Surface::Rpc,
        category: "health".into(),
        description: "Verify getHealth".into(),
        source_reference: Some("CA-01".into()),
        required_capabilities: vec![Capability::RpcClient],
        content_digest: "deadbeef".into(),
    };
    let store = FixtureStore::new(vec![meta.clone()]);
    assert_eq!(store.len(), 1);
    assert!(!store.is_empty());
    assert_eq!(store.by_id("p28-rpc-health"), Some(&meta));
    let store_json = serde_json::to_string(&store).unwrap();
    assert_eq!(
        serde_json::from_str::<FixtureStore>(&store_json).unwrap(),
        store
    );

    let policy = Policy::default();
    let policy_json = serde_json::to_string(&policy).unwrap();
    assert_eq!(
        serde_json::from_str::<Policy>(&policy_json).unwrap(),
        policy
    );

    for skip in [
        SkipCode::UnsupportedProtocol,
        SkipCode::MissingCapability,
        SkipCode::NetworkMismatch,
        SkipCode::SurfaceDisabled,
        SkipCode::Offline,
        SkipCode::Unknown,
    ] {
        let s = skip.to_string();
        assert!(!s.is_empty());
        assert_eq!(SkipCode::parse(&s), skip);
    }

    assert_eq!(ExitCode::Pass.code(), 0);
    assert_eq!(ExitCode::CompatibilityFailure.code(), 1);
    assert_eq!(ExitCode::ConfigurationError.code(), 2);
    assert_eq!(ExitCode::ExecutionError.code(), 3);
    assert_eq!(ExitCode::InvalidFixture.code(), 4);
    assert_eq!(ExitCode::InternalError.code(), 5);
    assert_eq!(ExitCode::Pass.to_string(), "pass");

    let cfg_err = canary_core::CanaryError::Configuration("bad config".into());
    assert_eq!(cfg_err.category(), ErrorCategory::Configuration);
    assert_eq!(ExitCode::from(&cfg_err), ExitCode::ConfigurationError);
}
