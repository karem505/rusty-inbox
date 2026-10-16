//! TLS client configuration (rustls + system trust store, ring provider).

use std::sync::{Arc, OnceLock};
use tokio_rustls::rustls::{self, pki_types::ServerName, ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

pub fn config() -> Arc<ClientConfig> {
    static CFG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        let native = rustls_native_certs::load_native_certs();
        for c in native.certs {
            let _ = roots.add(c);
        }
        let cfg = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("tls versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(cfg)
    })
    .clone()
}

pub fn connector() -> TlsConnector {
    TlsConnector::from(config())
}

pub fn server_name(host: &str) -> anyhow::Result<ServerName<'static>> {
    Ok(ServerName::try_from(host.to_string())?)
}
