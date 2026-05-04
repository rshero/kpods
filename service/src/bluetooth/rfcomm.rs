//! RFCOMM socket implementation for Nothing/CMF serial control channels.

use std::{
   io::{self, Read, Write},
   mem,
   os::fd::{FromRawFd, RawFd},
   time::Duration,
};

use bluer::Address;
use log::{debug, warn};
use smallvec::SmallVec;
use tokio::{
   sync::{mpsc, oneshot},
   task::JoinSet,
   time,
};

use crate::error::{AirPodsError, Result};

pub type Packet = SmallVec<[u8; 64]>;

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_RFCOMM: libc::c_int = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const READ_BUF_SIZE: usize = 512;

#[repr(C)]
#[derive(Clone, Copy)]
struct BdAddr {
   b: [u8; 6],
}

#[repr(C)]
struct SockAddrRc {
   rc_family: libc::sa_family_t,
   rc_bdaddr: BdAddr,
   rc_channel: u8,
}

enum Command {
   Send {
      data: Packet,
      then: oneshot::Sender<Result<()>>,
   },
}

#[derive(Debug)]
pub struct RfcommReceiver {
   rx: mpsc::Receiver<Result<Packet>>,
}

impl RfcommReceiver {
   pub async fn recv(&mut self) -> Result<Packet> {
      self.rx.recv().await.ok_or(AirPodsError::ConnectionClosed)?
   }
}

#[derive(Debug, Clone)]
pub struct RfcommSender {
   tx: mpsc::Sender<Command>,
}

impl RfcommSender {
   pub fn is_connected(&self) -> bool {
      !self.tx.is_closed()
   }

   pub async fn send(&self, data: &[u8]) -> Result<()> {
      if !self.is_connected() {
         return Err(AirPodsError::ConnectionClosed);
      }

      let (tx, rx) = oneshot::channel();
      self
         .tx
         .send(Command::Send {
            data: Packet::from_slice(data),
            then: tx,
         })
         .await
         .map_err(|_| AirPodsError::ConnectionClosed)?;

      time::timeout(WRITE_TIMEOUT, rx)
         .await
         .map_err(|_| AirPodsError::RequestTimeout)?
         .map_err(|_| AirPodsError::ConnectionClosed)?
   }
}

pub async fn connect(
   jset: &mut JoinSet<()>,
   address: Address,
   channels: &[u8],
) -> Result<(RfcommReceiver, RfcommSender)> {
   let channels = if channels.is_empty() {
      &[1][..]
   } else {
      channels
   };
   let mut last_err = None;

   for channel in channels {
      debug!("Creating RFCOMM socket for {address} channel {channel}");
      match time::timeout(
         CONNECT_TIMEOUT,
         tokio::task::spawn_blocking({
            let address = address.to_string();
            let channel = *channel;
            move || connect_blocking(&address, channel)
         }),
      )
      .await
      {
         Ok(Ok(Ok(file))) => {
            let (cmd_tx, cmd_rx) = mpsc::channel(128);
            let (in_tx, in_rx) = mpsc::channel(128);
            let reader = file.try_clone()?;

            jset.spawn_blocking(move || recv_thread(in_tx, reader));
            jset.spawn_blocking(move || send_thread(cmd_rx, file));

            return Ok((RfcommReceiver { rx: in_rx }, RfcommSender { tx: cmd_tx }));
         },
         Ok(Ok(Err(e))) => {
            debug!("RFCOMM connect failed on channel {channel}: {e}");
            last_err = Some(e);
         },
         Ok(Err(e)) => {
            return Err(AirPodsError::ActorPanicked(e));
         },
         Err(_) => {
            warn!("RFCOMM connect to {address} channel {channel} timed out");
            last_err = Some(io::Error::new(
               io::ErrorKind::TimedOut,
               "RFCOMM connect timed out",
            ));
         },
      }
   }

   Err(
      last_err
         .unwrap_or_else(|| io::Error::other("RFCOMM connect failed"))
         .into(),
   )
}

