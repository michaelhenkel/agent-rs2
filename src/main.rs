mod agent;
mod flow;
mod config;
mod control;
mod packet;

use core::num;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::Arc;
use std::sync::Mutex;
use std::process::exit;
use std::{thread::sleep, fmt::format};
use std::time::{Instant,Duration};
use std::env;
use agent::agent::Agent;
use packet::packet::{Datapath,Packet};
use config::config::{Config, Vmi};
use control::control::Control;
use ipnet::{self, IpAddrRange, Ipv4AddrRange};
use std::net::{IpAddr,Ipv4Addr};
use clap::Parser;


#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {

    #[clap(short, long, value_parser, default_value_t = 1)]
    agents: i32,

    #[clap(short, long, value_parser, default_value_t = 1)]
    vmis: u32,

    #[clap(short, long, value_parser, default_value_t = 1)]
    threads: i16,

    #[clap(short, long, value_parser, default_value_t = 0)]
    packets: u32,

    #[clap(short, long, value_parser, default_value_t = false)]
    stats: bool,

    #[clap(short, long, value_parser, default_value_t = false)]
    flows: bool,
}

fn as_u32_be(array: &[u8;4]) -> u32 {
    ((array[0] as u32) << 24) +
    ((array[1] as u32) << 16) +
    ((array[2] as u32) << 8) +
    ((array[3] as u32) << 0)
}

fn as_u32_le(array: &[u8;4]) -> u32 {
    ((array[0] as u32) << 0) +
    ((array[1] as u32) << 8) +
    ((array[2] as u32) << 16) +
    ((array[3] as u32) << 24)
}

fn as_br(x: u32) -> [u8; 4]{
    x.to_be_bytes()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    let args = Args::parse();
    let mut join_handlers = Vec::new();

    let config = Config::new();
    let control = Control::new();

    let control_handle = tokio::task::spawn(control.clone().run());
    join_handlers.push(control_handle);

    let num_agents = args.agents;
    let num_vmis = args.vmis;
    let num_partitions = args.threads;
    let num_packets = args.packets;

    let mut agent_list = Vec::new();
    
    for n in 0..num_agents{
        let agent_name = format!("agent{}", n);
        let agent = Agent::new(agent_name, control.clone().route_sender, num_partitions);
        let agent_res = tokio::task::spawn(agent.clone().run());
        join_handlers.push(agent_res);
    
        config.clone().add_agent(agent.name.clone(), agent.add_sender.clone());
        control.clone().add_agent(agent.name.clone(), agent.add_sender.clone());
        agent.clone().wait_for_partitions();

        agent_list.push(agent);
    }

    let mut agent_ips: HashMap<String, Vec<IpAddr>> = HashMap::new();
    let mut all_ips = Vec::new();

    for agent in 0..num_agents{
        let agent_name = format!("agent{}", agent);
        let mut vmi_list = Vec::new();
        for vmi in 0..num_vmis {
            let agent_ip = format!{"{}.1.1.0/32", agent+1};
            let vmi_ip: ipnet::Ipv4Net = agent_ip.parse().unwrap();
            let octets = vmi_ip.addr().octets();
            let mut ip_bin = as_u32_be(&octets);
            ip_bin = ip_bin +vmi;
            let new_octets = as_br(ip_bin);
            let new_ip = IpAddr::from(new_octets);
            vmi_list.push(new_ip);
            all_ips.push(new_ip);
            let ip = format!{"{}/32", new_ip};
                config.clone().add_vmi(Vmi { 
                    name: "vmi".to_string(),
                    ip: ip.parse().unwrap(),
                    agent: agent_name.clone(),
            });
        }
        agent_ips.insert(agent_name, vmi_list);
    }

    sleep(Duration::from_secs(2));

    if args.packets > 0{
        let mut datapath_list = HashMap::new();
        for agent in agent_list.clone() {
            let local_agent_ips = agent_ips.get(&agent.name).unwrap();
            let partition_sender_list = agent.clone().get_partition_list();
            let dp = Datapath::new(partition_sender_list, all_ips.clone(), local_agent_ips.clone(), num_partitions, agent.clone().name);
            datapath_list.insert(agent.clone().name, dp);
        }

        println!("starting packet send");

        let packet_list: Arc<Mutex<HashMap<String, Vec<Packet>>>> = Arc::new(Mutex::new(HashMap::new()));
        for (name, dp) in datapath_list.clone(){ 
            let packets = dp.generate_packets(num_packets);
            let mut p_list = packet_list.lock().unwrap();
            p_list.insert(name, packets);
        }


        let now = Instant::now(); 
        let mut sender_list = Vec::new();
        for (name, dp) in datapath_list.clone(){
            let p_map_list = packet_list.clone();
            let res = tokio::spawn(async move{
                let p_map_list = p_map_list.lock().unwrap();
                let p_list = p_map_list.get(&name).unwrap();
                for p in p_list {
                    dp.clone().send(p.clone());
                }
            });
            sender_list.push(res);
        }
        futures::future::join_all(sender_list).await;
        println!("millisecs {}",now.elapsed().as_millis());
    }

    if args.flows{
        for agent in agent_list.clone() {
            agent.get_flows();
        }
    }
    

    if args.stats{
        for agent in agent_list.clone() {
            agent.get_stats();
        }
    }


    
    exit(0);
    futures::future::join_all(join_handlers).await;
    
    Ok(())
}