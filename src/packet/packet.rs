use std::net::Ipv4Addr;
use crate::agent::agent::{Action,Add,Get,calculate_hash, n_mod_m};
use crate::flow::flow::{FlowKey,Flow};
use std::sync::Arc;
use std::sync::RwLock;
use std::collections::HashMap;
use ipnet;
use ipnet::IpAddrRange;
use rand::Rng;
use std::net::IpAddr;

#[derive(Clone)]
pub struct Datapath{
    flow_partition_sender_list: Arc<RwLock<HashMap<i16, crossbeam_channel::Sender<Action>>>>,
    src_hosts: Arc<RwLock<Vec<IpAddr>>>,
    dst_hosts: Arc<RwLock<Vec<IpAddr>>>,
    partitions: i16,
    hit_flow_counter: Arc<RwLock<u64>>,
    pub name: String,
}

impl Datapath {
    pub fn new(flow_partition_sender_list: Arc<RwLock<HashMap<i16, crossbeam_channel::Sender<Action>>>>, src_ips: Vec<IpAddr>, dst_ips: Vec<IpAddr>, partitions: i16, name: String) -> Self {
        Self{
            flow_partition_sender_list,
            src_hosts: Arc::new(RwLock::new(src_ips)),
            dst_hosts: Arc::new(RwLock::new(dst_ips)),
            partitions,
            hit_flow_counter: Arc::new(RwLock::new(0)),
            name,
        }
    }
    pub fn get_packet(mut self) -> Result<Packet, Box<dyn std::error::Error>> {
        let mut rng = rand::thread_rng();
        let mut src_ip_res: Ipv4Addr;
        let mut dst_ip_res: Ipv4Addr;
        loop{
            let (src_ip, dst_ip) = self.clone().random().unwrap();
            if src_ip != dst_ip {
                src_ip_res = src_ip;
                dst_ip_res = dst_ip;
                break;
                
            }
        }
        let mut rng = rand::thread_rng();
        let random_src_port: u16 = rng.gen_range(1..65535);
        let random_dst_port: u16 = rng.gen_range(1..65535);
        return Ok(Packet{
            src_ip: src_ip_res,
            dst_ip: dst_ip_res,
            src_port: random_src_port,
            dst_port: random_dst_port
        });
    }
    pub fn random(self) -> Result<(Ipv4Addr,Ipv4Addr), Box<dyn std::error::Error>> {
        let src_hosts = self.src_hosts.write().unwrap();
        let dst_hosts = self.dst_hosts.write().unwrap();
        let mut rng = rand::thread_rng();
        let random_src_idx: usize = rng.gen_range(0..src_hosts.clone().len());
        let random_dst_idx: usize = rng.gen_range(0..dst_hosts.clone().len());
        let src_ip = src_hosts.get(random_src_idx);
        let dst_ip = dst_hosts.get(random_dst_idx);
        let mut src_ip_res: Ipv4Addr = "0.0.0.0".parse().unwrap();
        let mut dst_ip_res: Ipv4Addr = "0.0.0.0".parse().unwrap();
        if let IpAddr::V4(ip) = src_ip.unwrap(){
            src_ip_res = *ip;
        };
        if let IpAddr::V4(ip) = dst_ip.unwrap(){
            dst_ip_res = *ip;
        };
  
        Ok((src_ip_res, dst_ip_res))
    }

    pub fn generate_packets(self, num_of_packets: u32) -> Vec<Packet>{
        let mut packet_list = Vec::new();
        for p in 1..num_of_packets{
            let packet = self.clone().get_packet().unwrap();
            packet_list.push(packet);
        }
        packet_list
    }

    pub fn send(self, packet: Packet) {
        let flow_partition_sender_list = self.flow_partition_sender_list.write().unwrap();
        let flow_key = FlowKey{
            src_prefix: packet.src_ip,
            dst_prefix: packet.dst_ip,
            src_prefix_len: 32,
            dst_prefix_len: 32,
        };
        let flow_key_hash = calculate_hash(&flow_key);
        let partition = n_mod_m(flow_key_hash, self.partitions.try_into().unwrap());
        let partition_sender = flow_partition_sender_list.get(&partition.try_into().unwrap()).unwrap();
        let (action_sender,action_receiver): (crossbeam_channel::Sender<Flow>, crossbeam_channel::Receiver<Flow>) = crossbeam_channel::unbounded();
        partition_sender.send(Action::Get(Get::MatchFlow(flow_key, action_sender)));
        let flow = action_receiver.recv().unwrap();
        let mut hit_flow_counter = self.hit_flow_counter.write().unwrap();
        *hit_flow_counter = *hit_flow_counter +1;
        //println!("got flow {}", flow.nh);
        
    }

    pub fn flow_counter(self) {
        let mut hit_flow_counter = self.hit_flow_counter.write().unwrap();
        println!("hit {} flows", *hit_flow_counter);
    }

}
#[derive(Clone)]
pub struct Packet{
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
}

impl Packet{

}