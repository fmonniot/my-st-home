//! Provides actor-based network capabilities.
//!
//! Network utilities uses an actor to represent an underlying socket.
//! We also use tokio-util's [`tokio_util::codec::Encoder`] and
//! [`tokio_util::codec::Decoder`] to transform the socket bytes into
//! messages in the actor world.

pub mod udp {
    use crate::actor::{Actor, ActorRef, Context, Message, Receiver, StreamHandle};
    use std::convert::From;
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;
    use tokio_util::codec::{Decoder, Encoder};

    #[derive(Debug)]
    pub enum CreationError {
        IO(std::io::Error),
    }

    impl From<std::io::Error> for CreationError {
        fn from(e: std::io::Error) -> Self {
            CreationError::IO(e)
        }
    }

    impl std::fmt::Display for CreationError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                CreationError::IO(e) => write!(f, "IO({})", e)
            }
        }
    }

    impl std::error::Error for CreationError {}

    // TODO Way better name
    // name that Command ?
    #[derive(Debug, Clone)]
    pub enum Msg<I: Message> {
        Write(I),
    }

    impl<I: Message> Message for Msg<I> {}

    pub struct Udp<C>
    where
        C: Decoder,
    {
        addr: SocketAddr,
        write: WriteFramedUdp<C>,
        // This field is only used to pass the stream between the struct creation
        // and the actor starting. At which point the stream will be taken
        // and used to feed messages to the actor itself.
        read: Option<ReadFramedUdp<C>>,

        stream_handle: Option<StreamHandle>,

        send_packet: Box<dyn Fn(UdpPacketResult<C::Item, C::Error>) -> () + Send + Sync>,
    }

    pub fn create<A, C, In, Out>(
        addr: SocketAddr,
        codec: C,
        target: ActorRef<A>,
    ) -> Result<Udp<C>, CreationError>
    where
        C: Decoder<Item = Out> + Encoder<In> + Unpin + Clone,
        Out: Message,
        In: Message,
        A: Receiver<UdpPacketResult<C::Item, <C as Decoder>::Error>>,
        <C as Decoder>::Error: Message,
    {
        // Blocking here should be ok-ish, as the async process is mostly on resolving addresses
        // which me don't really do in this project (we use IP addresses only in UDP)
        let handle = tokio::runtime::Handle::current();
        let socket = handle.block_on(UdpSocket::bind(addr.clone()))?;

        // TODO Let's not do that. Instead, let's have our own framed struct which take an
        // Arc<UdpSocket> and do only the read part (impl Stream only).
        // We can then store the socket in the actor state and handle write directly instead
        // of going through the cumbersome Sink interface
        let (write, read) = framed(socket, codec);

        let send_packet = move |item| {
            target.send_msg(item);
        };

        Ok(Udp {
            addr,
            write,
            read: Some(read),
            stream_handle: None,
            send_packet: Box::new(send_packet),
        })
    }

    impl<C> Actor for Udp<C>
    where
        C: Decoder + Send + 'static + Unpin,
        C::Item: Message,
        C::Error: Message,
    {
        fn pre_start(&mut self, ctx: &Context<Self>) {
            if let Some(stream) = self.read.take() {
                let handle = ctx.subscribe_to_stream(stream);
                self.stream_handle = Some(handle);
            }
        }
    }

    impl<C, In, Out> Receiver<Msg<Out>> for Udp<C>
    where
        In: Message,
        Out: Message,
        C: Decoder<Item = In> + Encoder<Out> + Send + Unpin + Clone + 'static,
        <C as Decoder>::Item: Message,
        <C as Decoder>::Error: Message,
        <C as Encoder<Out>>::Error: Send,
    {
        fn recv(&mut self, _ctx: &Context<Self>, msg: Msg<Out>) {
            match msg {
                Msg::Write(item) => {
                    let addr = self.addr.clone();
                    let mut w = self.write.clone();

                    tokio::spawn(async move {
                        let _ = w.send((item, addr)).await; // TODO Error handling
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

    impl<C, I> Receiver<UdpPacketResult<C::Item, C::Error>> for Udp<C>
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

    // Framing construct for UDP packets. Trying to get something based off
    // tokio-util for reading, but with easier writing capacity.

    use bytes::BufMut;
    use bytes::BytesMut;
    use core::task::{self, Poll};
    use futures::Stream;
    use futures_core::ready;
    use std::mem::MaybeUninit;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::io::ReadBuf;

    /// A [`Stream`] interface to an underlying `UdpSocket`, using
    /// the `Decoder` traits to decode frames.
    ///
    /// Taken from `tokio-util` with the `Sink` trait removed. This is because
    /// the `Sink` trait is too bothersome to use in the context of an actor.
    /// Instead we provides a simpler [`WriteFramedUdp`].
    ///
    /// See [`tokio_util::udp::UdpFramed`] for original `Stream` implementation.
    struct ReadFramedUdp<C> {
        socket: Arc<UdpSocket>,
        codec: C,
        rd: BytesMut, // rename to buf
        is_readable: bool,
        current_addr: Option<SocketAddr>,
    }

    #[derive(Clone)]
    struct WriteFramedUdp<C> {
        socket: Arc<UdpSocket>,
        codec: C,
        buf: BytesMut,
    }

    // TODO Can we go with lower buffers ? Evaluate the average size frame for LIFX.
    const INITIAL_RD_CAPACITY: usize = 64 * 1024;
    const INITIAL_WR_CAPACITY: usize = 8 * 1024;

    fn framed<C>(socket: UdpSocket, codec: C) -> (WriteFramedUdp<C>, ReadFramedUdp<C>)
    where
        C: Clone,
    {
        let socket = Arc::new(socket);

        let write = WriteFramedUdp {
            socket: socket.clone(),
            codec: codec.clone(),
            buf: BytesMut::with_capacity(INITIAL_WR_CAPACITY),
        };

        let read = ReadFramedUdp {
            socket,
            codec,
            rd: BytesMut::with_capacity(INITIAL_RD_CAPACITY),
            is_readable: false,
            current_addr: None,
        };

        (write, read)
    }

    impl<C> WriteFramedUdp<C> {
        async fn send<I>(self: &mut Self, item: (I, SocketAddr)) -> Result<(), C::Error>
        where
            C: Encoder<I>,
        {
            let (frame, out_addr) = item;

            self.codec.encode(frame, &mut self.buf)?;
            self.socket.send_to(&self.buf, out_addr).await?;
            self.buf.clear();

            Ok(())
        }
    }

    // See tokio_util::udp::UdpFramed for original implementation (version 0.6.3)
    impl<C: Decoder + Unpin> Stream for ReadFramedUdp<C> {
        type Item = Result<(C::Item, SocketAddr), C::Error>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
            let pin = self.get_mut();

            pin.rd.reserve(INITIAL_RD_CAPACITY);

            loop {
                // Are there still bytes left in the read buffer to decode?
                if pin.is_readable {
                    if let Some(frame) = pin.codec.decode_eof(&mut pin.rd)? {
                        let current_addr = pin
                            .current_addr
                            .expect("will always be set before this line is called");

                        return Poll::Ready(Some(Ok((frame, current_addr))));
                    }

                    // if this line has been reached then decode has returned `None`.
                    pin.is_readable = false;
                    pin.rd.clear();
                }

                // We're out of data. Try and fetch more data to decode
                let addr = unsafe {
                    // Convert `&mut [MaybeUnit<u8>]` to `&mut [u8]` because we will be
                    // writing to it via `poll_recv_from` and therefore initializing the memory.
                    let buf = &mut *(pin.rd.chunk_mut() as *mut _ as *mut [MaybeUninit<u8>]);
                    let mut read = ReadBuf::uninit(buf);
                    let ptr = read.filled().as_ptr();
                    let res = ready!(Pin::new(&mut pin.socket).poll_recv_from(cx, &mut read));

                    assert_eq!(ptr, read.filled().as_ptr());
                    let addr = res?;
                    pin.rd.advance_mut(read.filled().len());
                    addr
                };

                pin.current_addr = Some(addr);
                pin.is_readable = true;
            }
        }
    }
}
