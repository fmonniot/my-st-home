use embedded_hal_mock::i2c::{Mock as I2cMock, Transaction as I2cTrans};
use tsl_2591::TSL2591Sensor;

// common properties

const ADDR: u8 = 0x29;
const COMMAND_BIT: u8 = 0xA0;

const ENABLE_POWEROFF: u8 = 0x00;
const ENABLE_POWERON: u8 = 0x01;
const ENABLE_AEN: u8 = 0x02;
const ENABLE_AIEN: u8 = 0x10;
const ENABLE_NPIEN: u8 = 0x80;
const REGISTER_ENABLE: u8 = 0x00;
const REGISTER_CONTROL: u8 = 0x01;
const REGISTER_DEVICE_ID: u8 = 0x12;
const REGISTER_CHAN0_LOW: u8 = 0x14;
const REGISTER_CHAN1_LOW: u8 = 0x16;

// TODO Fix the numbers
pub fn new(transactions: &[I2cTrans]) -> TSL2591Sensor<I2cMock> {
    let mut tr = vec![
        // read device id
        I2cTrans::write_read(ADDR, vec![COMMAND_BIT | REGISTER_DEVICE_ID], vec![0x50]),
        // set gain
        I2cTrans::write_read(ADDR, vec![COMMAND_BIT | REGISTER_CONTROL], vec![0]),
        I2cTrans::write(ADDR, vec![COMMAND_BIT | REGISTER_CONTROL, 0]),
        // set integration time
        I2cTrans::write_read(ADDR, vec![COMMAND_BIT | REGISTER_CONTROL], vec![0]),
        I2cTrans::write(ADDR, vec![COMMAND_BIT | REGISTER_CONTROL, 0]),
        // enable
        I2cTrans::write(
            ADDR,
            vec![
                COMMAND_BIT | REGISTER_ENABLE,
                ENABLE_POWERON | ENABLE_AEN | ENABLE_AIEN | ENABLE_NPIEN,
            ],
        ),
    ];
    tr.extend_from_slice(transactions);

    println!("vec: {:?}", tr);
    TSL2591Sensor::new(I2cMock::new(&tr)).unwrap()
}

pub fn destroy(sensor: TSL2591Sensor<I2cMock>) {
    sensor.destroy().done();
}

// tests

#[test]
fn can_create_and_destroy() {
    let sensor = new(&[]);
    destroy(sensor);
}