fn connect_blocking(address: &str, channel: u8) -> io::Result<std::fs::File> {
   let fd = unsafe {
      libc::socket(
         AF_BLUETOOTH,
         libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
         BTPROTO_RFCOMM,
      )
   };
   if fd < 0 {
      return Err(io::Error::last_os_error());
   }

   if let Err(e) = set_nonblocking(fd, true) {
      unsafe {
         libc::close(fd);
      }
      return Err(e);
   }

   let addr = SockAddrRc {
      rc_family: AF_BLUETOOTH as libc::sa_family_t,
      rc_bdaddr: parse_bdaddr(address)?,
      rc_channel: channel,
   };

   let rc = unsafe {
      libc::connect(
         fd,
         (&addr as *const SockAddrRc).cast::<libc::sockaddr>(),
         mem::size_of::<SockAddrRc>() as libc::socklen_t,
      )
   };

   if rc < 0 {
      let err = io::Error::last_os_error();
      if err.raw_os_error() != Some(libc::EINPROGRESS) {
         unsafe {
            libc::close(fd);
         }
         return Err(err);
      }
      wait_connected(fd)?;
   }

   set_nonblocking(fd, false)?;
   debug!("RFCOMM connected to {address} channel {channel}");

   Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

fn parse_bdaddr(address: &str) -> io::Result<BdAddr> {
   let bytes: Vec<u8> = address
      .split(':')
      .map(|part| u8::from_str_radix(part, 16))
      .collect::<std::result::Result<_, _>>()
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid Bluetooth address"))?;

   if bytes.len() != 6 {
      return Err(io::Error::new(
         io::ErrorKind::InvalidInput,
         "invalid Bluetooth address length",
      ));
   }

   let mut bdaddr = [0; 6];
   for (dst, src) in bdaddr.iter_mut().zip(bytes.into_iter().rev()) {
      *dst = src;
   }
   Ok(BdAddr { b: bdaddr })
}

fn set_nonblocking(fd: RawFd, enabled: bool) -> io::Result<()> {
   let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
   if flags < 0 {
      return Err(io::Error::last_os_error());
   }
   let flags = if enabled {
      flags | libc::O_NONBLOCK
   } else {
      flags & !libc::O_NONBLOCK
   };
   if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } < 0 {
      return Err(io::Error::last_os_error());
   }
   Ok(())
}

fn wait_connected(fd: RawFd) -> io::Result<()> {
   let mut pfd = libc::pollfd {
      fd,
      events: libc::POLLOUT,
      revents: 0,
   };

   let rc = unsafe { libc::poll(&mut pfd, 1, CONNECT_TIMEOUT.as_millis() as libc::c_int) };
   if rc == 0 {
      return Err(io::Error::new(
         io::ErrorKind::TimedOut,
         "RFCOMM connect timed out",
      ));
   }
   if rc < 0 {
      return Err(io::Error::last_os_error());
   }

   let mut err: libc::c_int = 0;
   let mut len = mem::size_of_val(&err) as libc::socklen_t;
   if unsafe {
      libc::getsockopt(
         fd,
         libc::SOL_SOCKET,
         libc::SO_ERROR,
         (&mut err as *mut libc::c_int).cast(),
         &mut len,
      )
   } < 0
   {
      return Err(io::Error::last_os_error());
   }

   if err != 0 {
      Err(io::Error::from_raw_os_error(err))
   } else {
      Ok(())
   }
}

fn recv_thread(tx: mpsc::Sender<Result<Packet>>, mut file: std::fs::File) {
   let mut buf = [0u8; READ_BUF_SIZE];
   loop {
      match file.read(&mut buf) {
         Ok(0) => {
            let _ = tx.blocking_send(Err(AirPodsError::ConnectionLost));
            return;
         },
         Ok(n) => {
            debug!("← RFCOMM: {}", hex::encode(&buf[..n]));
            if tx.blocking_send(Ok(Packet::from_slice(&buf[..n]))).is_err() {
               return;
            }
         },
         Err(e) if e.kind() == io::ErrorKind::Interrupted => {},
         Err(e) => {
            let _ = tx.blocking_send(Err(AirPodsError::Io(e)));
            return;
         },
      }
   }
}

fn send_thread(mut rx: mpsc::Receiver<Command>, mut file: std::fs::File) {
   while let Some(cmd) = rx.blocking_recv() {
      match cmd {
         Command::Send { data, then } => {
            debug!("→ RFCOMM: {}", hex::encode(&data));
            let result = file.write_all(&data).map_err(AirPodsError::Io);
            let _ = then.send(result);
         },
      }
   }
}
