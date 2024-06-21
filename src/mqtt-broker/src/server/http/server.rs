use super::cache::{cache_info, index, metrics};
use crate::core::metadata_cache::MetadataCacheManager;
use crate::core::qos_manager::QosManager;
use crate::subscribe::subscribe_cache::SubscribeCacheManager;
use crate::{core::heartbeat_cache::HeartbeatCache, server::tcp::packet::ResponsePackage};
use axum::routing::get;
use axum::Router;
use common_base::{config::broker_mqtt::broker_mqtt_conf, log::info};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::broadcast::Sender;

pub const ROUTE_ROOT: &str = "/";
pub const ROUTE_CACHE: &str = "/caches";
pub const ROUTE_METRICS: &str = "/metrics";

#[derive(Clone)]
pub struct HttpServerState {
    pub metadata_cache: Arc<MetadataCacheManager>,
    pub heartbeat_manager: Arc<HeartbeatCache>,
    pub subscribe_cache: Arc<SubscribeCacheManager>,
    pub qos_manager: Arc<QosManager>,
}

impl HttpServerState {
    pub fn new(
        metadata_cache: Arc<MetadataCacheManager>,
        heartbeat_manager: Arc<HeartbeatCache>,
        subscribe_cache: Arc<SubscribeCacheManager>,
        qos_manager: Arc<QosManager>,
    ) -> Self {
        return Self {
            metadata_cache,
            heartbeat_manager,
            subscribe_cache,
            qos_manager,
        };
    }
}

pub async fn start_http_server(state: HttpServerState) {
    let config = broker_mqtt_conf();
    let ip: SocketAddr = format!("0.0.0.0:{}", config.http_port).parse().unwrap();
    let app = routes(state);
    let listener = tokio::net::TcpListener::bind(ip).await.unwrap();
    info(format!(
        "Broker HTTP Server start success. bind addr:{}",
        ip
    ));
    axum::serve(listener, app).await.unwrap();
}

fn routes(state: HttpServerState) -> Router {
    let meta = Router::new()
        .route(ROUTE_ROOT, get(index))
        .route(ROUTE_CACHE, get(cache_info))
        .route(ROUTE_METRICS, get(metrics));
    let app = Router::new().merge(meta);
    return app.with_state(state);
}
