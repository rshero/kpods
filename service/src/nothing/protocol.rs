use crate::{
   airpods::protocol::{BatteryInfo, BatteryState, BatteryStatus, NoiseControlMode},
   bluetooth::rfcomm::Packet,
};

const FRAME_START: u8 = 0x55;
const FRAME_KIND: u8 = 0x60;
const FRAME_VERSION: u8 = 0x01;
const HEADER_LEN: usize = 8;
const CRC_LEN: usize = 2;

pub const CMD_READ_BATTERY: u16 = 49159;
pub const CMD_READ_ANC: u16 = 49182;
pub const CMD_READ_FIRMWARE: u16 = 49218;
pub const CMD_READ_LISTENING_MODE: u16 = 49232;
pub const CMD_RING: u16 = 61442;
pub const CMD_SET_ANC: u16 = 61455;
pub const CMD_SET_LISTENING_MODE: u16 = 61469;

pub const EVT_BATTERY: u16 = 57345;
pub const EVT_BATTERY_ALT: u16 = 16391;
pub const EVT_ANC: u16 = 57347;
pub const EVT_ANC_ALT: u16 = 16414;
pub const EVT_LISTENING_MODE: u16 = 16464;
pub const EVT_EQ: u16 = 16415;
pub const EVT_FIRMWARE: u16 = 16450;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
   pub command: u16,
   pub operation_id: u8,
   pub payload: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Framer {
   buf: Vec<u8>,
}

impl Framer {
   pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
      self.buf.extend_from_slice(bytes);
      let mut frames = Vec::new();

      loop {
         let Some(start) = self.buf.iter().position(|byte| *byte == FRAME_START) else {
            self.buf.clear();
            break;
         };
         if start > 0 {
            self.buf.drain(..start);
         }
         if self.buf.len() < HEADER_LEN {
            break;
         }

         let payload_len = self.buf[5] as usize;
         let has_crc = self.buf[1] == FRAME_KIND && self.buf[2] == FRAME_VERSION;
         let total_len = HEADER_LEN + payload_len + if has_crc { CRC_LEN } else { 0 };
         if self.buf.len() < total_len {
            break;
         }

         let raw: Vec<u8> = self.buf.drain(..total_len).collect();
         if has_crc {
            let expected_crc = u16::from_le_bytes([raw[total_len - 2], raw[total_len - 1]]);
            let actual_crc = crc16(&raw[..total_len - CRC_LEN]);
            if expected_crc != actual_crc {
               continue;
            }
         }

         frames.push(Frame {
            command: u16::from_le_bytes([raw[3], raw[4]]),
            operation_id: raw[7],
            payload: raw[HEADER_LEN..HEADER_LEN + payload_len].to_vec(),
         });
      }

      frames
   }
}

pub fn build(command: u16, payload: &[u8], operation_id: u8) -> Packet {
   let payload_len = payload.len().min(u8::MAX as usize);
   let mut bytes = Vec::with_capacity(HEADER_LEN + payload_len + CRC_LEN);
   bytes.extend_from_slice(&[
      FRAME_START,
      FRAME_KIND,
      FRAME_VERSION,
      (command & 0xff) as u8,
      (command >> 8) as u8,
      payload_len as u8,
      0x00,
      operation_id,
   ]);
   bytes.extend_from_slice(&payload[..payload_len]);

   let crc = crc16(&bytes);
   bytes.extend_from_slice(&crc.to_le_bytes());
   Packet::from_slice(&bytes)
}

pub fn crc16(buffer: &[u8]) -> u16 {
   let mut crc = 0xffff;
   for byte in buffer {
      crc ^= u16::from(*byte);
      for _ in 0..8 {
         crc = if crc & 1 != 0 {
            (crc >> 1) ^ 0xa001
         } else {
            crc >> 1
         };
      }
   }
   crc
}

pub fn parse_battery(payload: &[u8]) -> Option<BatteryInfo> {
   let (&count, rest) = payload.split_first()?;
   let mut info = BatteryInfo::new();

   for chunk in rest.chunks_exact(2).take(count as usize) {
      let level = chunk[1] & 0x7f;
      let status = if chunk[1] & 0x80 != 0 {
         BatteryStatus::Charging
      } else {
         BatteryStatus::Normal
      };
      let state = BatteryState { level, status };

      match chunk[0] {
         0x02 => info.left = state,
         0x03 => info.right = state,
         0x04 => info.case = state,
         0x06 => info.headphone = state,
         _ => {},
      }
   }

   Some(info)
}

