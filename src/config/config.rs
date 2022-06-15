use std::sync::Arc;
use std::collections::HashMap;
use std::sync::RwLock;
use crate::agent::agent::Add;

#[derive(Clone)]
pub struct Config{
    pub agent_list:  Arc<RwLock<HashMap<String,crossbeam_channel::Sender<Add>>>>,
}

impl Config {
    pub fn new() -> Self {
        Self { 
            agent_list:  Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub fn add_agent(self, name: String, sender: crossbeam_channel::Sender<Add>) {
        //println!("adding agent {} to config", name);
        let mut sender_map = self.agent_list.write().unwrap();
        sender_map.insert(name, sender);
    }

    pub fn add_vmi(self,vmi: Vmi) {
        let mut sender_map = self.agent_list.write().unwrap();
        //println!("registered agents {}", sender_map.len());
        let agent_sender = sender_map.get_mut(&vmi.clone().agent);
        /* 
        for (a,b) in sender_map.clone() {
            println!("{}", a);
            b.send(Add::Vmi(vmi.clone()));
        }
        */
        
        match agent_sender{
            Some(sender) => {
                sender.send(Add::Vmi(vmi));
            },
            None => {
                println!("no sender found");
            },
        }
        
       
    }
}


#[derive(PartialEq,Hash,Eq,Clone)]
pub struct Vmi {
    pub name: String,
    pub ip: ipnet::Ipv4Net,
    pub agent: String,
}