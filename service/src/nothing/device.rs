use std::{
   sync::{
      Arc, Weak,
      atomic::{AtomicBool, AtomicU8, Ordering},
   },
   time::Duration,
};

use crossbeam::atomic::AtomicCell;
use log::{debug, info};
use parking_lot::Mutex;
use serde_json::json;
use tokio::{
   task::{JoinHandle, JoinSet},
   time,
};

use crate::{
   airpods::{
      device::{AirPods, DeviceKind, NothingModel, UpdateOp, WeakAirPods},
      protocol::NoiseControlMode,
   },
   bluetooth::rfcomm::{self, RfcommReceiver, RfcommSender},
   error::{AirPodsError, Result},
   event::{AirPodsEvent, EventSender},
   nothing::protocol,
};

const RFCOMM_CHANNELS: &[u8] = &[
   1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
   27, 28, 29, 30,
];
const CMF_HEADPHONE_PRO_CHANNELS: &[u8] = &[17, 28, 12, 13];

#[derive(Debug, Default)]
pub struct NothingState {
   firmware: Mutex<Option<String>>,
   anc_level: AtomicCell<Option<u8>>,
   eq_preset: AtomicCell<Option<u8>>,
   ringing: AtomicBool,
   operation_id: AtomicU8,
}

impl NothingState {
   pub fn firmware(&self) -> Option<String> {
      self.firmware.lock().clone()
   }

   pub fn update_firmware(&self, firmware: String) -> UpdateOp<String> {
      let mut lock = self.firmware.lock();
      if lock.as_deref() == Some(firmware.as_str()) {
         return UpdateOp::Noop;
      }
      let prev = lock.replace(firmware);
      match prev {
         Some(prev) => UpdateOp::Updated(prev),
         None => UpdateOp::Inserted,
      }
   }

   pub fn anc_level(&self) -> Option<u8> {
      self.anc_level.load()
   }

   pub fn update_anc_level(&self, level: u8) -> UpdateOp<u8> {
      UpdateOp::apply_atomic(&self.anc_level, Some(level))
   }

   pub fn eq_preset(&self) -> Option<u8> {
      self.eq_preset.load()
   }

   pub fn update_eq_preset(&self, preset: u8) -> UpdateOp<u8> {
      UpdateOp::apply_atomic(&self.eq_preset, Some(preset))
   }

   pub fn ringing(&self) -> bool {
      self.ringing.load(Ordering::Relaxed)
   }

   pub fn set_ringing(&self, enabled: bool) {
      self.ringing.store(enabled, Ordering::Relaxed);
   }

   fn next_operation_id(&self) -> u8 {
      let next = self
         .operation_id
         .fetch_add(1, Ordering::Relaxed)
         .wrapping_add(1);
      if next == 0 {
         self.operation_id.store(1, Ordering::Relaxed);
         1
      } else {
         next
      }
   }

   pub fn to_json(&self) -> serde_json::Value {
      json!({
         "firmware": self.firmware(),
         "anc_level": self.anc_level(),
         "eq_preset": self.eq_preset(),
         "ringing": self.ringing(),
         "supports": {
            "battery": true,
            "anc": true,
            "firmware": true,
            "eq": true,
            "ring": true,
            "gestures": false
         }
      })
   }
}

#[derive(Debug)]
pub struct NothingConnectionState {
   pub sender: RfcommSender,
   pub jset: JoinSet<()>,
}

impl Drop for NothingConnectionState {
   fn drop(&mut self) {
      self.jset.abort_all();
   }
}

pub async fn connect(
   device: &AirPods,
   state: Arc<NothingState>,
   event_tx: &EventSender,
) -> Result<(NothingConnectionState, JoinHandle<Option<AirPodsError>>)> {
   info!("Connecting to Nothing/CMF device at {}", device.address());
   let mut last_err = None;
   let mut saw_fastpair = false;
   let channels = channels_for(device.kind());

   for pass in 0..2 {
      if pass == 1 && !saw_fastpair {
         break;
      }

      for channel in channels {
         let mut jset = JoinSet::new();
         match rfcomm::connect(&mut jset, device.address(), &[*channel]).await {
            Ok((mut receiver, sender)) => {
               let probe =
                  protocol::build(protocol::CMD_READ_BATTERY, &[], state.next_operation_id());
               if let Err(e) = sender.send(&probe).await {
                  last_err = Some(e);
                  continue;
               }

               match time::timeout(Duration::from_millis(1500), receiver.recv()).await {
                  Ok(Ok(first_packet)) => {
                     let mut framer = protocol::Framer::default();
                     let frames = framer.push(&first_packet);
                     if frames.is_empty() {
                        if first_packet.starts_with(&[0x03, 0x01]) {
                           saw_fastpair = true;
                           debug!(
                              "Fast Pair response on RFCOMM channel {}: {}",
                              channel,
                              hex::encode(&first_packet)
                           );
                        } else {
                           debug!(
                              "Non-control response on RFCOMM channel {}: {}",
                              channel,
                              hex::encode(&first_packet)
                           );
                        }
                        continue;
                     }

                     info!(
                        "Nothing/CMF protocol responded on RFCOMM channel {}",
                        channel
                     );
                     request_initial_state(&state, &sender).await;

                     for frame in frames {
                        process_frame(device, &state, frame, event_tx);
                     }

                     let handle = start_packet_processor(
                        WeakAirPods::new(device),
                        Arc::downgrade(&state),
                        receiver,
                        framer,
                        event_tx.clone(),
                     );

                     return Ok((NothingConnectionState { sender, jset }, handle));
                  },
                  Ok(Err(e)) => {
                     last_err = Some(e);
                  },
                  Err(_) => {
                     debug!(
                        "No Nothing/CMF protocol response on RFCOMM channel {}",
                        channel
                     );
                     last_err = Some(AirPodsError::RequestTimeout);
                  },
               }
            },
            Err(e) => {
               last_err = Some(e);
            },
         }
      }
   }

   Err(last_err.unwrap_or(AirPodsError::RequestTimeout))
}

