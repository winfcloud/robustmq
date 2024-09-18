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

#[cfg(test)]
mod tests {
    use crate::common::get_placement_addr;
    use clients::{
        placement::{mqtt::call::placement_get_share_sub_leader, placement::call::register_node},
        poll::ClientPool,
    };
    use protocol::placement_center::generate::{common::ClusterType, mqtt::GetShareSubLeaderRequest, placement::RegisterNodeRequest};
    use std::sync::Arc;

    #[tokio::test]
    async fn mqtt_share_sub_test() {
        let client_poll: Arc<ClientPool> = Arc::new(ClientPool::new(3));
        let addrs = vec![get_placement_addr()];
        let cluster_name: String = "test_cluster".to_string();
        let group_name: String = "test_group".to_string();
        let node_ip: String = "127.0.0.1".to_string();
        let node_id: u64 = 1;

        
        let request = RegisterNodeRequest {
            cluster_type: ClusterType::MqttBrokerServer as i32,
            cluster_name: cluster_name.clone(),
            node_ip: node_ip.clone(),
            node_id: node_id.clone(),
            node_inner_addr: node_ip.clone(),
            extend_info: "".to_string(),
        };
        match register_node(client_poll.clone(), addrs.clone(), request).await {
            Ok(_) => {}
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        };

        let request = GetShareSubLeaderRequest {
            group_name: group_name.clone(),
            cluster_name: cluster_name.clone(),
        };
        match placement_get_share_sub_leader(client_poll.clone(), addrs.clone(), request).await {
            Ok(data) => {
                let mut flag = false;
                if data.broker_id == node_id
                && data.broker_addr == node_ip
                && data.extend_info == "" {
                    flag = true;
                }
                assert!(flag);
            }
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }
    }
}