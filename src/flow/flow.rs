use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::RwLock;
use ipnet;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use crate::agent::agent::{Action,Add,Get};


#[derive(Clone)]
pub struct FlowTable{
    name: String,
    flow_partition_sender_list: Arc<RwLock<HashMap<i16, crossbeam_channel::Sender<Action>>>>,
    flow_partition_list: Vec<FlowTablePartition>,
    partition_ready_sender: crossbeam_channel::Sender<bool>,
}

impl FlowTable {
    pub fn new(name: String, partition_ready_sender: crossbeam_channel::Sender<bool>) -> Self {
        Self{
            name,
            flow_partition_sender_list: Arc::new(RwLock::new(HashMap::new())),
            flow_partition_list: Vec::new(),
            partition_ready_sender,
        }
    }

    pub fn get_partition_list(self) -> Arc<RwLock<HashMap<i16, crossbeam_channel::Sender<Action>>>>{
        self.flow_partition_sender_list
    }

    pub fn get_flows(self) {
        let mut flow_partition_sender_list = self.flow_partition_sender_list.write().unwrap();
        for (part, flow_partition_sender) in flow_partition_sender_list.clone(){
            let (action_sender,action_receiver): (crossbeam_channel::Sender<Vec<Flow>>, crossbeam_channel::Receiver<Vec<Flow>>) = crossbeam_channel::unbounded();
            flow_partition_sender.send(Action::Get(Get::Flows((action_sender))));
            let flows = action_receiver.recv().unwrap();
            for flow in flows {
                println!("{} src {}/{} dst {}/{} nh {}", self.name, flow.flow_key.src_prefix, flow.flow_key.src_prefix_len, flow.flow_key.dst_prefix, flow.flow_key.dst_prefix_len, flow.nh);
            }
        }
    } 

    pub fn get_stats(self) {
        let mut flow_partition_sender_list = self.flow_partition_sender_list.write().unwrap();
        let mut total_duration = 0;
        let mut total_flows = 0;
        let mut partition = 0;
        for (part, flow_partition_sender) in flow_partition_sender_list.clone(){
            let (action_sender,action_receiver): (crossbeam_channel::Sender<FlowTableStats>, crossbeam_channel::Receiver<FlowTableStats>) = crossbeam_channel::unbounded();
            flow_partition_sender.send(Action::Get(Get::FlowTableStats((action_sender))));
            let flow_table_stats = action_receiver.recv().unwrap();
            total_duration = total_duration + flow_table_stats.duration;
            total_flows = total_flows + flow_table_stats.counter;
            partition = partition + 1;
            println!("{} partition {} flows {} duration {}", self.name, flow_table_stats.partition, flow_table_stats.counter, flow_table_stats.duration);
        }
        let dur = Duration::from_nanos(total_duration.try_into().unwrap());
        let partitions = flow_partition_sender_list.len();
        println!("----> {} flows in {} micro sec",total_flows, dur.as_micros()/partition);
    }
    

    pub async fn run(self, part: i16) -> Result<(), Box<dyn std::error::Error + Send>>{
        let (partition_count_sender,partition_count_receiver): (crossbeam_channel::Sender<i16>, crossbeam_channel::Receiver<i16>) = crossbeam_channel::unbounded();
        let partition_count_sender_arc = Arc::new(partition_count_sender);
        let mut flow_partition_list = self.flow_partition_list.clone();
        let mut res_list = Vec::new();
        for n in 0..part{
            let pcs = partition_count_sender_arc.clone();
            let (flow_partition_sender,flow_partition_receiver): (crossbeam_channel::Sender<Action>, crossbeam_channel::Receiver<Action>) = crossbeam_channel::unbounded();
            let mut flow_partition_sender_list = self.flow_partition_sender_list.write().unwrap();
            flow_partition_sender_list.insert(n, flow_partition_sender);
            let flow_table_partition = FlowTablePartition::new(flow_partition_receiver, n, self.name.clone());
            flow_partition_list.push(flow_table_partition.clone());
            let res = tokio::task::spawn_blocking(move||{
                flow_table_partition.run(pcs);
            }
            );
            res_list.push(res);
            

        }
        let mut running_partition_counter: i16 = 0;
        loop {
            let counter = partition_count_receiver.recv().unwrap();
            running_partition_counter = running_partition_counter + counter;
            if running_partition_counter == part {
                self.partition_ready_sender.send(true);
                break
            }
        }
        futures::future::join_all(res_list).await;
        Ok(())
    }


