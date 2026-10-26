//! Shared desktop HTTP trust and client identity; callers retain their transport policies.
pub(crate) fn config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    ureq::Agent::config_builder().tls_config(tls).http_status_as_error(false).user_agent("SolidCraft")
}
