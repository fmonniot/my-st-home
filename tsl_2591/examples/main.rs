#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
fn main() {
    use linux_embedded_hal::I2cdev;
    use tsl_2591::TSL2591Sensor;
    let dev = I2cdev::new("/dev/i2c-1").unwrap();
    let mut sensor = TSL2591Sensor::new(dev);

    println!(
        "Gain is: {}",
        lux_dev.get_gain().expect("Unable to get gain")
    );
    println!(
        "Integration time is: {}",
        lux_dev
            .get_integration_time()
            .expect("Unable to get integration time")
    );

    let mut iteration = 0;
    while iteration < 5 {
        iteration += 1;
        let visible = lux_dev.visible().unwrap();
        let infrared = lux_dev.infrared().unwrap();
        let full_spectrum = lux_dev.full_spectrum().unwrap();
        let lux = lux_dev.lux().unwrap();
        println!("Visible: {}", visible);
        println!("Infrared: {}", infrared);
        println!("Full Spectrum: {}", full_spectrum);
        println!("Lux: {}", lux);
        println!("");
    }
}
