extern crate futures;
extern crate websocket;
extern crate websocket_transport;

use futures::*;
use websocket::{OwnedMessage, WebSocketError};
use websocket::async::{Client, Core, Server, TcpStream};
use websocket::client::builder::ClientBuilder;
use websocket::server::NoTlsAcceptor;
use websocket_transport::{TransportOwnedMessage, WebSocketTransport};

const PROTOCOL: &str = "protocol";

fn get_address(port: u16) -> String {
    format!("127.0.0.1:{}", port)
}

fn get_ws_url(port: u16) -> String {
    format!("ws://127.0.0.1:{}", port)
}

fn serve(
    server: Server<NoTlsAcceptor>,
    serve: fn(Client<TcpStream>) -> Box<Future<Item=(), Error=()>>)
    -> Box<Future<Item=(), Error=()>>
{
    Box::new(
        server.incoming()
            .for_each(move |(upgrade, _addr)| {
                upgrade.use_protocol(PROTOCOL).accept()
                    .then(move |result| {
                        let (duplex, _) = result.unwrap();
                        serve(duplex)
                    })
                    .then(|_| Ok(()))
            })
            .then(|_| Ok(()))
    )
}

fn serve_echo(duplex: Client<TcpStream>) -> Box<Future<Item=(), Error=()>> {
    let (sink, stream) = duplex.split();
    Box::new(stream.forward(sink).then(|_| Ok(())))
}

fn serve_with_pings(duplex: Client<TcpStream>) -> Box<Future<Item=(), Error=()>> {
    let (sink, stream) = duplex.split();
    let stream_with_pings = stream
        .map(|msg| stream::iter_ok::<_, WebSocketError>(
            vec![OwnedMessage::Ping(vec![]), msg]))
        .flatten();
    Box::new(sink.send_all(stream_with_pings).then(|_| Ok(())))
}

#[test]
fn transfer_data() {
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let server = Server::bind(get_address(10000), &handle).unwrap();
    let client = ClientBuilder::new(get_ws_url(10000).as_str()).unwrap();

    let client_transport = client
        .add_protocol(PROTOCOL)
        .async_connect_insecure(&handle)
        .then(|result| {
            let (duplex, _headers) = result.unwrap();
            Ok(WebSocketTransport::from(duplex))
        });

    let data: TransportOwnedMessage = vec![1, 2, 3].into();
    let test = client_transport
        .and_then(|t| t.send(data.clone()))
        .and_then(|t| t.into_future().map_err(|(err, _t)| err))
        .and_then(|(next, _t)| {
            assert_eq!(next.unwrap(), data);
            Ok(())
        });

    handle.spawn(serve(server, serve_echo));
    core.run(test).unwrap();
}

#[test]
fn transfer_with_pings() {
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let server = Server::bind(get_address(10001), &handle).unwrap();
    let client = ClientBuilder::new(get_ws_url(10001).as_str()).unwrap();

    let client_transport = client
        .add_protocol(PROTOCOL)
        .async_connect_insecure(&handle)
        .then(|result| {
            let (duplex, _headers) = result.unwrap();
            Ok(WebSocketTransport::from(duplex))
        });

    let data1: TransportOwnedMessage = vec![1, 2, 3].into();
    let data2: TransportOwnedMessage = vec![4, 5, 6].into();
    let test = client_transport
        .and_then(|t| t.send(data1.clone()))
        .and_then(|t| t.send(data2.clone()))
        .and_then(|t| t.into_future().map_err(|(err, _t)| err))
        .and_then(|(next, t)| {
            assert_eq!(next.unwrap(), data1);
            t.into_future().map_err(|(err, _t)| err)
        })
        .and_then(|(next, _t)| {
            assert_eq!(next.unwrap(), data2);
            Ok(())
        });

    handle.spawn(serve(server, serve_with_pings));
    core.run(test).unwrap();
}
