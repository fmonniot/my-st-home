use std::{fs::File, io::BufReader, path::Path, time::Duration};
use serde::{Deserialize, Serialize};
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
use tokio::time::sleep;
use std::time::SystemTime;
use paho_mqtt as mqtt;
use futures::stream::StreamExt;

use embedded_graphics::{
    fonts::{Font12x16, Font6x8, Text},
    prelude::*,
    primitives::{Circle, Line},
    style::PrimitiveStyle,
    text_style,
};
use embedded_hal::prelude::*;
use epd_waveshare::{
    color::*,
    epd7in5_v2::{Display7in5, EPD7in5},
    graphics::{Display, DisplayRotation},
    prelude::*,
};
use linux_embedded_hal::{
    spidev::{self, SpidevOptions},
    sysfs_gpio::Direction,
    Delay, Pin, Spidev,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {


    // Configure SPI
    // Settings are taken from
    let mut spi = Spidev::open("/dev/spidev0.0").expect("spidev directory");
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(4_000_000)
        .mode(spidev::SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options).expect("spi configuration");

    // Configure Digital I/O Pin to be used as Chip Select for SPI
    let cs = Pin::new(26); //BCM7 CE0
    cs.export().expect("cs export");
    while !cs.is_exported() {}
    cs.set_direction(Direction::Out).expect("CS Direction");
    cs.set_value(1).expect("CS Value set to 1");

    let busy = Pin::new(5); //pin 29
    busy.export().expect("busy export");
    while !busy.is_exported() {}
    busy.set_direction(Direction::In).expect("busy Direction");
    //busy.set_value(1).expect("busy Value set to 1");

    let dc = Pin::new(6); //pin 31 //bcm6
    dc.export().expect("dc export");
    while !dc.is_exported() {}
    dc.set_direction(Direction::Out).expect("dc Direction");
    dc.set_value(1).expect("dc Value set to 1");

    let rst = Pin::new(16); //pin 36 //bcm16
    rst.export().expect("rst export");
    while !rst.is_exported() {}
    rst.set_direction(Direction::Out).expect("rst Direction");
    rst.set_value(1).expect("rst Value set to 1");

    let mut delay = Delay {};

    // Display7in5, EPD7in5
    let mut epd7in5 =
        EPD7in5::new(&mut spi, cs, busy, dc, rst, &mut delay).expect("eink initalize error");

    println!("Test all the rotations");
    let mut display = Display7in5::default();

    display.set_rotation(DisplayRotation::Rotate0);
    draw_text(&mut display, "Rotate 0!", 5, 50);

    display.set_rotation(DisplayRotation::Rotate90);
    draw_text(&mut display, "Rotate 90!", 5, 50);

    display.set_rotation(DisplayRotation::Rotate180);
    draw_text(&mut display, "Rotate 180!", 5, 50);

    display.set_rotation(DisplayRotation::Rotate270);
    draw_text(&mut display, "Rotate 270!", 5, 50);

    epd7in5.update_frame(&mut spi, &display.buffer())?;
    epd7in5
        .display_frame(&mut spi)
        .expect("display frame new graphics");
    delay.delay_ms(5000u16);

    println!("Now test new graphics with default rotation and some special stuff:");
    display.clear_buffer(Color::White);

    // draw a analog clock
    let _ = Circle::new(Point::new(64, 64), 64)
        .into_styled(PrimitiveStyle::with_stroke(Black, 1))
        .draw(&mut display);
    let _ = Line::new(Point::new(64, 64), Point::new(0, 64))
        .into_styled(PrimitiveStyle::with_stroke(Black, 1))
        .draw(&mut display);
    let _ = Line::new(Point::new(64, 64), Point::new(80, 80))
        .into_styled(PrimitiveStyle::with_stroke(Black, 1))
        .draw(&mut display);

    // draw white on black background
    let _ = Text::new("It's working-WoB!", Point::new(175, 250))
        .into_styled(text_style!(
            font = Font6x8,
            text_color = White,
            background_color = Black
        ))
        .draw(&mut display);

    // use bigger/different font
    let _ = Text::new("It's working-WoB!", Point::new(50, 200))
        .into_styled(text_style!(
            font = Font12x16,
            text_color = White,
            background_color = Black
        ))
        .draw(&mut display);

    // a moving `Hello World!`
    let limit = 10;
    // Can set on the 7"5 because it does not support partial refresh
    //epd7in5.set_lut(&mut spi, Some(RefreshLUT::QUICK)).unwrap();
    epd7in5.clear_frame(&mut spi).unwrap();
    for i in 0..limit {
        println!("Moving Hello World. Loop {} from {}", (i + 1), limit);

        draw_text(&mut display, "  Hello World! ", 5 + i * 12, 50);

        epd7in5.update_frame(&mut spi, &display.buffer()).unwrap();
        epd7in5
            .display_frame(&mut spi)
            .expect("display frame new graphics");

        delay.delay_ms(1_000u16);
    }

    println!("Finished tests - going to sleep");
    epd7in5.sleep(&mut spi);


    return Ok(());

    // TODO Change the .with_file_name to not have to pass a dummy file name here
    let cfg = Configuration::from_directory("/Users/francoismonniot/Projects/local/mqtt-console/data/project/nothing")?;

    if cfg.onboarding.identity_type != "ED25519" {
        println!("Only ED25519 keys are supported at the moment. {} passed", cfg.onboarding.identity_type);
        return Ok(()) // TODO return an error
    }

    println!("Configuration: {:#?}\n", cfg);

    // Key deserialization experimentation (could probably go into configuration)
    let public_key_bytes = base64::decode(&cfg.device_info.public_key)?;
    let private_key_bytes = base64::decode(&cfg.device_info.private_key)?;

    let public_key: PublicKey = PublicKey::from_bytes(&public_key_bytes)?;
    let secret_key: SecretKey = SecretKey::from_bytes(&private_key_bytes)?;
    let keypair = Keypair { public: public_key, secret: secret_key };

    let header = Header::with_serial(&cfg.device_info.serial_number);
    let body = Body::generate(cfg.onboarding.mn_id.clone());
    let jwt = generate_jwt(header, body, &keypair)?;


    //
    // Hack something for MQTT and then try to find a good abstraction for other to use
    //

    let mut mqtt_client = mqtt::CreateOptionsBuilder::new()
        .server_uri("ssl://mqtt-regional-useast1.api.smartthings.com:8883")
        .client_id("mqtt-console-rs")
        .persistence(None)
        .create_client()?;

    // Need to pass those, otherwise the c lib fails on NULL…
    let ssl_opts = mqtt::SslOptionsBuilder::new()
        .finalize();

    let conn_options = mqtt::ConnectOptionsBuilder::new()
        .clean_session(true)
        .user_name(&cfg.stcli.device_id)
        .password(jwt)
        .mqtt_version(mqtt::MQTT_VERSION_3_1_1)
        .ssl_options(ssl_opts)
        .finalize();
    

        
    // connect
    mqtt_client.connect(conn_options).await?;
    let mut messages = mqtt_client.get_stream(25);

    println!("connected");

    // subscribe
    let topics = [
        format!("/v1/commands/{}", &cfg.stcli.device_id),
        format!("/v1/notifications/{}", &cfg.stcli.device_id),
    ];
    let topics_qos = [1, 1];
    //mqtt_client.subscribe_many(&topics, &topics_qos).await?;

    println!("subscribed");

    tokio::spawn(async move {
        while let Some(msg_opt) = messages.next().await {
            if let Some(msg) = msg_opt {

                let topic = msg.topic();
                let _payload = msg.payload();

                println!("Received message on {}: {}", topic, msg.payload_str());

            } else {
                // A "None" means we were disconnected.
            }
        }
    });

    //let topic = format!("/v1/deviceEvents/{}", cfg.stcli.device_id);
    //let msg = mqtt::Message::new(topic, "{}", mqtt::QOS_1);
    //mqtt_client.publish(msg).await?;

    //sleep(Duration::from_millis(30_000)).await;
    println!("Sleep expired, closing");

    mqtt_client.disconnect(None).await?;

    Ok(())
}


fn draw_text(display: &mut Display7in5, text: &str, x: i32, y: i32) {
    let _ = Text::new(text, Point::new(x, y))
        .into_styled(text_style!(
            font = Font6x8,
            text_color = Black,
            background_color = White
        ))
        .draw(display);
}

// JWT (custom implementation for now. TODO see if we can use a standard crate)

fn generate_jwt(header: Header, body: Body, keypair: &Keypair) -> Result<String, Box<dyn std::error::Error>> {
    let h = serde_json::to_vec(&header)?;
    let b = serde_json::to_vec(&body)?;

    let h2 = base64::encode(h);
    let b2 = base64::encode(b);

    let msg = format!("{}.{}", h2, b2);
    let signature = keypair.try_sign(msg.as_bytes())?;

    Ok(format!("{}.{}.{}", h2, b2, base64::encode(signature)))
}


#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Header {
  alg: String,
  kty: String,
  crv: String,
  typ: String,
  ver: String,
  kid: String,
}

