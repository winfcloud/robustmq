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

use crate::handler::connection::Connection;
use crate::security::acl::metadata::AclMetadata;
use crate::security::AuthDriver;
use crate::storage::user::UserStorage;
use crate::storage::{cluster::ClusterStorage, topic::TopicStorage};
use crate::subscribe::subscriber::SubscribeData;
use clients::poll::ClientPool;
use common_base::config::broker_mqtt::broker_mqtt_conf;
use common_base::tools::now_second;
use dashmap::DashMap;
use log::warn;
use metadata_struct::acl::mqtt_acl::MQTTAcl;
use metadata_struct::acl::mqtt_blacklist::MQTTAclBlackList;
use metadata_struct::mqtt::cluster::MQTTClusterDynamicConfig;
use metadata_struct::mqtt::session::MQTTSession;
use metadata_struct::mqtt::topic::MQTTTopic;
use metadata_struct::mqtt::user::MQTTUser;
use protocol::broker_server::generate::placement::{
    MqttBrokerUpdateCacheActionType, MqttBrokerUpdateCacheResourceType, UpdateCacheRequest,
};
use protocol::mqtt::common::{MQTTProtocol, PublishProperties, Subscribe, SubscribeProperties};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::time::sleep;

#[derive(Clone, Serialize, Deserialize)]
pub enum MetadataCacheAction {
    Set,
    Del,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum MetadataCacheType {
    Cluster,
    User,
    Topic,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MetadataChangeData {
    pub action: MetadataCacheAction,
    pub data_type: MetadataCacheType,
    pub value: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionLiveTime {
    pub protobol: MQTTProtocol,
    pub keep_live: u16,
    pub heartbeat: u64,
}

#[derive(Clone)]
pub struct QosAckPacketInfo {
    pub sx: Sender<QosAckPackageData>,
    pub create_time: u64,
}

#[derive(Clone, Debug)]
pub struct QosAckPackageData {
    pub ack_type: QosAckPackageType,
    pub pkid: u16,
}

#[derive(Clone, PartialEq, PartialOrd, Debug)]
pub enum QosAckPackageType {
    PubAck,
    PubComp,
    PubRel,
    PubRec,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientPkidData {
    pub client_id: String,
    pub create_time: u64,
}

#[derive(Clone)]
pub struct CacheManager {
    pub client_poll: Arc<ClientPool>,

    // cluster_name
    pub cluster_name: String,

    // (cluster_name, Cluster)
    pub cluster_info: DashMap<String, MQTTClusterDynamicConfig>,

    // (username, User)
    pub user_info: DashMap<String, MQTTUser>,

    // (client_id, Session)
    pub session_info: DashMap<String, MQTTSession>,

    // (client_id, <path,SubscribeData>)
    pub subscribe_filter: DashMap<String, DashMap<String, SubscribeData>>,

    // (client_id, vec<pkid>)
    pub publish_pkid_info: DashMap<String, Vec<u16>>,

    // (connect_id, Connection)
    pub connection_info: DashMap<u64, Connection>,

    // (topic_name, Topic)
    pub topic_info: DashMap<String, MQTTTopic>,

    // (topic_id, topic_name)
    pub topic_id_name: DashMap<String, String>,

    // (client_id, HeartbeatShard)
    pub heartbeat_data: DashMap<String, ConnectionLiveTime>,

    //(client_id_pkid, AckPacketInfo)
    pub qos_ack_packet: DashMap<String, QosAckPacketInfo>,

    // (client_id_pkid, QosPkidData)
    pub client_pkid_data: DashMap<String, ClientPkidData>,

    // acl metadata
    pub acl_metadata: AclMetadata,
}

impl CacheManager {
    pub fn new(client_poll: Arc<ClientPool>, cluster_name: String) -> Self {
        let cache = CacheManager {
            client_poll,
            cluster_name,
            cluster_info: DashMap::with_capacity(1),
            user_info: DashMap::with_capacity(8),
            session_info: DashMap::with_capacity(8),
            topic_info: DashMap::with_capacity(8),
            topic_id_name: DashMap::with_capacity(8),
            connection_info: DashMap::with_capacity(8),
            subscribe_filter: DashMap::with_capacity(8),
            publish_pkid_info: DashMap::with_capacity(8),
            heartbeat_data: DashMap::with_capacity(8),
            qos_ack_packet: DashMap::with_capacity(8),
            client_pkid_data: DashMap::with_capacity(8),
            acl_metadata: AclMetadata::new(),
        };
        return cache;
    }

    pub fn add_client_subscribe(
        &self,
        client_id: String,
        protocol: MQTTProtocol,
        subscribe: Subscribe,
        subscribe_properties: Option<SubscribeProperties>,
    ) {
        for filter in subscribe.filters {
            if let Some(data) = self.subscribe_filter.get_mut(&client_id) {
                data.insert(
                    filter.path.clone(),
                    SubscribeData {
                        protocol: protocol.clone(),
                        filter,
                        subscribe_properties: subscribe_properties.clone(),
                    },
                );
            } else {
                let data = DashMap::with_capacity(8);
                data.insert(
                    filter.path.clone(),
                    SubscribeData {
                        protocol: protocol.clone(),
                        filter,
                        subscribe_properties: subscribe_properties.clone(),
                    },
                );
                self.subscribe_filter.insert(client_id.clone(), data);
            };
        }
    }

    pub fn remove_filter_by_pkid(&self, client_id: &String, filters: &Vec<String>) {
        for path in filters {
            if let Some(sub_list) = self.subscribe_filter.get_mut(client_id) {
                if sub_list.contains_key(path) {
                    sub_list.remove(path);
                }
            }
        }
    }

    pub fn remove_filter_by_client_id(&self, client_id: String) {
        self.subscribe_filter.remove(&client_id);
    }

    pub fn get_session_info(&self, client_id: &String) -> Option<MQTTSession> {
        if let Some(session) = self.session_info.get(client_id) {
            return Some(session.clone());
        }
        return None;
    }

    pub fn update_session_connect_id(&self, client_id: &String, connect_id: Option<u64>) {
        if let Some(mut session) = self.session_info.get_mut(client_id) {
            session.update_connnction_id(connect_id);
            if connect_id.is_none() {
                session.update_distinct_time()
            }
        }
    }

    pub fn apply(&self, data: String) {
        let data: MetadataChangeData = serde_json::from_str(&data).unwrap();
        match data.data_type {
            MetadataCacheType::User => match data.action {
                MetadataCacheAction::Set => {
                    let user: MQTTUser = serde_json::from_str(&data.value).unwrap();
                    self.add_user(user);
                }
                MetadataCacheAction::Del => self.del_user(data.value),
            },
            MetadataCacheType::Topic => match data.action {
                MetadataCacheAction::Set => {}
                MetadataCacheAction::Del => {}
            },
            MetadataCacheType::Cluster => match data.action {
                MetadataCacheAction::Set => {}
                MetadataCacheAction::Del => {}
            },
        }
    }

    pub fn set_cluster_info(&self, cluster: MQTTClusterDynamicConfig) {
        self.cluster_info.insert(self.cluster_name.clone(), cluster);
    }

    pub fn get_cluster_info(&self) -> MQTTClusterDynamicConfig {
        if let Some(cluster) = self.cluster_info.get(&self.cluster_name) {
            return cluster.clone();
        }
        return MQTTClusterDynamicConfig::new();
    }

    pub fn add_user(&self, user: MQTTUser) {
        self.user_info.insert(user.username.clone(), user);
    }

    pub fn del_user(&self, value: String) {
        let data: MQTTUser = serde_json::from_str(&value).unwrap();
        self.user_info.remove(&data.username);
    }

    pub fn add_session(&self, client_id: String, session: MQTTSession) {
        self.session_info.insert(client_id, session);
    }

    pub fn add_connection(&self, connect_id: u64, conn: Connection) {
        if let Some(mut session) = self.session_info.get_mut(&conn.client_id) {
            session.connection_id = Some(connect_id);
            self.connection_info.insert(connect_id, conn);
        }
    }

    pub fn add_topic(&self, topic_name: &String, topic: &MQTTTopic) {
        let t = topic.clone();
        self.topic_info.insert(topic_name.clone(), t.clone());
        self.topic_id_name.insert(t.topic_id, topic_name.clone());
    }

    pub fn update_topic_retain_message(
        &self,
        topic_name: &String,
        retain_message: Option<Vec<u8>>,
    ) {
        if let Some(mut topic) = self.topic_info.get_mut(topic_name) {
            topic.retain_message = retain_message;
        }
    }

    pub fn login_success(&self, connect_id: u64, user_name: String) {
        if let Some(mut conn) = self.connection_info.get_mut(&connect_id) {
            conn.login_success(user_name)
        }
    }

    pub fn is_login(&self, connect_id: u64) -> bool {
        if let Some(conn) = self.connection_info.get(&connect_id) {
            return conn.is_login;
        }
        return false;
    }

    pub fn topic_exists(&self, topic: &String) -> bool {
        return self.topic_info.contains_key(topic);
    }

    pub fn topic_name_by_id(&self, topic_id: String) -> Option<String> {
        if let Some(data) = self.topic_id_name.get(&topic_id) {
            return Some(data.clone());
        }
        return None;
    }

    pub fn get_topic_by_name(&self, topic_name: &String) -> Option<MQTTTopic> {
        if let Some(topic) = self.topic_info.get(topic_name) {
            return Some(topic.clone());
        }
        return None;
    }

    pub fn remove_session(&self, client_id: &String) {
        self.session_info.remove(client_id);
        self.subscribe_filter.remove(client_id);
        self.publish_pkid_info.remove(client_id);
        self.heartbeat_data.remove(client_id);

        for (key, _) in self.qos_ack_packet.clone() {
            if key.starts_with(client_id) {
                self.qos_ack_packet.remove(&key);
            }
        }

        for (key, _) in self.client_pkid_data.clone() {
            if key.starts_with(client_id) {
                self.qos_ack_packet.remove(&key);
            }
        }
    }

    pub fn remove_connection(&self, connect_id: u64) {
        self.connection_info.remove(&connect_id);
    }

    pub fn get_topic_alias(&self, connect_id: u64, topic_alias: u16) -> Option<String> {
        if let Some(conn) = self.connection_info.get(&connect_id) {
            if let Some(topic_name) = conn.topic_alias.get(&topic_alias) {
                return Some(topic_name.clone());
            } else {
                return None;
            }
        }
        return None;
    }

    pub fn topic_alias_exists(&self, connect_id: u64, topic_alias: u16) -> bool {
        if let Some(conn) = self.connection_info.get(&connect_id) {
            return conn.topic_alias.contains_key(&topic_alias);
        }
        return false;
    }

    pub fn add_topic_alias(
        &self,
        connect_id: u64,
        topic_name: &String,
        publish_properties: &Option<PublishProperties>,
    ) {
        if let Some(properties) = publish_properties {
            if let Some(alias) = properties.topic_alias {
                if let Some(conn) = self.connection_info.get_mut(&connect_id) {
                    conn.topic_alias.insert(alias, topic_name.clone());
                }
            }
        }
    }

    pub fn get_connect_id(&self, client_id: &String) -> Option<u64> {
        if let Some(sess) = self.session_info.get(client_id) {
            if let Some(conn_id) = sess.connection_id {
                return Some(conn_id);
            }
        }
        return None;
    }

    pub fn get_connection(&self, connect_id: u64) -> Option<Connection> {
        if let Some(conn) = self.connection_info.get(&connect_id) {
            return Some(conn.clone());
        }
        return None;
    }

    pub async fn get_pkid(&self, client_id: &String) -> u16 {
        let pkid = self.get_available_pkid(client_id).await;
        if let Some(mut pkid_list) = self.publish_pkid_info.get_mut(client_id) {
            pkid_list.push(pkid);
        } else {
            self.publish_pkid_info.insert(client_id.clone(), vec![pkid]);
        }
        return pkid;
    }

    async fn get_available_pkid(&self, client_id: &String) -> u16 {
        loop {
            if let Some(pkid_list) = self.publish_pkid_info.get(client_id) {
                for i in 1..65535 {
                    if pkid_list.contains(&i) {
                        continue;
                    }
                    return i;
                }
            } else {
                self.publish_pkid_info.insert(client_id.clone(), vec![1]);
                return 1;
            }
            sleep(Duration::from_millis(10)).await;
            warn!("{}", "No pkid available for client, wait 10ms.");
        }
    }

    pub fn remove_pkid_info(&self, client_id: &String, pkid: u16) {
        if let Some(mut pkid_list) = self.publish_pkid_info.get_mut(client_id) {
            pkid_list.retain(|x| *x == pkid);
        }
    }

    pub fn is_new_sub(&self, client_id: &String, path: &String) -> bool {
        if let Some(sub) = self.subscribe_filter.get(client_id) {
            return !sub.contains_key(path);
        }
        return true;
    }

    pub async fn load_metadata_cache(&self, auth_driver: Arc<AuthDriver>) {
        let conf = broker_mqtt_conf();
        // load cluster config
        let cluster_storage = ClusterStorage::new(self.client_poll.clone());
        let cluster = match cluster_storage
            .get_cluster_config(conf.cluster_name.clone())
            .await
        {
            Ok(Some(cluster)) => cluster,
            Ok(None) => MQTTClusterDynamicConfig::new(),
            Err(e) => {
                panic!(
                    "Failed to load the cluster configuration with error message:{}",
                    e.to_string()
                );
            }
        };
        self.set_cluster_info(cluster);

        // load all topic
        let topic_storage = TopicStorage::new(self.client_poll.clone());
        let topic_list = match topic_storage.topic_list().await {
            Ok(list) => list,
            Err(e) => {
                panic!(
                    "Failed to load the topic list with error message:{}",
                    e.to_string()
                );
            }
        };

        for (_, topic) in topic_list {
            self.add_topic(&topic.topic_name, &topic);
        }

        // load all user
        let user_list = match auth_driver.read_all_user().await {
            Ok(list) => list,
            Err(e) => {
                panic!(
                    "Failed to load the user list with error message:{}",
                    e.to_string()
                );
            }
        };

        for (_, user) in user_list {
            self.add_user(user);
        }

        // load all acl
        let acl_list = match auth_driver.read_all_acl().await {
            Ok(list) => list,
            Err(e) => {
                panic!(
                    "Failed to load the acl list with error message:{}",
                    e.to_string()
                );
            }
        };
        for acl in acl_list {
            self.add_acl(acl);
        }

        // load all blacklist
        let blacklist_list = match auth_driver.read_all_blacklist().await {
            Ok(list) => list,
            Err(e) => {
                panic!(
                    "Failed to load the blacklist list with error message:{}",
                    e.to_string()
                );
            }
        };
        for blacklist in blacklist_list {
            self.add_blacklist(blacklist);
        }
    }

    pub async fn init_system_user(&self) {
        // init system user
        let conf = broker_mqtt_conf();
        let system_user_info = MQTTUser {
            username: conf.system.default_user.clone(),
            password: conf.system.default_password.clone(),
            is_superuser: true,
        };
        let user_storage = UserStorage::new(self.client_poll.clone());
        match user_storage.save_user(system_user_info.clone()).await {
            Ok(_) => {
                self.add_user(system_user_info);
            }
            Err(e) => {
                panic!("{}", e.to_string());
            }
        }
    }

    pub fn report_heartbeat(&self, client_id: &String, live_time: ConnectionLiveTime) {
        self.heartbeat_data.insert(client_id.clone(), live_time);
    }

    pub fn remove_heartbeat(&self, client_id: &String) {
        self.heartbeat_data.remove(client_id);
    }

    pub fn add_ack_packet(&self, client_id: &String, pkid: u16, packet: QosAckPacketInfo) {
        let key = self.key(client_id, pkid);
        self.qos_ack_packet.insert(key, packet);
    }

    pub fn add_acl(&self, acl: MQTTAcl) {
        self.acl_metadata.parse_mqtt_acl(acl);
    }

    pub fn add_blacklist(&self, blacklist: MQTTAclBlackList) {
        self.acl_metadata.parse_mqtt_blacklist(blacklist);
    }

    pub fn remove_ack_packet(&self, client_id: &String, pkid: u16) {
        let key = self.key(client_id, pkid);
        self.qos_ack_packet.remove(&key);
    }

    pub fn get_ack_packet(&self, client_id: String, pkid: u16) -> Option<QosAckPacketInfo> {
        let key = self.key(&client_id, pkid);
        if let Some(data) = self.qos_ack_packet.get(&key) {
            return Some(data.clone());
        }
        return None;
    }

    pub fn add_client_pkid(&self, client_id: &String, pkid: u16) {
        let key = self.key(client_id, pkid);
        self.client_pkid_data.insert(
            key,
            ClientPkidData {
                client_id: client_id.clone(),
                create_time: now_second(),
            },
        );
    }

    pub fn delete_client_pkid(&self, client_id: &String, pkid: u16) {
        let key = self.key(client_id, pkid);
        self.client_pkid_data.remove(&key);
    }

    pub fn get_client_pkid(&self, client_id: &String, pkid: u16) -> Option<ClientPkidData> {
        let key = self.key(client_id, pkid);
        if let Some(data) = self.client_pkid_data.get(&key) {
            return Some(data.clone());
        }
        return None;
    }

    fn key(&self, client_id: &String, pkid: u16) -> String {
        return format!("{}_{}", client_id, pkid);
    }
}

pub fn update_cache_metadata(request: UpdateCacheRequest) {
    match request.resource_type() {
        MqttBrokerUpdateCacheResourceType::Session => match request.action_type() {
            MqttBrokerUpdateCacheActionType::Add => {}
            MqttBrokerUpdateCacheActionType::Delete => {}
        },
        MqttBrokerUpdateCacheResourceType::User => match request.action_type() {
            MqttBrokerUpdateCacheActionType::Add => {}
            MqttBrokerUpdateCacheActionType::Delete => {}
        },
    }
}
