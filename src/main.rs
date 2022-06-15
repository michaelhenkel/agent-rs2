mod agent;
mod flow;
mod config;
mod control;

use core::num;
use std::{thread::sleep, fmt::format};
use std::time::Duration;

use agent::agent::Agent;
use config::config::{Config, Vmi};
use control::control::Control;
use ipnet;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{


    let mut join_handlers = Vec::new();

    let config = Config::new();
    let control = Control::new();

    let control_handle = tokio::task::spawn(control.clone().run());
    join_handlers.push(control_handle);

    let num_agents = 50;
    let num_vmis = 50;
    let num_partitions = 8;

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

 
     
    for agent in 0..num_agents{
        let agent_name = format!("agent{}", agent);
        for vmi in 0..num_vmis {
            let ip = format!{"{}.1.1.{}/32", agent+1, vmi+1};
            config.clone().add_vmi(Vmi { 
                name: "vmi".to_string(),
                ip: ip.parse().unwrap(),
                agent: agent_name.clone(),
             });
        }
    }

    sleep(Duration::from_secs(10));

    for agent in agent_list.clone() {
        agent.get_stats();
    }

    /*
    for agent in agent_list {
        agent.get_flows();
    }
    */

    futures::future::join_all(join_handlers).await;
    
    Ok(())
}