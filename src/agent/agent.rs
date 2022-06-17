use crate::flow::flow::{FlowTable,Flow,FlowKey, FlowTableStats};
use crate::config::config::Vmi;
use crate::control::control::Route;

use std::sync::Arc;
use std::collections::HashMap;
use std::sync::RwLock;
use ipnet;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use parking_lot::{Mutex, Condvar, RawMutex};



pub enum Add{
    Vmi(Vmi),
    Route(Route),
    Flow(Flow),
}

pub enum Get{
    FlowTableStats(crossbeam_channel::Sender<FlowTableStats>),
    Flows(crossbeam_channel::Sender<Vec<Flow>>),
    MatchFlow(FlowKey,crossbeam_channel::Sender<Flow>,Arc<(Mutex<bool>, Condvar)>),
}

pub enum Action{
    Add(Add),
    Get(Get),
}

#[derive(Clone)]
pub struct Agent{
    pub name: String,
    partition_ready_receiver: crossbeam_channel::Receiver<bool>,
    partition_ready_sender: crossbeam_channel::Sender<bool>,
    pub add_sender: crossbeam_channel::Sender<Action>,
    add_receiver: crossbeam_channel::Receiver<Action>,
    route_sender: crossbeam_channel::Sender<Add>,
    local_route_table: Arc<RwLock<HashMap<ipnet::Ipv4Net,String>>>,
    remote_route_table: Arc<RwLock<HashMap<ipnet::Ipv4Net,String>>>,
    vmi_table: Arc<RwLock<HashMap<ipnet::Ipv4Net,String>>>,
    partitions: i16,
    ft: FlowTable,
}

impl Agent {
    pub fn new(name: String, route_sender: crossbeam_channel::Sender<Add>, partitions: i16) -> Self {
        let (partition_ready_sender,partition_ready_receiver): (crossbeam_channel::Sender<bool>, crossbeam_channel::Receiver<bool>) = crossbeam_channel::unbounded();
        let (add_sender,add_receiver): (crossbeam_channel::Sender<Action>, crossbeam_channel::Receiver<Action>) = crossbeam_channel::unbounded();

        Self{
            name: name.clone(),
            partition_ready_receiver,
            partition_ready_sender: partition_ready_sender.clone(),
            add_sender,
            add_receiver,
            route_sender,
            local_route_table: Arc::new(RwLock::new(HashMap::new())),
            remote_route_table: Arc::new(RwLock::new(HashMap::new())),
            vmi_table: Arc::new(RwLock::new(HashMap::new())),
            partitions,
            ft: FlowTable::new(name.clone(), partition_ready_sender.clone()),
        }
    }

    pub fn get_partition_list(self) -> Arc<RwLock<HashMap<i16, crossbeam_channel::Sender<Action>>>>{
        self.ft.get_partition_list()
    }

    pub fn get_stats(self) {
        self.ft.get_stats();
    }

