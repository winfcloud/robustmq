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

use crate::handler::cache::CacheManager;
use crate::observability::system_topic::report_system_data;
use grpc_clients::pool::ClientPool;
use std::sync::Arc;
use storage_adapter::storage::ArcStorageAdapter;

// metrics
pub(crate) const SYSTEM_TOPIC_BROKERS_METRICS_BYTES_RECEIVED: &str =
    "$SYS/brokers/${node}/metrics/bytes/received";
pub(crate) const SYSTEM_TOPIC_BROKERS_METRICS_BYTES_SENT: &str =
    "$SYS/brokers/${node}/metrics/bytes/sent";

pub(crate) async fn report_broker_metrics_bytes(
    client_pool: &Arc<ClientPool>,
    metadata_cache: &Arc<CacheManager>,
    message_storage_adapter: &ArcStorageAdapter,
) {
    report_system_data(
        client_pool,
        metadata_cache,
        message_storage_adapter,
        SYSTEM_TOPIC_BROKERS_METRICS_BYTES_RECEIVED,
        || async {
            "".to_string()
            // metadata_cache.get_bytes_received().to_string()
        },
    )
    .await;

    report_system_data(
        client_pool,
        metadata_cache,
        message_storage_adapter,
        SYSTEM_TOPIC_BROKERS_METRICS_BYTES_SENT,
        || async {
            "".to_string()
            // metadata_cache.get_bytes_sent().to_string()
        },
    )
    .await;
}
