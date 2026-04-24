use crate::protocol::{DeviceConfig, LedEffect, PicoToHost};
use crate::usb::USB_TX_CHANNEL;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use sequential_storage::cache::NoCache;
use sequential_storage::map::{MapConfig, MapStorage, PostcardValue};

const FLASH_SIZE_BYTES: usize = 2 * 1024 * 1024;
const CONFIG_FLASH_RANGE: core::ops::Range<u32> = 0x001F_0000..0x0020_0000;
const CONFIG_KEY: u8 = 1;
const WORKBUF_SIZE: usize = 128;

pub type ConfigFlash = Flash<'static, FLASH, Async, FLASH_SIZE_BYTES>;
type ConfigStorage = MapStorage<u8, ConfigFlash, NoCache>;

pub enum ConfigCommand {
    SaveLedEffect(LedEffect),
    SetConfig(DeviceConfig),
    SendConfigToHost,
}

pub static CONFIG_COMMAND_CHANNEL: Channel<ThreadModeRawMutex, ConfigCommand, 8> = Channel::new();

impl<'a> PostcardValue<'a> for DeviceConfig {}

pub fn new_storage(flash: ConfigFlash) -> ConfigStorage {
    MapStorage::new(
        flash,
        const { MapConfig::new(CONFIG_FLASH_RANGE) },
        NoCache::new(),
    )
}

pub async fn load_config(storage: &mut ConfigStorage) -> DeviceConfig {
    let mut workbuf = [0u8; WORKBUF_SIZE];

    match storage
        .fetch_item::<DeviceConfig>(&mut workbuf, &CONFIG_KEY)
        .await
    {
        Ok(Some(config)) => config,
        Ok(None) => {
            let default_config = DeviceConfig::default();
            let _ = storage
                .store_item(&mut workbuf, &CONFIG_KEY, &default_config)
                .await;
            default_config
        }
        Err(_) => DeviceConfig::default(),
    }
}

#[embassy_executor::task]
pub async fn config_task(mut storage: ConfigStorage, mut current_config: DeviceConfig) {
    let mut workbuf = [0u8; WORKBUF_SIZE];

    loop {
        match CONFIG_COMMAND_CHANNEL.receive().await {
            ConfigCommand::SaveLedEffect(effect) => {
                current_config.led_effect = effect;
                let saved = storage
                    .store_item(&mut workbuf, &CONFIG_KEY, &current_config)
                    .await
                    .is_ok();
                let _ = USB_TX_CHANNEL.try_send(if saved {
                    PicoToHost::ConfigSaved
                } else {
                    PicoToHost::ConfigSaveFailed
                });
            }
            ConfigCommand::SetConfig(new_config) => {
                current_config = new_config;
                let saved = storage
                    .store_item(&mut workbuf, &CONFIG_KEY, &current_config)
                    .await
                    .is_ok();
                let _ = USB_TX_CHANNEL.try_send(if saved {
                    PicoToHost::ConfigSaved
                } else {
                    PicoToHost::ConfigSaveFailed
                });
            }
            ConfigCommand::SendConfigToHost => {
                let _ = USB_TX_CHANNEL.try_send(PicoToHost::Config {
                    config: current_config,
                });
            }
        }
    }
}
