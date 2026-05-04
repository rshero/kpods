use std::{collections::HashMap, fmt, str::FromStr};

use bluer::Address;
use log::info;
use zbus::{fdo, interface, object_server::SignalEmitter, zvariant};

use crate::{
   airpods::protocol::{FeatureId, NoiseControlMode},
   bluetooth::manager::BluetoothManager,
   media_control,
};

pub struct AirPodsService {
   bluetooth_manager: BluetoothManager,
}

impl AirPodsService {
   pub const fn new(bluetooth_manager: BluetoothManager) -> Self {
      Self { bluetooth_manager }
   }
}

fn to_arg_error<T: fmt::Display>(e: T) -> fdo::Error {
   fdo::Error::InvalidArgs(e.to_string())
}

fn get_u8_param(
   params: &HashMap<String, zvariant::Value<'_>>,
   name: &'static str,
) -> fdo::Result<u8> {
   let value = params
      .get(name)
      .ok_or_else(|| to_arg_error(format_args!("Missing '{name}' parameter")))?;

   if let Ok(value) = value.downcast_ref::<u8>() {
      return Ok(value);
   }
   if let Ok(value) = value.downcast_ref::<u16>() {
      return u8::try_from(value).map_err(to_arg_error);
   }
   if let Ok(value) = value.downcast_ref::<u32>() {
      return u8::try_from(value).map_err(to_arg_error);
   }
   if let Ok(value) = value.downcast_ref::<i16>() {
      return u8::try_from(value).map_err(to_arg_error);
   }
   if let Ok(value) = value.downcast_ref::<i32>() {
      return u8::try_from(value).map_err(to_arg_error);
   }
   if let Ok(value) = value.downcast_ref::<i64>() {
      return u8::try_from(value).map_err(to_arg_error);
   }

   Err(to_arg_error(format_args!(
      "Invalid '{name}' parameter: expected integer"
   )))
}

#[interface(name = "org.kairpods.manager")]
impl AirPodsService {
   async fn get_devices(&self) -> fdo::Result<String> {
      let states: Vec<serde_json::Value> = self
         .bluetooth_manager
         .all_devices()
         .await
         .into_iter()
         .map(|d| d.to_json())
         .collect();
      Ok(serde_json::to_string(&states).unwrap())
   }

   async fn get_device(&self, address: String) -> fdo::Result<String> {
      let addr = Address::from_str(&address).map_err(to_arg_error)?;
      let dev = self.bluetooth_manager.get_device(addr).await?;
      Ok(dev.to_json().to_string())
   }

   async fn passthrough(&self, address: String, packet: String) -> fdo::Result<bool> {
      let addr = Address::from_str(&address).map_err(to_arg_error)?;
      let dev = self.bluetooth_manager.get_device(addr).await?;
      let packet = hex::decode(packet).map_err(to_arg_error)?;
      dev.passthrough(&packet).await?;
      Ok(true)
   }

