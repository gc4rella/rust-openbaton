pub mod openbaton;
pub mod entities;

use std::cell::RefCell;
use std::fs::File;
use std::io::Read;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use amqp::{Session, Channel, Table, Basic, protocol};
use serde_json::Value;

extern crate ctrlc;
extern crate toml;
extern crate amqp;
extern crate serde;
extern crate serde_json;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[derive(Debug, Deserialize)]
struct Config {
    broker_ip: String,
    broker_port: u16,
    username: String,
    password: String,
    #[serde(rename = "type")]
    _type: String,
    name: String,
    vhost: String,
}

#[derive(Debug, Serialize)]
struct VimDriverDeregisterRequest {
    #[serde(rename = "username")]
    usr: String,
    #[serde(rename = "password")]
    pwd: String,
    action: String,
}

impl VimDriverDeregisterRequest {
    fn new(usr: String, pwd: String) -> VimDriverDeregisterRequest {
        VimDriverDeregisterRequest {
            usr: usr,
            pwd: pwd,
            action: String::from("deregister"),
        }
    }
}

#[derive(Debug, Serialize)]
struct VimDriverRegisterRequest {
    #[serde(rename = "type")]
    plugin_full_name: String,
    action: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RabbitCreds {
    #[serde(rename = "rabbitUsername")]
    pub username: String,
    #[serde(rename = "rabbitPassword")]
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum MethodName {
    #[serde(rename = "refresh")]
    Refresh,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenBatonPluginMessage {
    #[serde(rename = "methodName")]
    method_name: MethodName,
    #[serde(rename = "selector")]
    selector: String,
    #[serde(rename = "parameters")]
    parameters: Vec<String>,
    #[serde(rename = "interfaceClass")]
    interface_class: Value,
}

fn parse(path: &str) -> Config {
    let mut file_content = String::new();
    let mut f = File::open(path).expect("Unable to open file");
    f.read_to_string(&mut file_content).expect(
        "Unable to read string",
    );
    info!("{}", file_content);
    let decoded: Config = toml::from_str(&file_content[..]).unwrap();
    decoded
}

fn get_user_and_pwd(config: &Config) -> (String, String) {
    let amqp_uri = &format!(
        "amqp://{}:{}@{}:{}/{}",
        config.username,
        config.password,
        config.broker_ip,
        config.broker_port,
        config.vhost
    )
        [..];
    info!("Amqp URI: {}", amqp_uri);
    let mut session = Session::open_url(amqp_uri).unwrap();
    debug!("Opened Session");

    let mut channel = session.open_channel(1).unwrap();
    debug!("Opened Channel");
    let plugin_full_name = &format!("vim-drivers.{}.{}", config._type, config.name)[..];
    let register_msg = VimDriverRegisterRequest {
        plugin_full_name: plugin_full_name.to_string(),
        action: "register".to_string(),
    };
    let register_msg_json = serde_json::to_string(&register_msg).unwrap();

    // queue: &str, passive: bool, durable: bool, exclusive: bool, auto_delete: bool, nowait: bool,
    // arguments: Table
    let queue_declare = channel
        .queue_declare("", false, false, true, true, false, Table::new())
        .unwrap();

    debug!("Queue declare: {:?}", queue_declare);
    channel.basic_prefetch(1).ok().expect("Failed to prefetch");
    let queue_name = &queue_declare.queue[..];

    debug!("Declaring consumers...");

    // consumer, queue: &str, consumer_tag: &str, no_local: bool, no_ack: bool, exclusive: bool,
    // nowait: bool, arguments: Table

    let (tx, rx) = mpsc::channel();
    let closure_consumer = move |_chan: &mut Channel,
                                 _deliver: protocol::basic::Deliver,
                                 _headers: protocol::basic::BasicProperties,
                                 data: Vec<u8>| {
        let rc: RabbitCreds = serde_json::from_str(&String::from_utf8(data).unwrap()[..]).unwrap();

        _chan.close(200, "Bye").unwrap();

        tx.send(rc.username).unwrap();
        tx.send(rc.password).unwrap();
    };

    channel
        .basic_consume(
            closure_consumer,
            queue_name.to_string(),
            "".to_string(),
            false,
            false,
            false,
            false,
            Table::new(),
        )
        .unwrap();

    channel
        .basic_publish(
            "",
            "nfvo.manager.handling",
            true,
            false,
            protocol::basic::BasicProperties {
                reply_to: Some(queue_name.to_string()),
                content_type: Some("text".to_string()),
                ..Default::default()
            },
            register_msg_json.as_bytes().to_vec(),
        )
        .unwrap();

    // let start_channel = Arc::clone(&channel);
    thread::spawn(move || {
        debug!("start consuming...");
        // let mut ch = start_channel.lock().unwrap();
        channel.start_consuming();
    });


    // handle.join();
    let username = rx.recv().unwrap();
    debug!("Username: {:?}", username);
    let password = rx.recv().unwrap();
    debug!("Password: {:?}", password);
    return (username, password);
}

pub fn start_instances<V: openbaton::VimDriver + 'static>(
    num_of_instances: usize,
    vim_driver: V,
    vim_type: &str,
    vim_name: &str,
) {
    info!("Starting {} instances", num_of_instances);

    let config: Config = parse("config.toml");
    let (usr, pwd) = get_user_and_pwd(&config);
    info!("Got  usr:{} anf pwd:{} instances", usr, pwd);

    let amqp_uri = &format!(
        "amqp://{}:{}@{}:{}/{}",
        usr,
        pwd,
        config.broker_ip,
        config.broker_port,
        config.vhost
    )
        [..];
    info!("Amqp URI: {}", amqp_uri);
    let mut session = Session::open_url(amqp_uri).unwrap();
    debug!("Opened Session");

    let mut channel = session.open_channel(1).unwrap();
    debug!("Opened Channel");

    let queue_declare = channel
        .queue_declare(
            format!("vim-drivers.{}.{}", vim_type, vim_name),
            false,
            false,
            false,
            true,
            false,
            Table::new(),
        )
        .unwrap();

    debug!("Queue declare: {:?}", queue_declare);

    let queue_name = &queue_declare.queue[..];
    let exchange_name = "openbaton-exchange";
    debug!("Declaring exchange...");
    let exchange_declare1 = channel.exchange_declare(
        exchange_name,
        "topic",
        true,
        true,
        false,
        false,
        false,
        Table::new(),
    );
    debug!("Exchange declare: {:?}", exchange_declare1);
    debug!("Binding {} with {}", queue_name, exchange_name);
    // let dur = std::time::Duration::from_secs(100);
    // std::thread::sleep(dur);
    channel
        .queue_bind(queue_name, exchange_name, queue_name, false, Table::new())
        .unwrap();
    debug!("Bind to exchange");
    channel.basic_prefetch(1).ok().expect("Failed to prefetch");

    let vim_driver = RefCell::new(vim_driver);
    let listener_closure = move |_chan: &mut Channel,
                                 _deliver: protocol::basic::Deliver,
                                 headers: protocol::basic::BasicProperties,
                                 data: Vec<u8>| {
        let plugin_msg: OpenBatonPluginMessage =
            serde_json::from_str(&String::from_utf8(data).unwrap()[..]).unwrap();


        match plugin_msg.method_name {
            MethodName::Refresh => {
                //                let res: <V as openbaton::VimDriver>::T;
                let res = vim_driver.borrow_mut().refresh(
                    serde_json::from_str(
                        &plugin_msg.parameters[0][..],
                    ).unwrap(),
                );
                if let amqp::TableEntry::LongString(ref reply_to) = headers.headers.unwrap()["reply_to"] {
                    let properties = protocol::basic::BasicProperties {
                        content_type: Some("text".to_owned()),
                        ..Default::default()
                    };
                    _chan
                        .basic_publish(
                            exchange_name,
                            &reply_to[..],
                            true,
                            false,
                            properties,
                            serde_json::to_string(&res).unwrap().into_bytes(),
                        )
                        .ok()
                        .expect("Failed publishing");
                }
            }
        }
    };


    channel
        .basic_consume(
            listener_closure,
            queue_name,
            "",
            false,
            false,
            false,
            false,
            Table::new(),
        )
        .unwrap();

    let arc_chan = Arc::new(Mutex::new(channel));
    let chan = Arc::clone(&arc_chan);

    let arc_sess = Arc::new(Mutex::new(session));
    let sess = Arc::clone(&arc_sess);

    let arc_usr = Arc::new(usr);
    let arc_pwd = Arc::new(pwd);

    ctrlc::set_handler(move || {
        let deregister_msg_json = serde_json::to_string(&VimDriverDeregisterRequest::new(String::from(arc_usr.as_str()), String::from(arc_pwd.as_str()))).unwrap();
        let mut mutex_chan = chan.lock().unwrap();
        debug!("Unregistering: {}", deregister_msg_json);
        mutex_chan.basic_publish(
            "",
            "nfvo.manager.handling",
            true,
            false,
            protocol::basic::BasicProperties { ..Default::default() },
            deregister_msg_json.as_bytes().to_vec()
        ).unwrap();
        mutex_chan.close(22, "bye").unwrap();
        sess.lock().unwrap().close(200, "Good Bye");
    }).expect("Error setting Ctrl-C handler");
    debug!("Correctly set ctrl-c handler");
    let chan1 = Arc::clone(&arc_chan);
    chan1.lock().unwrap().start_consuming();
}


#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