fn channels_for(kind: DeviceKind) -> &'static [u8] {
   match kind {
      DeviceKind::Nothing {
         model: NothingModel::CmfHeadphonePro,
      } => CMF_HEADPHONE_PRO_CHANNELS,
      _ => RFCOMM_CHANNELS,
   }
}

pub async fn request_initial_state(state: &NothingState, sender: &RfcommSender) {
   for command in [
      protocol::CMD_READ_BATTERY,
      protocol::CMD_READ_FIRMWARE,
      protocol::CMD_READ_ANC,
      protocol::CMD_READ_LISTENING_MODE,
   ] {
      let packet = protocol::build(command, &[], state.next_operation_id());
      let _ = sender.send(&packet).await;
      time::sleep(Duration::from_millis(100)).await;
   }
}

pub async fn set_noise_control(
   state: &NothingState,
   sender: &RfcommSender,
   mode: NoiseControlMode,
) -> Result<()> {
   let level = protocol::mode_to_anc_level(mode);
   set_anc_level(state, sender, level).await
}

pub async fn set_anc_level(state: &NothingState, sender: &RfcommSender, level: u8) -> Result<()> {
   let payload = protocol::anc_level_to_payload(level)
      .ok_or_else(|| AirPodsError::FeatureNotSupported(format!("ANC level {level}")))?;
   let packet = protocol::build(protocol::CMD_SET_ANC, &payload, state.next_operation_id());
   sender.send(&packet).await?;
   state.update_anc_level(level);
   Ok(())
}

pub async fn set_eq_preset(state: &NothingState, sender: &RfcommSender, preset: u8) -> Result<()> {
   let packet = protocol::build(
      protocol::CMD_SET_LISTENING_MODE,
      &protocol::listening_mode_payload(preset),
      state.next_operation_id(),
   );
   sender.send(&packet).await?;
   state.update_eq_preset(preset);
   Ok(())
}

pub async fn set_ring(state: &NothingState, sender: &RfcommSender, enabled: bool) -> Result<()> {
   let packet = protocol::build(
      protocol::CMD_RING,
      &protocol::ring_payload(enabled),
      state.next_operation_id(),
   );
   sender.send(&packet).await?;
   state.set_ringing(enabled);
   Ok(())
}

fn start_packet_processor(
   device: WeakAirPods,
   state: Weak<NothingState>,
   mut rx: RfcommReceiver,
   mut framer: protocol::Framer,
   event_tx: EventSender,
) -> JoinHandle<Option<AirPodsError>> {
   tokio::spawn(async move {
      loop {
         match rx.recv().await {
            Ok(packet) => {
               for frame in framer.push(&packet) {
                  let Some(device) = device.upgrade() else {
                     return None;
                  };
                  let Some(state) = state.upgrade() else {
                     return None;
                  };
                  process_frame(&device, &state, frame, &event_tx);
               }
            },
            Err(e) => {
               if let Some(device) = device.upgrade() {
                  device.notify_transport_disconnected(&event_tx).await;
               }
               return Some(e);
            },
         }
      }
   })
}

fn process_frame(
   device: &AirPods,
   state: &NothingState,
   frame: protocol::Frame,
   event_tx: &EventSender,
) {
   debug!(
      "{}: Nothing frame command={} op={} payload={}",
      device.address(),
      frame.command,
      frame.operation_id,
      hex::encode(&frame.payload)
   );

   match frame.command {
      protocol::EVT_BATTERY | protocol::EVT_BATTERY_ALT => {
         if let Some(battery) = protocol::parse_battery(&frame.payload)
            && device.update_battery_info(battery).is_updated()
         {
            event_tx.emit(device, AirPodsEvent::BatteryUpdated(battery));
         }
      },
      protocol::EVT_ANC | protocol::EVT_ANC_ALT => {
         if let Some(level) = protocol::parse_anc_level(&frame.payload) {
            let level_changed = state.update_anc_level(level).is_updated();
            if let Some(mode) = protocol::anc_level_to_mode(level)
               && device.update_noise_mode(mode).is_updated()
            {
               event_tx.emit(device, AirPodsEvent::NoiseControlChanged(mode));
            } else if level_changed {
               event_tx.emit(device, AirPodsEvent::DeviceInfoChanged);
            }
         }
      },
      protocol::EVT_FIRMWARE => {
         if let Some(firmware) = protocol::parse_firmware(&frame.payload)
            && state.update_firmware(firmware).is_updated()
         {
            event_tx.emit(device, AirPodsEvent::DeviceInfoChanged);
         }
      },
      protocol::EVT_EQ | protocol::EVT_LISTENING_MODE => {
         if let Some(preset) = protocol::parse_eq_preset(&frame.payload)
            && state.update_eq_preset(preset).is_updated()
         {
            event_tx.emit(device, AirPodsEvent::DeviceInfoChanged);
         }
      },
      _ => {},
   }
}
