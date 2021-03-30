#![allow(warnings, unused, dead_code)] // Still working on this module :)

use core::num::NonZeroU16;
use futures::{Stream, StreamExt};
use protocol::packet::PublishPacket;
use reqwest::Client;
use std::{ops::Sub, time::Duration};
use tokio::sync::broadcast;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use mqtt::{
    self as protocol,
    control::variable_header::{ConnectReturnCode, PacketIdentifier},
    packet::suback::SubscribeReturnCode,
    packet::VariablePacket,
    QualityOfService, TopicFilter,
};

#[derive(Clone, Debug)]
struct SubMessage {
    pub topic_name: String,
    pub payload: Vec<u8>,
    pub qos: QualityOfService,
}

#[derive(Clone, Debug)]
pub struct Publish {
    topic: String,
    payload: Vec<u8>,
    qos: QualityOfService,
}

//stubs
type ConnectOptions = ();

#[derive(Clone)]
pub enum KeepAlive {
    /// Keep alive ping packets are disabled.
    Disabled,

    /// Send a keep alive ping packet every `secs` seconds.
    Enabled {
        /// The number of seconds between packets.
        secs: u16,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    // Use String instead of runloop::HandleError to not leak private type
    #[error("Failed to send mqtt write request to the run loop")]
    HandleError(String),

    #[error("A connection already exists")]
    AlreadyConnected,

    #[error("Operation cannot be done because no connection exists")]
    NotConnected,
}

impl std::convert::From<runloop::HandleError> for ClientError {
    fn from(error: runloop::HandleError) -> Self {
        ClientError::HandleError(format!("{}", error))
    }
}

#[derive(Clone)]
pub(crate) struct ClientOptions {
    // See ClientBuilder methods for per-field documentation.
    pub(super) host: String,
    pub(crate) port: u16,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) keep_alive: KeepAlive,
    //pub(crate) runtime: TokioRuntime,
    pub(crate) client_id: Option<String>,
    //pub(crate) packet_buffer_len: usize,
    //pub(crate) max_packet_len: usize,
    pub(crate) operation_timeout: Duration,
    pub(crate) tls_client_config: rustls::ClientConfig,
}

impl ClientOptions {
    fn default<S: Into<String>>(host: S, port: u16) -> ClientOptions {
        let mut tls_client_config = rustls::ClientConfig::new();

        // Using platform certificates by default
        tls_client_config.root_store =
            rustls_native_certs::load_native_certs().expect("could not load platform certs");

        ClientOptions {
            host: host.into(),
            port,
            username: None,
            password: None,
            keep_alive: KeepAlive::Disabled,
            client_id: None,
            operation_timeout: Duration::from_secs(2),
            tls_client_config,
        }
    }
}

// TODO add missing options
pub struct Builder {
    pub(super) host: String,
    pub(crate) port: u16,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) keep_alive: KeepAlive,
    pub(crate) client_id: Option<String>,
}
impl Builder {
    fn new(host: &str, port: u16) -> Builder {
        Builder {
            host: host.to_string(),
            port,
            username: None,
            password: None,
            keep_alive: KeepAlive::Disabled,
            client_id: None,
        }
    }

    pub fn build(self) -> MqttClient {
        let mut opts = ClientOptions::default(self.host, self.port);
        opts.username = self.username;
        opts.password = self.password;
        opts.keep_alive = self.keep_alive;
        opts.client_id = self.client_id;

        MqttClient::create(opts)
    }

    pub fn set_user_name<S: Into<String>>(mut self, user_name: S) -> Self {
        self.username = Some(user_name.into());
        self
    }

    pub fn set_password<S: Into<String>>(mut self, password: S) -> Self {
        self.password = Some(password.into());
        self
    }

    pub fn set_keep_alive(mut self, keepalive: KeepAlive) -> Self {
        self.keep_alive = keepalive;
        self
    }

    pub fn set_client_id<S: Into<String>>(mut self, client_id: S) -> Self {
        self.client_id = Some(client_id.into());
        self
    }
}

