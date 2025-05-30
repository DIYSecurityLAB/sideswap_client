use std::net::SocketAddr;

use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::{error::Error, worker::Command};

use super::api;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientId(u64);

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    listen_on: SocketAddr,
}

struct Data {
    command_sender: UnboundedSender<Command>,
    ws_stream: WebSocketStream<TcpStream>,
}

async fn send_msg(data: &mut Data, msg: Message) {
    let res = data.ws_stream.send(msg).await;
    if let Err(err) = res {
        log::debug!("ws message sending failed: {err}");
    }
}

async fn send_from(data: &mut Data, from: api::From) {
    let msg = serde_json::to_string(&from).expect("must not fail");
    send_msg(data, Message::text(msg)).await;
}

async fn send_notif(data: &mut Data, notif: api::Notif) {
    send_from(data, api::From::Notif { notif }).await;
}

async fn process_ws_req(data: &mut Data, req: api::Req) -> Result<api::Resp, Error> {
    let (res_sender, res_receiver) = oneshot::channel();
    data.command_sender.send(Command::Request {
        req,
        res_sender: res_sender.into(),
    })?;
    let resp = res_receiver.await??;
    Ok(resp)
}

async fn process_to_msg(data: &mut Data, to: api::To) {
    match to {
        api::To::Req { id, req } => {
            let res = process_ws_req(data, req).await;
            match res {
                Ok(resp) => send_from(data, api::From::Resp { id, resp }).await,
                Err(err) => {
                    send_from(
                        data,
                        api::From::Error {
                            id,
                            err: err.into(),
                        },
                    )
                    .await
                }
            }
        }
    }
}

fn get_req_id(msg: &str) -> api::ReqId {
    #[derive(serde::Deserialize)]
    pub enum ToIdOnly {
        Req { id: api::ReqId },
    }
    serde_json::from_str::<ToIdOnly>(msg)
        .map(|ToIdOnly::Req { id }| id)
        .unwrap_or_default()
}

async fn process_ws_msg(data: &mut Data, msg: Message) {
    match msg {
        Message::Text(msg) => {
            let res = serde_json::from_str::<api::To>(&msg);
            match res {
                Ok(to) => {
                    process_to_msg(data, to).await;
                }
                Err(err) => {
                    send_from(
                        data,
                        api::From::Error {
                            id: get_req_id(&msg),
                            err: api::Error {
                                code: api::ErrorCode::InvalidRequest,
                                text: format!("invalid JSON: {err}"),
                                details: None,
                            },
                        },
                    )
                    .await;
                }
            }
        }
        Message::Binary(_) => {
            log::debug!("binary message ignored");
        }
        Message::Ping(_) => {}
        Message::Pong(_) => {}
        Message::Close(msg) => {
            log::debug!("close message received: {msg:?}");
        }
        Message::Frame(_) => {
            log::debug!("frame message ignored");
        }
    }
}

async fn client_loop(
    data: &mut Data,
    mut notif_receiver: UnboundedReceiver<api::Notif>,
) -> Result<(), anyhow::Error> {
    loop {
        tokio::select! {
            msg = data.ws_stream.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        process_ws_msg(data, msg).await;
                    },
                    Some(Err(err)) => {
                        log::debug!("ws connection closed: {err}");
                        break;
                    },
                    None => {
                        log::debug!("ws connection closed");
                        break;
                    },
                }
            },

            notif = notif_receiver.recv() => {
                match notif {
                    Some(notif) => {
                        send_notif(data, notif).await;
                    },
                    None => {
                        log::debug!("disconnect client");
                        break;
                    },
                }
            },
        }
    }

    Ok(())
}

async fn client_run(
    command_sender: UnboundedSender<Command>,
    client_id: ClientId,
    tcp_stream: TcpStream,
) {
    let ws_stream = match tokio_tungstenite::accept_async(tcp_stream).await {
        Ok(ws_stream) => ws_stream,
        Err(err) => {
            log::error!("ws handshake failed: {err}");
            return;
        }
    };

    let mut data = Data {
        command_sender,
        ws_stream,
    };

    let (event_sender, event_receiver) = unbounded_channel();

    let _ = data.command_sender.send(Command::ClientConnected {
        client_id,
        notif_sender: event_sender.into(),
    });

    let result = client_loop(&mut data, event_receiver).await;

    if let Err(err) = result {
        log::debug!("ws connection stopped: {err}");
    }

    let _ = data
        .command_sender
        .send(Command::ClientDisconnected { client_id });
}

async fn run(config: Config, command_sender: UnboundedSender<Command>) {
    log::info!("start WS server on {}...", config.listen_on);
    let listener = TcpListener::bind(&config.listen_on)
        .await
        .expect("port must be open");
    let mut last_id = 0;

    loop {
        let (tcp_stream, _socket) = listener.accept().await.expect("should not fail");

        last_id += 1;
        let client_id = ClientId(last_id);

        tokio::spawn(client_run(command_sender.clone(), client_id, tcp_stream));
    }
}

pub fn start(config: Config, command_sender: UnboundedSender<Command>) {
    tokio::task::spawn(run(config, command_sender));
}
