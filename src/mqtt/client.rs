use futures::Stream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;



struct MqttClient {

}
enum QoS {
    QoS_0,QoS_1,QoS_2
}

struct TopicSubscription(String, QoS);

//stubs
type ConnectOptions = ();
type SubscribeResponse = ();
type ConnectReturnCode = ();

pub struct Publish {
    topic: String,
    payload: Vec<u8>,
    qos: QoS,
    retain: bool,
}

impl MqttClient {

    pub fn new<T>(&self, opts: T) -> MqttClient
    where T: Into<Option<ConnectOptions>> {
        unimplemented!()
    }

    // TODO Probably implement our own enum error here. We may want to include more
    // cases (disconnected for example. not sure tbh).
    pub fn subscriptions(&self) -> impl Stream<Item = Result<(),BroadcastStreamRecvError >> {
        // it's a known error that unimplemented/todo macros cannot fill in impl Trait.
        // Will have to live with the compiler error until I put the queue in here
        unimplemented!()
    }
    
    pub async fn connect(&self) -> ConnectReturnCode {
        unimplemented!()
    }

    pub async fn subscribe<I: Iterator<Item = TopicSubscription>>(&self, topics: I) -> SubscribeResponse {
        unimplemented!()
    }

    pub async fn disconnect(&self) {
        unimplemented!()
    }

    pub async fn publish(&self, message: Publish) -> Result<(), ()> {
        unimplemented!()
    }

}