pub struct MqttClient {
    /// A queue acting as a single producer - multi consumer between network runloop
    /// and our users. We only store the sender because Receiver are created as needed
    /// (and only a Sender is needed to keep the channel alive).
    // TODO Not sure if PublishPacket should be exposed outside of the run loop.
    messages: broadcast::Sender<PublishPacket>,
    run_loop: Option<runloop::Handle>,
    /// Configuration for the runloop. We keep a copy in case we have to recreate a new
    /// runloop. Which is what happens when a user call disconnect and then connect.
    options: ClientOptions,
}

impl MqttClient {
    // default options
    pub fn new(host: &str, port: u16) -> MqttClient {
        let options = ClientOptions::default(host, port);

        MqttClient::create(options)
    }

    // TODO Hide the options behind a builder structure, but the idea is to have a customizable client
    // Required parameters are still asked to create the builder (host/port is the minimum I think)
    pub fn builder(host: &str, port: u16) -> Builder {
        Builder::new(host, port)
    }

    fn create(options: ClientOptions) -> MqttClient {
        let (messages, _) = broadcast::channel(16);

        // TODO Do we need to keep the sender in the client ?
        // I'd say we should give it to the runloop and let it do the bridges as required
        MqttClient {
            messages,
            run_loop: None,
            options,
        }
    }

    /// Receive the messages from the various subscription made by this client.
    ///
    /// Note that messages received prior creation of this stream won't be visible. It is thus
    /// recommend to create one Stream before creating the actual subscriptions.
    // TODO Probably implement our own enum error here. We may want to include more
    // cases (disconnected for example. not sure tbh).
    fn subscriptions(&self) -> impl Stream<Item = Result<SubMessage, BroadcastStreamRecvError>> {
        let receiver = self.messages.subscribe();

        let a = tokio_stream::wrappers::BroadcastStream::new(self.messages.subscribe());

        let b = a.map(|r| {
            r.map(|publ| {
                let qos = match publ.qos() {
                    protocol::packet::QoSWithPacketIdentifier::Level0 => QualityOfService::Level0,
                    protocol::packet::QoSWithPacketIdentifier::Level1(_) => {
                        QualityOfService::Level1
                    }
                    protocol::packet::QoSWithPacketIdentifier::Level2(_) => {
                        QualityOfService::Level2
                    }
                };

                SubMessage {
                    topic_name: publ.topic_name().to_string(),
                    payload: publ.payload().to_vec(),
                    qos,
                }
            })
        });

        b
    }

    pub async fn connect(&mut self) -> Result<(), ClientError> {
        if self.run_loop.is_some() {
            return Err(ClientError::NotConnected);
        }

        // TODO Handle the error when ::create returns them
        let run_loop = runloop::create(self.options.clone(), self.messages.clone()).await;
        self.run_loop = Some(run_loop);

        Ok(())
    }

    pub async fn disconnect(&mut self) {
        // TODO disconnecting means shutting down the run_loop and removing it from self
        unimplemented!()
    }

    pub async fn subscribe<I: Iterator<Item = (TopicFilter, QualityOfService)>>(
        &self,
        topics: I,
    ) -> Vec<SubscribeReturnCode> {
        unimplemented!()
    }

    pub async fn publish(&self, message: Publish) -> Result<(), ClientError> {
        match &self.run_loop {
            Some(h) => Ok(h.publish(message).await?),
            None => Err(ClientError::NotConnected),
        }
    }
}

mod runloop {
    use super::{ClientOptions, KeepAlive, SubMessage};
    use bytes::BytesMut;
    use futures::{Sink, SinkExt, Stream};
    use log::{debug, error, trace, warn};
    use mqtt::{
        self as protocol,
        control::variable_header::{protocol_level, ConnectReturnCode, PacketIdentifier},
        control::ProtocolLevel,
        packet::{
            publish::QoSWithPacketIdentifier, ConnackPacket, ConnectPacket, MqttCodec,
            PubackPacket, PublishPacket, VariablePacket, VariablePacketError, SubscribePacket, SubackPacket
        },
        QualityOfService, TopicName,
    };
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::net::TcpStream;
    use tokio::{
        net::tcp,
        sync::{broadcast, mpsc, oneshot, RwLock},
    };
    use tokio_rustls::{rustls::ClientConfig, webpki::DNSNameRef, TlsConnector};
    use tokio_stream::StreamExt;
    use tokio_util::codec::{Decoder, Encoder, Framed};

