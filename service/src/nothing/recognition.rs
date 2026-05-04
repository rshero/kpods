use bluer::Device;
use uuid::Uuid;

use crate::airpods::device::{DeviceKind, NothingModel};

pub const NOTHING_SPP_UUID: Uuid = Uuid::from_u128(0xaeac4a03_dff5_498f_843a_34487cf133eb);
pub const NOTHING_FASTPAIR_UUID: Uuid = Uuid::from_u128(0xdf21fe2c_2515_4fdb_8886_f12c4d67927c);

pub async fn identify(dev: &Device) -> Option<DeviceKind> {
   let name = match dev.name().await.ok().flatten() {
      Some(name) => Some(name),
      None => dev.alias().await.ok(),
   };

   let model = name
      .as_deref()
      .and_then(NothingModel::from_name)
      .unwrap_or(NothingModel::Generic);

   if let Ok(Some(uuids)) = dev.uuids().await
      && uuids
         .iter()
         .any(|uuid| matches!(*uuid, NOTHING_SPP_UUID | NOTHING_FASTPAIR_UUID))
   {
      return Some(DeviceKind::Nothing { model });
   }

   if name
      .as_deref()
      .is_some_and(|name| NothingModel::from_name(name).is_some())
   {
      return Some(DeviceKind::Nothing { model });
   }

   None
}