    pub fn send_flow(self, flow: Flow, part: i16) {
        let flow_partition_sender_list = self.flow_partition_sender_list.write().unwrap();
        let p = flow_partition_sender_list.get(&part).unwrap();
        p.send(Action::Add(Add::Flow(flow)));
    }
}

#[derive(Clone)]
pub struct FlowTablePartition{
    flow_partition_receiver: crossbeam_channel::Receiver<Action>,
    partition: i16,
    name: String,
    flow_table: Arc<RwLock<HashMap<FlowKey, String>>>,
    flow_counter: Arc<RwLock<u32>>,
    timer: Arc<RwLock<Instant>>,
    duration: Arc<RwLock<u128>>,
}

impl FlowTablePartition {
    pub fn new(flow_partition_receiver: crossbeam_channel::Receiver<Action>, partition: i16, name: String) -> Self {
        Self{
            flow_partition_receiver,
            partition,
            name,
            flow_table: Arc::new(RwLock::new(HashMap::new())),
            flow_counter: Arc::new(RwLock::new(0)),
            timer: Arc::new(RwLock::new(Instant::now())),
            duration: Arc::new(RwLock::new(0)),
        }
    }
    pub fn run(self, partition_count_sender: Arc<crossbeam_channel::Sender<i16>>){
        //println!("{} running partition {}", self.name, self.partition);
        partition_count_sender.send(1);
        loop {
            let action = self.flow_partition_receiver.recv().unwrap();
            match action {
                Action::Add(Add::Flow(flow)) => {
                    let mut flow_table = self.flow_table.write().unwrap();
                    flow_table.insert(flow.flow_key, flow.nh.clone());
                    let mut counter = self.flow_counter.write().unwrap();
                    let mut timer = self.timer.write().unwrap();
                    let mut duration = self.duration.write().unwrap();
                    let now = Instant::now();
                    if *counter == 0 {
                        *timer = now;
                    }
                    let new_duration = now.duration_since(*timer);
                        
                    *duration = new_duration.as_nanos();
                    *counter = *counter + 1;
                },
                Action::Get(Get::FlowTableStats(flow_table_stats_sender)) => {
                    let duration = self.duration.write().unwrap();
                    let counter = self.flow_counter.write().unwrap();
                    flow_table_stats_sender.send(FlowTableStats{
                        duration: *duration,
                        counter: *counter,
                        partition: self.partition,
                    });
                },
                Action::Get(Get::Flows(flow_table_stats_sender)) => {
                    let mut flow_list = Vec::new();
                    let flow_table = self.flow_table.write().unwrap();
                    for (flow_key, nh) in flow_table.clone(){
                        flow_list.push(Flow{
                            flow_key: flow_key,
                            nh: nh,
                        });
                    }
                    flow_table_stats_sender.send(flow_list);
                },
                Action::Get(Get::MatchFlow(flow_k, flow_table_stats_sender)) => {
                    let flow_table = self.flow_table.write().unwrap();
                    let nh_res = flow_table.get_key_value(&flow_k);
                    match nh_res {
                        Some((flow_key, nh)) => {
                            flow_table_stats_sender.send(Flow { 
                                flow_key: flow_key.clone(),
                                nh: nh.clone(),
                             });
                        },
                        None => { println!("didn't find flow"); },
                    }
                },
                _ => {},
            };

        }
    }
     
    pub fn get_stats(self) -> Result<(u128, u32, i16), Box<dyn std::error::Error + Send> >{
        let mut duration = self.duration.write().unwrap();
        let mut counter = self.flow_counter.write().unwrap();
        Ok((*duration, *counter, self.partition))
    }


    
}

async fn join_parallel<T: Send + 'static>(
    futs: impl IntoIterator<Item = impl futures::future::Future<Output = T> + Send + 'static>,
) -> Vec<T> {
    let tasks: Vec<_> = futs.into_iter().map(tokio::spawn).collect();
    // unwrap the Result because it is introduced by tokio::spawn()
    // and isn't something our caller can handle
    futures::future::join_all(tasks)
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect()
}

#[derive(PartialEq,Hash,Eq,Clone)]
pub struct FlowKey{
    pub src_prefix: Ipv4Addr,
    pub src_prefix_len: u8,
    pub dst_prefix: Ipv4Addr,
    pub dst_prefix_len: u8,
}

pub struct Flow{
    pub flow_key: FlowKey,
    pub nh: String,
}

pub struct FlowTableStats {
    duration: u128,
    counter: u32,
    partition: i16,
}