    #[derive(Debug, thiserror::Error)]
    pub(crate) enum HandleError {
        #[error("Failed to send mqtt write request to the run loop: {0}")]
        WriterRequest(#[from] mpsc::error::SendError<WriterRequest>),

        #[error("Failed to receive response from the run loop: {0}")]
        WriterResponse(#[from] oneshot::error::RecvError),

        #[error("Invalid topic name passed: {0}")]
        InvalidTopicName(#[from] protocol::topic_name::TopicNameError),
    }

    pub(super) struct Handle {
        /// Each task is responsible to observe this stream and shut themselves
        shutdown: broadcast::Sender<()>,

        /// Send packet to be written
        write_tx: mpsc::Sender<WriterRequest>,

        /// Queue to create new receiver of publish messages received from the server
        messages: broadcast::Sender<PublishPacket>,
    }

    impl Handle {
        pub(crate) async fn publish(&self, publish: super::Publish) -> Result<(), HandleError> {
            let topic_name = TopicName::new(publish.topic)?;
            let qos = publish.qos;
            let command = WriterCommand::Publish(topic_name, qos, publish.payload);
            let (sender, receiver) = oneshot::channel();

            let req = WriterRequest {
                result: Some(sender),
                command,
            };
            self.write_tx.send(req).await?;

            let res = receiver.await?;
            // TODO Handle this case when I know what error to look for
            let a: Result<(), HandleError> = match res {
                Ok(()) => Ok(()),
                Err(()) => Ok(()),
            };

            Ok(a?)
        }
    }

    struct ConnectionState {
        /// Tasks that await a confirmation from the server. It maps a pid to the callback.
        ///
        /// _Note: we use u16 as a key because PacketIdentifier doesn't implemente Ord._
        outstanding: BTreeMap<u16, oneshot::Sender<Result<(), ()>>>,

        /// Keep record of the last used packet id. We will always increase it and wrap
        /// when reached u16 limit. We assume by that time, we will have freed the first
        /// packet ids.
        last_pid: u16,
    }

    impl ConnectionState {
        fn next_pid(&mut self) -> u16 {
            // When we reach 65535, we go back to 0 and hope we don't have 60k+ in flight messages :)
            let (pid, _) = self.last_pid.overflowing_add(1);
            self.last_pid = pid;
            pid
        }
    }

    #[derive(Debug)]
    pub(crate) struct WriterRequest {
        result: Option<oneshot::Sender<Result<(), ()>>>,
        command: WriterCommand,
    }

    #[derive(Debug)]
    pub(crate) enum WriterCommand {
        Publish(TopicName, QualityOfService, Vec<u8>),
        Subscribe(),
        Unsubscribe(),
        Disconnect(),
    }

    // TODO Remove TCP once I got the basic working. That will simplify the type signature quite a bit (use impl Trait and no boxing)
    async fn create_stream(
        opts: &ClientOptions,
    ) -> Result<
        (
            impl Stream<Item = Result<VariablePacket, VariablePacketError>>,
            impl Sink<VariablePacket, Error = std::io::Error>,
        ),
        Box<dyn std::error::Error>,
    > {
        use futures::StreamExt;
        let c = Arc::new(opts.tls_client_config.clone());
        let connector = TlsConnector::from(c);
        let domain = DNSNameRef::try_from_ascii_str(&*opts.host)?;
        let tcp = TcpStream::connect((opts.host.as_str(), opts.port)).await?;
        let conn = connector.connect(domain, tcp).await?;

        let f = Framed::new(conn, MqttCodec::new());

        let (write, read) = f.split();

        Ok((read, write))
    }

    pub(super) async fn create(
        opts: ClientOptions,
        enqueue_pub: broadcast::Sender<PublishPacket>,
    ) -> Handle {
        // Create the TLS/TCP stream

        let (mut packet_stream, mut packet_sink) =
            create_stream(&opts).await.expect("Needs to use ? notation");

        // Initialize the connection. If this fails, there is no need to create the remaining machinery.
        debug!("MQTT handshake");

        // TODO Generate a uuid instead of hardcoded value
        let client_id = opts
            .client_id
            .as_ref()
            .map(|c| c.clone())
            .unwrap_or("fc606d39-ac57-40e0-8b41-891ee3088e46".to_string());

        let mut pkt = ConnectPacket::with_level("MQTT", client_id, protocol_level::SPEC_3_1_1)
            .expect("protocol level should be valid");
        let sec = match opts.keep_alive {
            KeepAlive::Disabled => 0,
            KeepAlive::Enabled { secs } => secs,
        };
        pkt.set_keep_alive(sec);
        pkt.set_clean_session(true);
        pkt.set_user_name(opts.username.clone());
        pkt.set_password(opts.password);
        packet_sink.send(pkt.into()).await;

        match packet_stream.next().await {
            Some(Ok(VariablePacket::ConnackPacket(pkt))) => match pkt.connect_return_code() {
                ConnectReturnCode::ConnectionAccepted => {
                    debug!("Connected")
                }
                ret_code => {
                    error!("Can't connect, received {:?}", ret_code);
                    todo!("return an error")
                }
            },
            Some(Ok(packet)) => {
                error!("Expected connack but got packet: {:?}", packet);
                todo!("return an error")
            }
            Some(Err(error)) => {
                error!("Expected connack but got error: {:?}", error);
                todo!("return an error")
            }
            None => {
                error!("Connection closed while waiting for connack");
                todo!("return an error")
            }
        }

        // We are now connected, let's finish setting up the run loop

        // Channels between tasks
        let (shutdown, _) = broadcast::channel(1);

        let (timer_tx, timer_rx) = mpsc::channel(10);
        let (write_tx, write_rx) = mpsc::channel(10);
        let write_shutdown = shutdown.subscribe();
        let read_shutdown = shutdown.subscribe();
        let keep_alive_shutdown = shutdown.subscribe();

        // connection state

        let state = Arc::new(RwLock::new(ConnectionState {
            outstanding: BTreeMap::new(),
            last_pid: 0,
        }));

        // spawn tasks
        let w = tokio::spawn({
            let state = state.clone();
            async move { network_write(state, write_rx, packet_sink, write_shutdown).await }
        });
        let r = tokio::spawn({
            let state = state.clone();
            let enqueue_pub = enqueue_pub.clone();
            async move { network_read(state, packet_stream, timer_tx, enqueue_pub, read_shutdown).await }
        });
        let t = tokio::spawn({
            let state = state.clone();
            async move { keep_alive(state, timer_rx, keep_alive_shutdown).await }
        });

        Handle {
            shutdown,
            write_tx,
            messages: enqueue_pub,
        }
    }

    // handle packet received from the server
    async fn network_read<S: Stream<Item = Result<VariablePacket, VariablePacketError>> + Unpin>(
        state: Arc<RwLock<ConnectionState>>,
        mut packet_stream: S,
        timer: mpsc::Sender<()>,
        enqueue_pub: broadcast::Sender<PublishPacket>,
        shutdown: broadcast::Receiver<()>,
    ) {
        loop {
            match packet_stream.next().await {
                None => {
                    debug!("End of Stream. Connection closed.");
                    break;
                }
                Some(Err(error)) => {
                    warn!("Error while decoding a packet from the server: {:?}", error)
                }
                Some(Ok(VariablePacket::PingrespPacket(..))) => {
                    debug!("Receiving PINGRESP from broker ..");
                    timer
                        .send(())
                        .await
                        .expect("PINGRESP notification to timer failed")
                }
                Some(Ok(VariablePacket::PublishPacket(publ))) => {
                    // TODO Might want to change the serialization here, as the default will print the payload's bytes
                    debug!("PUBLISH: {:?}", publ);

                    // Handle this Result in a different way. Here Ok means at least one
                    // receiver got `p`. Err means no receiver were present.
                    match enqueue_pub.send(publ) {
                        Ok(_) => (),
                        Err(broadcast::error::SendError(publ)) => {
                            debug!("Unhandle message {:?} because there was no receiver", publ);
                        }
                    }
                }
                Some(Ok(VariablePacket::PubackPacket(pkt))) => {
                    let channel = {
                        let mut state = state.write().await;
                        state.outstanding.remove(&pkt.packet_identifier())
                    };

                    match channel {
                        Some(channel) => channel
                            .send(Ok(()))
                            .expect("puback response callback failed"),
                        None => warn!(
                            "Received a packet identifier without an outstanding channel. pid={}",
                            pkt.packet_identifier()
                        ),
                    }
                }
                Some(Ok(VariablePacket::SubackPacket(pkt))) => {
                    let channel = {
                        let mut state = state.write().await;
                        state.outstanding.remove(&pkt.packet_identifier())
                    };

                    // TODO Send them back. Will require a different outstanding field.
                    let subs = pkt.subscribes().to_owned();

                    match channel {
                        Some(channel) => channel
                            .send(Ok(()))
                            .expect("puback response callback failed"),
                        None => warn!(
                            "Received a packet identifier without an outstanding channel. pid={}",
                            pkt.packet_identifier()
                        ),
                    }
                }
                Some(Ok(packet)) => {
                    trace!("PACKET {:?}", packet);
                }
            }
        }
    }

    use futures::select;

    // handle packet to the server
    async fn network_write<S: Sink<VariablePacket, Error = std::io::Error> + Unpin>(
        state: Arc<RwLock<ConnectionState>>,
        commands: mpsc::Receiver<WriterRequest>,
        mut packet_sink: S,
        shutdown: broadcast::Receiver<()>,
    ) {
        use either::Either;

        let s1 = tokio_stream::wrappers::ReceiverStream::new(commands);
        let s2 = tokio_stream::wrappers::BroadcastStream::new(shutdown);
        let mut f = s1.map(Either::Left).merge(s2.map(Either::Right));

        loop {
            match f.next().await {
                Some(Either::Left(WriterRequest { result, command })) => {
                    match command {
                        WriterCommand::Publish(topic_name, qos, payload) => {
                            // pub fn new<P: Into<Vec<u8>>>(topic_name: TopicName, qos: QoSWithPacketIdentifier, payload: P) -> PublishPacket
                            let pkt = match (qos, result) {
                                (QualityOfService::Level0, _) => {
                                    // Just write the publish packet
                                    PublishPacket::new(
                                        topic_name,
                                        QoSWithPacketIdentifier::Level0,
                                        payload,
                                    )
                                }
                                (QualityOfService::Level1, Some(response)) => {
                                    // generate pid and register a puback callback
                                    let pid = {
                                        let mut s = state.write().await;
                                        let pid = s.next_pid();
                                        let existing = s.outstanding.insert(pid, response);
                                        if (existing.is_some()) {
                                            warn!("Well, we did rewrite an existing packet id :(. pid={}", pid)
                                        }
                                        pid
                                    };

                                    PublishPacket::new(
                                        topic_name,
                                        QoSWithPacketIdentifier::Level1(pid),
                                        payload,
                                    )
                                }
                                _ => panic!("QoS2 or QoS1 without callback, aren't supported"),
                            };
                            packet_sink.send(pkt.into()).await;
                        }
                        WriterCommand::Subscribe() => {
                            // SubscribePacket { fn new(pkid: u16, subscribes: Vec<(TopicFilter, QualityOfService)>) -> SubscribePacket
                        }
                        WriterCommand::Unsubscribe() => {}
                        WriterCommand::Disconnect() => {}
                    }
                }
                Some(Either::Right(e)) => {
                    if e.is_err() {
                        error!("shutdown receiver lagged behind, which should not happen as it's used as a deferred");
                    }

                    // Received the shut down signal, exiting the loop
                    break;
                }
                None => {
                    warn!("Unexpected end of network_write streams");
                    break;
                }
            }
        }
    }

    // TODO
    // handle keep alive timings
    async fn keep_alive(
        state: Arc<RwLock<ConnectionState>>,
        ping_resps: mpsc::Receiver<()>,
        shutdown: broadcast::Receiver<()>,
    ) {
    }
}
