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

use super::{cache::CacheManager, keep_alive::client_keep_live_time};
use crate::{
    server::connection_manager::ConnectionManager, storage::session::SessionStorage,
    subscribe::subscribe_manager::SubscribeManager,
};
use clients::poll::ClientPool;
use common_base::{
    error::common::CommonError,
    tools::{now_second, unique_id},
};
use dashmap::DashMap;
use metadata_struct::mqtt::cluster::MQTTClusterDynamicConfig;
use protocol::mqtt::common::{Connect, ConnectProperties};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicIsize, Ordering},
        Arc,
    },
};

pub const REQUEST_RESPONSE_PREFIX_NAME: &str = "/sys/request_response/";

#[derive(Default, Clone, Debug)]
pub struct Connection {
    // Connection ID
    pub connect_id: u64,
    // Each connection has a unique Client ID
    pub client_id: String,
    // Mark whether the link is already logged in
    pub is_login: bool,
    //
    pub source_ip_addr: String,
    //
    pub login_user: String,
    // When the client does not report a heartbeat, the maximum survival time of the connection,
    pub keep_alive: u16,
    // Records the Topic alias information for the connection dimension
    pub topic_alias: DashMap<u16, String>,
    // Record the maximum number of QOS1 and QOS2 packets that the client can send in connection dimension. Scope of data flow control.
    pub client_max_receive_maximum: u16,
    // Record the connection dimension, the size of the maximum request packet that can be received.
    pub max_packet_size: u32,
    // Record the maximum number of connection dimensions and topic aliases. The default value ranges from 0 to 65535
    pub topic_alias_max: u16,
    // Flags whether to return a detailed error message to the client when an error occurs.
    pub request_problem_info: u8,
    // Flow control part keeps track of how many QOS 1 and QOS 2 messages are still pending on the connection
    pub receive_qos_message: Arc<AtomicIsize>,
    // Flow control part keeps track of how many QOS 1 and QOS 2 messages are still pending on the connection
    pub sender_qos_message: Arc<AtomicIsize>,
    // Time when the connection was created
    pub create_time: u64,
}

impl Connection {
    pub fn new(
        connect_id: u64,
        client_id: &String,
        receive_maximum: u16,
        max_packet_size: u32,
        topic_alias_max: u16,
        request_problem_info: u8,
        keep_alive: u16,
        source_ip_addr: String,
    ) -> Connection {
        return Connection {
            connect_id,
            client_id: client_id.clone(),
            is_login: false,
            keep_alive,
            client_max_receive_maximum: receive_maximum,
            max_packet_size,
            topic_alias: DashMap::with_capacity(2),
            topic_alias_max,
            request_problem_info,
            receive_qos_message: Arc::new(AtomicIsize::new(0)),
            sender_qos_message: Arc::new(AtomicIsize::new(0)),
            create_time: now_second(),
            source_ip_addr,
            ..Default::default()
        };
    }

    pub fn login_success(&mut self, user_name: String) {
        self.is_login = true;
        self.login_user = user_name;
    }

    pub fn is_response_proplem_info(&self) -> bool {
        return self.request_problem_info == 1;
    }

    pub fn get_recv_qos_message(&self) -> isize {
        return self.receive_qos_message.fetch_add(0, Ordering::Relaxed);
    }

    pub fn recv_qos_message_incr(&self) {
        self.receive_qos_message.fetch_add(1, Ordering::Relaxed);
    }

    pub fn recv_qos_message_decr(&self) {
        self.receive_qos_message.fetch_add(-1, Ordering::Relaxed);
    }

    pub fn get_send_qos_message(&self) -> isize {
        return self.sender_qos_message.fetch_add(0, Ordering::Relaxed);
    }

    pub fn send_qos_message_incr(&self) {
        self.sender_qos_message.fetch_add(1, Ordering::Relaxed);
    }

    pub fn send_qos_message_decr(&self) {
        self.sender_qos_message.fetch_add(-1, Ordering::Relaxed);
    }
}

pub fn build_connection(
    connect_id: u64,
    client_id: &String,
    cluster: &MQTTClusterDynamicConfig,
    connect: &Connect,
    connect_properties: &Option<ConnectProperties>,
    addr: &SocketAddr,
) -> Connection {
    let keep_alive = client_keep_live_time(cluster, connect.keep_alive);

    let (client_receive_maximum, max_packet_size, topic_alias_max, request_problem_info) =
        if let Some(properties) = connect_properties {
            let client_receive_maximum = if let Some(value) = properties.receive_maximum {
                value
            } else {
                cluster.protocol.receive_max
            };

            let max_packet_size = if let Some(value) = properties.max_packet_size {
                std::cmp::min(value, cluster.protocol.max_packet_size)
            } else {
                cluster.protocol.max_packet_size
            };

            let topic_alias_max = if let Some(value) = properties.topic_alias_max {
                std::cmp::min(value, cluster.protocol.topic_alias_max)
            } else {
                cluster.protocol.topic_alias_max
            };

            let request_problem_info = if let Some(value) = properties.request_problem_info {
                value
            } else {
                0
            };

            (
                client_receive_maximum,
                max_packet_size,
                topic_alias_max,
                request_problem_info,
            )
        } else {
            (
                cluster.protocol.receive_max,
                cluster.protocol.max_packet_size,
                cluster.protocol.topic_alias_max,
                0,
            )
        };
    return Connection::new(
        connect_id,
        &client_id,
        client_receive_maximum,
        max_packet_size,
        topic_alias_max,
        request_problem_info,
        keep_alive,
        addr.to_string(),
    );
}