impl Header {
    fn with_serial(serial: &str) -> Header {
        Header {
            alg: "EdDSA".to_string(),
            kty: "OKP".to_string(),
            crv: "ed25519".to_string(),
            typ: "JWT".to_string(),
            ver: "0.0.1".to_string(),
            kid: serial.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Body {
    iat: String,
    jti: String,
    mn_id: String,
}

impl Body {
    fn generate(mn_id: String) -> Body {
        let jti = uuid::Uuid::new_v4().to_hyphenated().to_string();
        let sys_time = SystemTime::now();
        let iat = sys_time.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        let iat = format!("{}", iat);

        Body {
            iat,
            jti,
            mn_id
        }
    }
}


// Configuration

#[derive(Debug)]
struct Configuration {
    device_info: DeviceInfo,
    stcli: StCli,
    meta: MetaFile,
    onboarding: OnboardingConfig,
}

impl Configuration {
    fn from_directory<P: AsRef<Path>>(p: P) -> Result<Configuration, Box<dyn std::error::Error>> {

        let path = p.as_ref();
        
        let di: DeviceInfoFile = read_json_from_file(path.with_file_name("device_info.json"))?;
        let meta: MetaFile = read_json_from_file(path.with_file_name("meta.json"))?;
        let of: OnboardingConfigFile = read_json_from_file(path.with_file_name("onboarding_config.json"))?;

        Ok(Configuration {
            device_info: di.device_info,
            stcli: di.stcli,
            meta,
            onboarding: of.onboarding_config
        })
    }
}

fn read_json_from_file<P: AsRef<Path>, T: serde::de::DeserializeOwned>(path: P) -> Result<T, Box<dyn std::error::Error>> {
    // Open the file in read-only mode with buffer.
    println!("Reading file: {:?}", path.as_ref());
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let t = serde_json::from_reader(reader)?;

    Ok(t)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceInfoFile {
    device_info: DeviceInfo,
    stcli: StCli,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceInfo {
    firmware_version: String,
    private_key: String,
    public_key: String,
    serial_number: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct StCli {
    device_id: String
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MetaFile {
    location_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OnboardingConfigFile {
    onboarding_config: OnboardingConfig
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OnboardingConfig {
    device_onboarding_id: String,
    mn_id: String,
    setup_id: String,
    vid: String,
    device_type_id: String,
    identity_type: String,
    device_integration_profile_key: DipKey,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DipKey {
    id: String,
    major_version: u8,
    minor_version: u8,
}
