#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::flash::{Async, Flash, ERASE_SIZE};
use embassy_rp::peripherals::FLASH;
use embassy_time::Timer;
use defmt::*;
use serde::{Deserialize, Serialize};
use heapless::String;
use postcard;
use rustc_hash::FxHasher;
use core::hash::{BuildHasherDefault, Hash, Hasher};

// ===================== CONFIG =====================

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Config {
    pub format_version: u16,
    pub device_name: String<64>,
    pub threshold: f32,
    pub boot_count: u32,
}

// Compile-time magic number based on the struct type
const CONFIG_MAGIC: u32 = {
    let mut hasher = BuildHasherDefault::<FxHasher>::default();
    core::any::type_name::<Config>().hash(&mut hasher);
    (hasher.finish() as u32) ^ 0xA5A5A5A5u32
};

const FLASH_SECTOR_ADDR: u32 = 0x100F0000; // Use a high address (last sectors on typical 2MB flash)
const SECTOR_SIZE: usize = 4096;

type MyFlash = Flash<'static, FLASH, Async, SECTOR_SIZE>;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    info!("Flash storage example starting on RP2350");

    let mut flash = Flash::<_, Async, SECTOR_SIZE>::new(p.FLASH);

    match load_config(&mut flash).await {
        Ok(config) => {
            info!("Loaded existing config: {:?}", config);
            let mut updated = config;
            updated.boot_count += 1;
            if updated.device_name.is_empty() {
                let _ = updated.device_name.push_str("RP2350-Demo");
            }
            save_config(&mut flash, &updated).await;
        }
        Err(e) => {
            warn!("No valid config found ({}). Writing defaults.", e);
            let default = Config {
                format_version: 1,
                device_name: String::try_from("RP2350-Demo").unwrap(),
                threshold: 42.5,
                boot_count: 1,
            };
            save_config(&mut flash, &default).await;
        }
    }

    info!("Demo complete. Rebooting in 3 seconds...");
    Timer::after_secs(3).await;
}

async fn load_config(flash: &mut MyFlash) -> Result<Config, &'static str> {
    let mut buf = [0u8; SECTOR_SIZE];
    flash.read(FLASH_SECTOR_ADDR, &mut buf).await.map_err(|_| "flash read failed")?;

    if &buf[0..4] != &CONFIG_MAGIC.to_le_bytes() {
        return Err("magic number mismatch");
    }

    postcard::from_bytes_crc32::<Config>(&buf[4..])
        .map_err(|_| "postcard deserialization or CRC error")
}

async fn save_config(flash: &mut MyFlash, config: &Config) -> bool {
    let Ok(data) = postcard::to_vec_crc32::<_, 512>(config) else {
        error!("Failed to serialize config");
        return false;
    };

    let mut buf = [0xFFu8; SECTOR_SIZE];
    buf[0..4].copy_from_slice(&CONFIG_MAGIC.to_le_bytes());
    buf[4..4 + data.len()].copy_from_slice(&data);

    // Erase and write
    let _ = flash.erase(FLASH_SECTOR_ADDR, SECTOR_SIZE).await;
    let result = flash.write(FLASH_SECTOR_ADDR, &buf[0..(4 + data.len())]).await;

    if result.is_ok() {
        info!("Config saved successfully to flash");
        true
    } else {
        error!("Failed to write to flash");
        false
    }
}