pub fn parse_firmware(payload: &[u8]) -> Option<String> {
   let firmware = std::str::from_utf8(payload)
      .ok()?
      .trim_end_matches('\0')
      .trim();
   if firmware.is_empty() {
      None
   } else {
      Some(firmware.to_string())
   }
}

pub fn parse_anc_level(payload: &[u8]) -> Option<u8> {
   let mode_status = *payload.get(1)?;
   if matches!(mode_status, 0x05 | 0x07) {
      return anc_status_to_level(mode_status);
   }

   let status = if payload.get(3) == Some(&0x02) {
      *payload.get(4)?
   } else {
      mode_status
   };

   anc_status_to_level(status)
}

fn anc_status_to_level(status: u8) -> Option<u8> {
   Some(match status {
      0x05 => 1,
      0x07 => 2,
      0x03 => 3,
      0x01 => 4,
      0x02 => 5,
      0x04 => 6,
      _ => return None,
   })
}

pub fn parse_eq_preset(payload: &[u8]) -> Option<u8> {
   payload.first().copied()
}

pub fn anc_level_to_mode(level: u8) -> Option<NoiseControlMode> {
   Some(match level {
      1 => NoiseControlMode::Off,
      2 => NoiseControlMode::Transparency,
      3..=5 => NoiseControlMode::Active,
      6 => NoiseControlMode::Adaptive,
      _ => return None,
   })
}

pub fn mode_to_anc_level(mode: NoiseControlMode) -> u8 {
   match mode {
      NoiseControlMode::Off => 1,
      NoiseControlMode::Transparency => 2,
      NoiseControlMode::Active => 4,
      NoiseControlMode::Adaptive => 6,
   }
}

pub fn anc_level_to_payload(level: u8) -> Option<[u8; 3]> {
   let status = match level {
      1 => 0x05,
      2 => 0x07,
      3 => 0x03,
      4 => 0x01,
      5 => 0x02,
      6 => 0x04,
      _ => return None,
   };
   Some([0x01, status, 0x00])
}

pub fn anc_level_to_strength_payload(level: u8) -> Option<[u8; 3]> {
   let status = match level {
      3 => 0x03,
      4 => 0x01,
      5 => 0x02,
      6 => 0x04,
      _ => return None,
   };
   Some([0x02, status, 0x00])
}

pub fn listening_mode_payload(preset: u8) -> [u8; 2] {
   [preset, 0x00]
}

pub fn ring_payload(enabled: bool) -> [u8; 2] {
   [0x02, u8::from(enabled)]
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn build_and_parse_roundtrip() {
      let packet = build(CMD_READ_BATTERY, &[1, 6, 80], 7);
      let mut framer = Framer::default();
      let frames = framer.push(&packet);
      assert_eq!(frames.len(), 1);
      assert_eq!(frames[0].command, CMD_READ_BATTERY);
      assert_eq!(frames[0].operation_id, 7);
      assert_eq!(frames[0].payload, [1, 6, 80]);
   }

   #[test]
   fn parse_b175_anc_reports() {
      assert_eq!(
         parse_anc_level(&[0x01, 0x07, 0x00, 0x02, 0x04, 0x00]),
         Some(2)
      );
      assert_eq!(
         parse_anc_level(&[0x01, 0x01, 0x00, 0x02, 0x01, 0x00]),
         Some(4)
      );
      assert_eq!(
         parse_anc_level(&[0x01, 0x02, 0x00, 0x02, 0x02, 0x00]),
         Some(5)
      );
      assert_eq!(
         parse_anc_level(&[0x01, 0x04, 0x00, 0x02, 0x04, 0x00]),
         Some(6)
      );
   }

   #[test]
   fn build_b175_anc_payloads() {
      assert_eq!(anc_level_to_payload(1), Some([0x01, 0x05, 0x00]));
      assert_eq!(anc_level_to_payload(2), Some([0x01, 0x07, 0x00]));
      assert_eq!(anc_level_to_payload(3), Some([0x01, 0x03, 0x00]));
      assert_eq!(anc_level_to_payload(4), Some([0x01, 0x01, 0x00]));
      assert_eq!(anc_level_to_payload(5), Some([0x01, 0x02, 0x00]));
      assert_eq!(anc_level_to_payload(6), Some([0x01, 0x04, 0x00]));
      assert_eq!(anc_level_to_strength_payload(3), Some([0x02, 0x03, 0x00]));
      assert_eq!(anc_level_to_strength_payload(4), Some([0x02, 0x01, 0x00]));
      assert_eq!(anc_level_to_strength_payload(5), Some([0x02, 0x02, 0x00]));
      assert_eq!(anc_level_to_strength_payload(6), Some([0x02, 0x04, 0x00]));
   }
}
