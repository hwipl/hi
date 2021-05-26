use crate::config;
use crate::daemon_message::Message;
use crate::swarm;
use crate::unix_socket;
use async_std::task;

async fn handle_client(mut client: unix_socket::UnixClient) {
    while let Ok(msg) = client.receive_message().await {
        println!("received message from client: {:?}", msg);
        match msg {
            // handle OK message
            Message::Ok => {
                if let Err(e) = client.send_message(Message::Ok).await {
                    eprintln!("handle client error: {}", e);
                    return;
                }
            }

            // handle error message
            Message::Error { message } => {
                println!("received error message from client: {:?}", message);
            }

            // handle connect address request
            Message::ConnectAddress { .. } => {
                let message = String::from("Not yet implemented");
                let error = Message::Error { message };
                if let Err(e) = client.send_message(error).await {
                    eprintln!("handle client error: {}", e);
                    return;
                }
            }

            // handle get name request
            Message::GetName { .. } => {
                let message = String::from("Not yet implemented");
                let error = Message::Error { message };
                if let Err(e) = client.send_message(error).await {
                    eprintln!("handle client error: {}", e);
                    return;
                }
            }
        }
    }
}

async fn run_server(config: config::Config, server: unix_socket::UnixServer) {
    // handle incoming connections
    task::spawn(async move {
        while let Some(client) = server.next().await {
            task::spawn(handle_client(client));
        }
    });

    // run swarm
    if let Err(e) = swarm::run(config.connect) {
        eprintln!("swarm error: {}", e);
    }
    println!("swarm stopped");
}

pub fn run(config: config::Config) {
    // run unix socket server
    task::block_on(async {
        match unix_socket::UnixServer::listen().await {
            Ok(server) => run_server(config, server).await,
            Err(e) => eprintln!("unix socket server error: {}", e),
        };
        println!("unix socket server stopped");
    });
}
