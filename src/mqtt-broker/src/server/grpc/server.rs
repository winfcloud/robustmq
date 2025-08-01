// Copyright 2023 RobustMQ Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;

use common_base::error::common::CommonError;
use grpc_clients::pool::ClientPool;
use protocol::broker_mqtt::broker_mqtt_admin::mqtt_broker_admin_service_server::MqttBrokerAdminServiceServer;
use protocol::broker_mqtt::broker_mqtt_inner::mqtt_broker_inner_service_server::MqttBrokerInnerServiceServer;
use schema_register::schema::SchemaRegisterManager;
use storage_adapter::storage::ArcStorageAdapter;
use tonic::transport::Server;
use tracing::info;

use super::inner::GrpcInnerServices;
use crate::bridge::manager::ConnectorManager;
use crate::common::metrics_cache::MetricsCacheManager;
use crate::handler::cache::CacheManager;
use crate::server::common::connection_manager::ConnectionManager;
use crate::server::grpc::admin::GrpcAdminServices;
use crate::subscribe::manager::SubscribeManager;

pub struct GrpcServer {
    port: u32,
    metadata_cache: Arc<CacheManager>,
    connector_manager: Arc<ConnectorManager>,
    connection_manager: Arc<ConnectionManager>,
    subscribe_manager: Arc<SubscribeManager>,
    schema_manager: Arc<SchemaRegisterManager>,
    client_pool: Arc<ClientPool>,
    message_storage_adapter: ArcStorageAdapter,
    metrics_cache_manager: Arc<MetricsCacheManager>,
}

impl GrpcServer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        port: u32,
        metadata_cache: Arc<CacheManager>,
        connector_manager: Arc<ConnectorManager>,
        subscribe_manager: Arc<SubscribeManager>,
        connection_manager: Arc<ConnectionManager>,
        schema_manager: Arc<SchemaRegisterManager>,
        client_pool: Arc<ClientPool>,
        message_storage_adapter: ArcStorageAdapter,
        metrics_cache_manager: Arc<MetricsCacheManager>,
    ) -> Self {
        Self {
            port,
            connector_manager,
            metadata_cache,
            connection_manager,
            subscribe_manager,
            client_pool,
            message_storage_adapter,
            schema_manager,
            metrics_cache_manager,
        }
    }
    pub async fn start(&self) -> Result<(), CommonError> {
        let addr = format!("0.0.0.0:{}", self.port).parse()?;
        info!("Broker Grpc Server start success. port:{}", self.port);
        let inner_handler = GrpcInnerServices::new(
            self.metadata_cache.clone(),
            self.subscribe_manager.clone(),
            self.connector_manager.clone(),
            self.schema_manager.clone(),
            self.client_pool.clone(),
            self.message_storage_adapter.clone(),
        );
        let admin_handler = GrpcAdminServices::new(
            self.client_pool.clone(),
            self.metadata_cache.clone(),
            self.connection_manager.clone(),
            self.subscribe_manager.clone(),
            self.metrics_cache_manager.clone(),
        );
        Server::builder()
            .accept_http1(true)
            .layer(tower_http::cors::CorsLayer::very_permissive())
            .layer(tonic_web::GrpcWebLayer::new())
            .add_service(MqttBrokerInnerServiceServer::new(inner_handler))
            .add_service(MqttBrokerAdminServiceServer::new(admin_handler))
            .serve(addr)
            .await?;
        Ok(())
    }
}