pub fn get_client_id(client_id: &String) -> (String, bool) {
    if client_id.is_empty() {
        return (unique_id(), true);
    } else {
        return (client_id.clone(), false);
    };
}

pub fn response_information(connect_properties: &Option<ConnectProperties>) -> Option<String> {
    if let Some(properties) = connect_properties {
        if let Some(request_response_info) = properties.request_response_info {
            if request_response_info == 1 {
                return Some(REQUEST_RESPONSE_PREFIX_NAME.to_string());
            }
        }
    }
    return None;
}

pub async fn disconnect_connection(
    client_id: &String,
    connect_id: u64,
    cache_manager: &Arc<CacheManager>,
    client_poll: &Arc<ClientPool>,
    connnection_manager: &Arc<ConnectionManager>,
    subscribe_manager: &Arc<SubscribeManager>,
) -> Result<(), CommonError> {
    // Remove the connection cache
    cache_manager.remove_connection(connect_id);
    // Remove the client id bound connection information
    cache_manager.update_session_connect_id(client_id, None);
    // Once the connection is dropped, the push thread for the Client ID dimension is paused
    subscribe_manager.stop_push_by_client_id(&client_id);

    // Remove the Connect id of the Session in the Placement Center
    let session_storage = SessionStorage::new(client_poll.clone());
    match session_storage
        .update_session(client_id, 0, 0, 0, now_second())
        .await
    {
        Ok(_) => {}
        Err(e) => {
            return Err(e);
        }
    }

    // Close the real network connection
    connnection_manager.clonse_connect(connect_id).await;
    return Ok(());
}

#[cfg(test)]
mod test {
    use super::build_connection;
    use super::get_client_id;
    use super::response_information;
    use super::Connection;
    use super::REQUEST_RESPONSE_PREFIX_NAME;
    use metadata_struct::mqtt::cluster::MQTTClusterDynamicConfig;
    use protocol::mqtt::common::Connect;
    use protocol::mqtt::common::ConnectProperties;

    #[tokio::test]
    pub async fn build_connection_test() {
        let connect_id = 1;
        let client_id = "client_id-***".to_string();
        let cluster = MQTTClusterDynamicConfig::new();
        let connect = Connect {
            keep_alive: 10,
            client_id: client_id.clone(),
            clean_session: true,
        };
        let connect_properties = ConnectProperties {
            session_expiry_interval: Some(60),
            receive_maximum: Some(100),
            max_packet_size: Some(100),
            request_problem_info: Some(0),
            request_response_info: Some(0),
            topic_alias_max: Some(100),
            user_properties: Vec::new(),
            authentication_method: None,
            authentication_data: None,
        };
        let addr = format!("0.0.0.0:8080").parse().unwrap();
        let mut conn = build_connection(
            connect_id,
            &client_id,
            &cluster,
            &connect,
            &Some(connect_properties),
            &addr,
        );
        assert_eq!(conn.connect_id, connect_id);
        assert_eq!(conn.client_id, client_id);
        assert!(!conn.is_login);
        conn.login_success("".into());
        assert!(conn.is_login);
        assert_eq!(conn.keep_alive, 10);
        assert_eq!(conn.client_max_receive_maximum, 100);
        assert_eq!(conn.max_packet_size, 100);
        assert_eq!(conn.topic_alias_max, 100);
        assert_eq!(conn.request_problem_info, 0);
    }

    #[tokio::test]
    pub async fn get_client_id_test() {
        let client_id = "".to_string();
        let (new_client_id, is_new) = get_client_id(&client_id);
        assert!(is_new);
        assert!(!new_client_id.is_empty());

        let client_id = "client_id-***".to_string();
        let (new_client_id, is_new) = get_client_id(&client_id);
        assert!(!is_new);
        assert_eq!(new_client_id, client_id);
        assert!(!new_client_id.is_empty());
    }

    #[tokio::test]
    pub async fn response_information_test() {
        let mut connect_properties = ConnectProperties::default();
        connect_properties.request_response_info = Some(1);
        let res = response_information(&Some(connect_properties));
        assert_eq!(res.unwrap(), REQUEST_RESPONSE_PREFIX_NAME.to_string());

        let res = response_information(&Some(ConnectProperties::default()));
        assert!(res.is_none());

        let mut connect_properties = ConnectProperties::default();
        connect_properties.request_response_info = Some(0);
        let res = response_information(&Some(connect_properties));
        assert!(res.is_none());
    }

    #[tokio::test]
    pub async fn recv_qos_message_num_test() {
        let conn = Connection::default();
        assert_eq!(conn.get_recv_qos_message(), 0);
        conn.recv_qos_message_incr();
        assert_eq!(conn.get_recv_qos_message(), 1);
        conn.recv_qos_message_decr();
        assert_eq!(conn.get_recv_qos_message(), 0);
    }

    #[tokio::test]
    pub async fn send_qos_message_num_test() {
        let conn = Connection::default();
        assert_eq!(conn.get_send_qos_message(), 0);
        conn.send_qos_message_incr();
        assert_eq!(conn.get_send_qos_message(), 1);
        conn.send_qos_message_decr();
        assert_eq!(conn.get_send_qos_message(), 0);
    }
}
