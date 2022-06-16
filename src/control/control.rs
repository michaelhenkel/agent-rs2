use std::sync::Arc;
use std::collections::HashMap;
use std::sync::RwLock;
use crate::agent::agent::{Add, Action};

#[derive(Clone)]
pub struct Control{
    pub agent_add_route_list:  Arc<RwLock<HashMap<String,crossbeam_channel::Sender<Action>>>>,
    route_receiver: crossbeam_channel::Receiver<Add>,
    pub route_sender: crossbeam_channel::Sender<Add>,
    pub route_table:  Arc<RwLock<HashMap<ipnet::Ipv4Net,String>>>,
}

impl Control {
    pub fn new() -> Self {
        let (route_sender,route_receiver): (crossbeam_channel::Sender<Add>, crossbeam_channel::Receiver<Add>) = crossbeam_channel::unbounded();

        Self { 
            agent_add_route_list: Arc::new(RwLock::new(HashMap::new())),
            route_receiver,
            route_sender,
            route_table: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub fn add_agent(self, name: String, add_route_sender: crossbeam_channel::Sender<Action>) {
        //println!("adding agent {} to config", name);
        let mut sender_map = self.agent_add_route_list.write().unwrap();
        sender_map.insert(name, add_route_sender.clone());
        let mut route_table = self.route_table.write().unwrap();
        for (route, nh) in route_table.clone() {
            add_route_sender.send(Action::Add(Add::Route(Route{
                dst: route,
                nh: nh,
            })));
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send>>{
        let res = tokio::task::spawn_blocking(move||{
            loop{
                let add = self.route_receiver.recv();
                match add.unwrap() {
                    Add::Route(route) => {
                        //println!("control got route {} {}", route.dst, route.nh);
                        let mut route_table = self.route_table.write().unwrap();
                        route_table.insert(route.clone().dst, route.clone().nh);
                        let sender_map = self.agent_add_route_list.write().unwrap();
                        for (agent, sender) in sender_map.clone(){
                            //println!("sending route to {}", agent);
                            sender.send(Action::Add(Add::Route(route.clone())));
                        }
                    },
                    _ => {},
                }
            }
        }
        );
        res.await;
        Ok(())
    }
    
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Route{
    pub dst: ipnet::Ipv4Net,
    pub nh: String,
}