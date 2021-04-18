//! Provides actor-based network capabilities.
//!
//! Network utilities uses an actor to represent an underlying socket.
//! We also use tokio-util's [`tokio_util::codec::Encoder`] and
//! [`tokio_util::codec::Decoder`] to transform the socket bytes into
//! messages in the actor world.

pub mod udp {
    use crate::actor::{Actor, ActorRef, Context, Message, Receiver, StreamHandle};
    use futures::{SinkExt, stream::{SplitSink, SplitStream, StreamExt}};
    use std::convert::From;
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;
    use tokio_util::{
        codec::{Decoder, Encoder},
        udp::UdpFramed,
    };

    pub enum CreationError {
        IO(std::io::Error),
    }

    impl From<std::io::Error> for CreationError {
        fn from(e: std::io::Error) -> Self {
            CreationError::IO(e)
        }
    }

    // TODO Way better name
    #[derive(Debug, Clone)]
    enum Msg<I: Message> {
        Write(I),
    }

    impl<I: Message> Message for Msg<I> {}

    pub struct Udp<C, I>
    where
        C: Decoder,
    {
        addr: SocketAddr,
        sink: SplitSink<UdpFramed<C>, (I, SocketAddr)>,
        // This is only used to pass the stream between the struct creation
        // and the actor starting. At which point the stream will be taken
        // and used to feed messages to the actor itself.
        stream: Option<SplitStream<UdpFramed<C>>>,

        stream_handle: Option<StreamHandle>,

        send_packet: Box<dyn Fn(UdpPacketResult<C::Item, C::Error>) -> () + Send + Sync>,
    }

    pub async fn create<A, C, I>(
        addr: SocketAddr,
        codec: C,
        target: ActorRef<A>,
    ) -> Result<Udp<C, I>, CreationError>
    where
        C: Decoder<Item = I> + Encoder<I> + Unpin,
        I: Message,
        A: Receiver<UdpPacketResult<C::Item, <C as Decoder>::Error>>,
        <C as Decoder>::Error: Message,
    {
        // Blocking here should be ok-ish, as the async process is mostly on resolving addresses
        // which me don't really do in this project (we use IP addresses only in UDP)
        let handle = tokio::runtime::Handle::current();
        let socket = handle.block_on(UdpSocket::bind(addr.clone()))?;
        let framed = UdpFramed::new(socket, codec);

        let (sink, stream) = framed.split();

        let send_packet = move |item| {
            target.send_msg(item);
        };

        Ok(Udp {
            addr,
            sink,
            stream: Some(stream),
            stream_handle: None,
            send_packet: Box::new(send_packet),
        })
    }

    impl<C, I> Actor for Udp<C, I>
    where
        I: Message,
        C: Decoder<Item = I> + Send + 'static + Unpin,
        C::Item: Message,
        C::Error: Message,
    {
        fn pre_start(&mut self, ctx: &Context<Self>) {
            if let Some(stream) = self.stream.take() {
                let handle = ctx.subscribe_to_stream(stream);
                self.stream_handle = Some(handle);
            }
        }
    }

    impl<C, I> Receiver<Msg<I>> for Udp<C, I>
    where
        I: Message,
        C: Decoder<Item = I> + Encoder<I> + Send + Unpin + 'static,
        <C as Decoder>::Item: Message,
        <C as Decoder>::Error: Message,
        <C as Encoder<I>>::Error: Send,
     {
        fn recv(&mut self, _ctx: &Context<Self>, msg: Msg<I>) {
            match msg {
                Msg::Write(item) => {
                    let addr = self.addr.clone();
                    let s = self.sink.send((item, addr));
                    tokio::spawn(async move {
                        s.await
                    });
                }
            }

        }
    }

    type UdpPacketResult<I, E> = Result<(I, SocketAddr), E>;

    impl<I, E> Message for UdpPacketResult<I, E>
    where
        I: Message,
        E: Message,
    {
    }

    impl<C, I> Receiver<UdpPacketResult<C::Item, C::Error>> for Udp<C, I>
    where
        I: Message,
        C: Decoder<Item = I> + Send + Unpin + 'static,
        C::Item: Message,
        C::Error: Message,
    {
        fn recv(&mut self, _ctx: &Context<Self>, msg: UdpPacketResult<C::Item, C::Error>) {
            (self.send_packet)(msg)
        }
    }
}