    pub fn get_flows(self) {
        self.ft.get_flows();
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send>>{
        //let ft = FlowTable::new(self.name.clone(), self.partition_ready_sender.clone());
        let mut join_handlers = Vec::new();
        let ft_join_handler = tokio::task::spawn(self.ft.clone().run(self.partitions));
        join_handlers.push(ft_join_handler);

        let receive_join_handler = tokio::task::spawn(self.receive());
        join_handlers.push(receive_join_handler);

        futures::future::join_all(join_handlers).await;
        Ok(())
    }

    pub async fn receive(self) -> Result<(), Box<dyn std::error::Error + Send>>{
        tokio::task::spawn_blocking(move || {
            loop {
                let add = self.add_receiver.recv();
                match add.unwrap() {
                    Action::Add(Add::Vmi(vmi)) => {
                        //println!("{} got vmi {}", self.name, vmi.agent);
                        let mut vmi_table = self.vmi_table.write().unwrap();
                        let vmi_name = format!("{}{}", vmi.clone().name, vmi_table.len());
                        vmi_table.insert(vmi.clone().ip, vmi_name);
                        self.route_sender.send(Add::Route(Route{
                            dst: vmi.ip,
                            nh: vmi.agent,
                        }));
                    },
                    Action::Add(Add::Route(route)) => {
                        //println!("{} got route {} {}", self.name, route.dst, route.nh);
                        if route.nh == self.name {
                            
                            let vmi_table = self.vmi_table.write().unwrap();
                            let nh = vmi_table.get(&route.dst).unwrap();

                            let mut local_rt = self.local_route_table.write().unwrap();
                            local_rt.insert(route.clone().dst, nh.clone());
                            for (dst, nh) in local_rt.clone() {
                                if route.clone().dst != dst {
                                    let ingress_flow = Flow{
                                        flow_key: FlowKey{
                                            src_prefix: route.clone().dst.addr(),
                                            src_prefix_len: route.clone().dst.prefix_len(),
                                            dst_prefix: dst.clone().addr(),
                                            dst_prefix_len: dst.clone().prefix_len(),
                                        },
                                        nh: nh.clone(),
                                    };

                                    let flow_key_hash = calculate_hash(&ingress_flow.flow_key);
                                    let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
                                    self.ft.clone().send_flow(ingress_flow, partition.try_into().unwrap());
                                    
                                    let vmi = vmi_table.get(&route.clone().dst).unwrap();
                                    let egress_flow = Flow{
                                        flow_key: FlowKey{
                                            src_prefix: dst.clone().addr(),
                                            src_prefix_len: dst.clone().prefix_len(),
                                            dst_prefix: route.clone().dst.addr(),
                                            dst_prefix_len: route.clone().dst.prefix_len(),
                                        },
                                        nh: vmi.clone(),
                                    };

                                    let flow_key_hash = calculate_hash(&egress_flow.flow_key);
                                    let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
                                    self.ft.clone().send_flow(egress_flow, partition.try_into().unwrap());

                                }
                            }

                            let mut remote_rt = self.remote_route_table.write().unwrap();
                            for (dst, nh) in remote_rt.clone() {
                                let ingress_flow = Flow{
                                    flow_key: FlowKey{
                                        src_prefix: route.clone().dst.addr(),
                                        src_prefix_len: route.clone().dst.prefix_len(),
                                        dst_prefix: dst.clone().addr(),
                                        dst_prefix_len: dst.clone().prefix_len(),
                                    },
                                    nh: nh.clone()
                                };

                                let flow_key_hash = calculate_hash(&ingress_flow.flow_key);
                                let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
                                self.ft.clone().send_flow(ingress_flow, partition.try_into().unwrap());

                                let egress_flow = Flow{
                                    flow_key: FlowKey{
                                        src_prefix: dst.clone().addr(),
                                        src_prefix_len: dst.clone().prefix_len(),
                                        dst_prefix: route.clone().dst.addr(),
                                        dst_prefix_len: route.clone().dst.prefix_len(),
                                    },
                                    nh: nh,
                                };

                                let flow_key_hash = calculate_hash(&egress_flow.flow_key);
                                let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
                                self.ft.clone().send_flow(egress_flow, partition.try_into().unwrap());
                            }


                        } else {
                            let vmi_table = self.vmi_table.write().unwrap();
                            let mut rt = self.remote_route_table.write().unwrap();
                            rt.insert(route.clone().dst, route.clone().nh);
                            let mut local_rt = self.local_route_table.write().unwrap();
                            for (dst, nh) in local_rt.clone() {
                                if route.clone().dst != dst {
                                    let vmi = vmi_table.get(&dst).unwrap();
                                    let ingress_flow = Flow{
                                        flow_key: FlowKey{
                                            src_prefix: route.clone().dst.addr(),
                                            src_prefix_len: route.clone().dst.prefix_len(),
                                            dst_prefix: dst.clone().addr(),
                                            dst_prefix_len: dst.clone().prefix_len(),
                                        },
                                        nh: vmi.clone(),
                                    };

                                    let flow_key_hash = calculate_hash(&ingress_flow.flow_key);
                                    let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
                                    self.ft.clone().send_flow(ingress_flow, partition.try_into().unwrap());

                                    let egress_flow = Flow{
                                        flow_key: FlowKey{
                                            src_prefix: dst.clone().addr(),
                                            src_prefix_len: dst.clone().prefix_len(),
                                            dst_prefix: route.clone().dst.addr(),
                                            dst_prefix_len: route.clone().dst.prefix_len(),
                                        },
                                        nh: route.clone().nh,
                                    };

                                    let flow_key_hash = calculate_hash(&egress_flow.flow_key);
                                    let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
                                    self.ft.clone().send_flow(egress_flow, partition.try_into().unwrap());
                                }
                            }
                        }

                    },
                    _ => {},
                }
            }
        });
        Ok(())
    }

    pub fn wait_for_partitions(self){
        loop{
            let _res = self.partition_ready_receiver.recv().unwrap();
            break
        }
    }

    pub fn get_route_tables(self) {
        println!("getting route tables");
        let rt = self.local_route_table.write().unwrap();
        for (dst, nh) in rt.clone() {
            println!("local dst {} nh {}", dst, nh);
        }
        let rt = self.remote_route_table.write().unwrap();
        for (dst, nh) in rt.clone() {
            println!("remote dst {} nh {}", dst, nh);
        }
    }

}

pub fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub fn n_mod_m <T: std::ops::Rem<Output = T> + std::ops::Add<Output = T> + Copy>
  (n: T, m: T) -> T {
    ((n % m) + m) % m
}