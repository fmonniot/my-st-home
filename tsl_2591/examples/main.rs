#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
fn main() {
    use rppal::i2c::I2c;

    let i2c = I2c::new().expect("Unable to open I2C bus.");
    let mut sensor = tsl_2591::TSL2591Sensor::new(i2c).unwrap();

    println!(
        "Gain is: {}",
        sensor.get_gain().expect("Unable to get gain")
    );
    println!(
        "Integration time is: {}",
        sensor
            .get_integration_time()
            .expect("Unable to get integration time")
    );

    let mut iteration = 0;
    while iteration < 5 {
        iteration += 1;
        let visible = sensor.visible().unwrap();
        let infrared = sensor.infrared().unwrap();
        let full_spectrum = sensor.full_spectrum().unwrap();
        let lux = sensor.lux().unwrap();
        println!("Visible: {}", visible);
        println!("Infrared: {}", infrared);
        println!("Full Spectrum: {}", full_spectrum);
        println!("Lux: {}", lux);
        println!("");
    }
}