   async fn send_command(
      &self,
      address: String,
      action: String,
      params: HashMap<String, zvariant::Value<'_>>,
      #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
   ) -> fdo::Result<bool> {
      let addr = Address::from_str(&address).map_err(to_arg_error)?;

      let dev = self.bluetooth_manager.get_device(addr).await?;

      match action.as_str() {
         "set_noise_mode" => {
            let mode_str = params
               .get("value")
               .ok_or_else(|| to_arg_error("Missing 'value' parameter"))?
               .downcast_ref::<String>()
               .map_err(|e| to_arg_error(format_args!("Invalid 'value' parameter: {e}")))?;

            let mode: NoiseControlMode = mode_str
               .parse()
               .map_err(|_| to_arg_error(format_args!("Invalid noise mode: {mode_str:?}")))?;

            dev.set_noise_control(mode).await?;

            info!("Set noise mode to {mode} for {address}");

            // Emit property change immediately so UI updates
            self.devices_changed(&emitter).await?;
         },

         "set_feature" => {
            let feature_str = params
               .get("feature")
               .ok_or_else(|| to_arg_error("Missing 'feature' parameter"))?
               .downcast_ref::<String>()
               .map_err(|e| to_arg_error(format_args!("Invalid 'feature' parameter: {e}")))?;

            let feature: FeatureId = feature_str
               .parse()
               .map_err(|_| to_arg_error(format_args!("Invalid feature: {feature_str:?}")))?;

            let enabled = params
               .get("enabled")
               .ok_or_else(|| to_arg_error("Missing 'enabled' parameter"))?
               .downcast_ref::<bool>()
               .map_err(|e| {
                  to_arg_error(format_args!(
                     "Invalid 'enabled' value for feature: {feature}: {e}"
                  ))
               })?;

            dev.set_feature(feature, enabled).await?;
            info!("Set feature {feature} to {enabled} for {address}");

            // Emit property change immediately so UI updates
            self.devices_changed(&emitter).await?;
         },

         "set_nothing_anc_level" => {
            let level = get_u8_param(&params, "level")?;
            dev.set_nothing_anc_level(level).await?;
            info!("Set Nothing/CMF ANC level to {level} for {address}");
            self.devices_changed(&emitter).await?;
         },

         "set_nothing_eq_preset" => {
            let preset = get_u8_param(&params, "preset")?;
            dev.set_nothing_eq_preset(preset).await?;
            info!("Set Nothing/CMF EQ preset to {preset} for {address}");
            self.devices_changed(&emitter).await?;
         },

         "set_nothing_ring" => {
            let enabled = params
               .get("enabled")
               .ok_or_else(|| to_arg_error("Missing 'enabled' parameter"))?
               .downcast_ref::<bool>()
               .map_err(|e| to_arg_error(format_args!("Invalid 'enabled' parameter: {e}")))?;

            dev.set_nothing_ring(enabled).await?;
            info!("Set Nothing/CMF ring to {enabled} for {address}");
            self.devices_changed(&emitter).await?;
         },

         _ => {
            return Err(to_arg_error(format_args!("Unknown action: {action}")));
         },
      }

      Ok(true)
   }

   async fn connect_device(&self, address: String) -> fdo::Result<bool> {
      let addr = Address::from_str(&address).map_err(to_arg_error)?;
      self.bluetooth_manager.establish_aap(addr).await?;
      Ok(true)
   }

   async fn disconnect_device(&self, address: String) -> fdo::Result<bool> {
      let addr = Address::from_str(&address).map_err(to_arg_error)?;
      self.bluetooth_manager.disconnect_aap(addr).await?;
      Ok(true)
   }

   async fn set_auto_play_pause(&self, enabled: bool) -> fdo::Result<bool> {
      media_control::set_enabled(enabled);
      info!("Auto play/pause set to {enabled}");
      Ok(true)
   }

   async fn get_auto_play_pause(&self) -> fdo::Result<bool> {
      Ok(media_control::is_enabled())
   }

   // Signals
   #[zbus(signal)]
   pub async fn device_connected(emitter: &SignalEmitter<'_>, address: &str) -> zbus::Result<()>;

   #[zbus(signal)]
   pub async fn device_disconnected(emitter: &SignalEmitter<'_>, address: &str)
   -> zbus::Result<()>;

   #[zbus(signal)]
   pub async fn battery_updated(
      emitter: &SignalEmitter<'_>,
      address: &str,
      battery: &str,
   ) -> zbus::Result<()>;

   #[zbus(signal)]
   pub async fn noise_control_changed(
      emitter: &SignalEmitter<'_>,
      address: &str,
      mode: &str,
   ) -> zbus::Result<()>;

   #[zbus(signal)]
   pub async fn ear_detection_changed(
      emitter: &SignalEmitter<'_>,
      address: &str,
      ear_detection: &str,
   ) -> zbus::Result<()>;

   #[zbus(signal)]
   pub async fn device_name_changed(
      emitter: &SignalEmitter<'_>,
      address: &str,
      name: &str,
   ) -> zbus::Result<()>;

   #[zbus(signal)]
   pub async fn device_error(emitter: &SignalEmitter<'_>, address: &str) -> zbus::Result<()>;

   // Properties for polling-free updates
   #[zbus(property)]
   async fn devices(&self) -> String {
      self.get_devices().await.unwrap_or_default()
   }

   #[zbus(property)]
   async fn connected_count(&self) -> u32 {
      self.bluetooth_manager.count_devices().await
   }
}
