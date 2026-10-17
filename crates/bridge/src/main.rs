//! rsrpc-bridge: Wine and Proton named pipe bridge for rsRPC.
//!
//! Creates a Win32 named pipe (`\\.\pipe\discord-ipc-0`) inside Wine/Proton and
//! transparently forwards packets bidirectionally to the host Linux Unix domain
//! socket (`/run/user/<uid>/discord-ipc-0`).

#![allow(
  clippy::assertions_on_constants,
  clippy::unreachable,
  clippy::panic,
  clippy::unwrap_used,
  clippy::expect_used
)]

#[cfg(not(windows))]
fn main() {
  eprintln!("rsrpc-bridge is a Windows executable designed to run inside Wine/Proton.");
  eprintln!("To build for Wine/Proton: cargo build -p rsrpc-bridge --target x86_64-pc-windows-gnu");
}

#[cfg(windows)]
mod imp {
  use std::ffi::OsStr;
  use std::os::windows::ffi::OsStrExt;
  use std::path::Path;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::Arc;
  use std::thread;

  use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, FALSE, HANDLE,
    INVALID_HANDLE_VALUE, TRUE,
  };
  use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
  use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT,
  };

  const BUF_SIZE: usize = 65536;

  #[repr(C)]
  struct SockAddrUn {
    sun_family: u16,
    sun_path: [u8; 108],
  }

  struct LinuxSocket(i32);

  impl LinuxSocket {
    #[cfg(target_arch = "x86_64")]
    unsafe fn syscall(num: usize, a1: usize, a2: usize, a3: usize) -> isize {
      let ret: isize;
      core::arch::asm!(
        "syscall",
        inlateout("rax") num as isize => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
      );
      ret
    }

    #[cfg(target_arch = "x86")]
    unsafe fn syscall(num: usize, a1: usize, a2: usize, a3: usize) -> isize {
      let ret: isize;
      core::arch::asm!(
        "push ebx",
        "mov ebx, edx",
        "int 0x80",
        "pop ebx",
        inlateout("eax") num as isize => ret,
        in("edx") a1,
        in("ecx") a2,
        in("esi") a3,
        options(preserves_flags)
      );
      ret
    }

    fn open_unix(path: &str) -> Option<Self> {
      let path_bytes = path.as_bytes();
      if path_bytes.len() >= 108 {
        return None;
      }

      // SYS_socket = 41 on x86_64, AF_UNIX = 1, SOCK_STREAM = 1, protocol = 0
      #[cfg(target_arch = "x86_64")]
      let fd = unsafe { Self::syscall(41, 1, 1, 0) };
      #[cfg(target_arch = "x86")]
      let fd = {
        // SYS_socketcall = 102, SYS_SOCKET = 1
        let args = [1usize, 1, 0];
        unsafe { Self::syscall(102, 1, args.as_ptr() as usize, 0) }
      };

      if fd < 0 {
        return None;
      }
      let fd = fd as i32;

      let mut addr = SockAddrUn {
        sun_family: 1, // AF_UNIX
        sun_path: [0; 108],
      };
      addr.sun_path[..path_bytes.len()].copy_from_slice(path_bytes);

      // SYS_connect = 42 on x86_64
      #[cfg(target_arch = "x86_64")]
      let res = unsafe { Self::syscall(42, fd as usize, &addr as *const _ as usize, 110) };
      #[cfg(target_arch = "x86")]
      let res = {
        let args = [fd as usize, &addr as *const _ as usize, 110];
        unsafe { Self::syscall(102, 3, args.as_ptr() as usize, 0) }
      };

      if res == 0 {
        Some(Self(fd))
      } else {
        unsafe {
          #[cfg(target_arch = "x86_64")]
          Self::syscall(3, fd as usize, 0, 0); // SYS_close = 3
          #[cfg(target_arch = "x86")]
          Self::syscall(6, fd as usize, 0, 0); // SYS_close = 6
        }
        None
      }
    }

    fn read(&self, buf: &mut [u8]) -> isize {
      #[cfg(target_arch = "x86_64")]
      unsafe {
        Self::syscall(0, self.0 as usize, buf.as_mut_ptr() as usize, buf.len())
      }
      #[cfg(target_arch = "x86")]
      unsafe {
        Self::syscall(3, self.0 as usize, buf.as_mut_ptr() as usize, buf.len())
      }
    }

    fn write_all(&self, mut buf: &[u8]) -> bool {
      while !buf.is_empty() {
        #[cfg(target_arch = "x86_64")]
        let n = unsafe { Self::syscall(1, self.0 as usize, buf.as_ptr() as usize, buf.len()) };
        #[cfg(target_arch = "x86")]
        let n = unsafe { Self::syscall(4, self.0 as usize, buf.as_ptr() as usize, buf.len()) };

        if n <= 0 {
          return false;
        }
        buf = &buf[n as usize..];
      }
      true
    }

    fn close(&self) {
      unsafe {
        #[cfg(target_arch = "x86_64")]
        Self::syscall(3, self.0 as usize, 0, 0);
        #[cfg(target_arch = "x86")]
        Self::syscall(6, self.0 as usize, 0, 0);
      }
    }
  }

  impl Drop for LinuxSocket {
    fn drop(&mut self) {
      self.close();
    }
  }

  fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
  }

  fn find_linux_socket() -> Option<LinuxSocket> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();
    let base_dirs = [runtime_dir.as_deref(), Some("/run/user/1000"), Some("/tmp")];

    for base in base_dirs.into_iter().flatten() {
      for i in 0..10 {
        let candidates = [
          format!("{base}/discord-ipc-{i}"),
          format!("{base}/app/com.discordapp.Discord/discord-ipc-{i}"),
          format!("{base}/.flatpak/dev.vencord.Vesktop/xdg-run/discord-ipc-{i}"),
          format!("{base}/snap.discord/discord-ipc-{i}"),
          format!("{base}/snap.discord-canary/discord-ipc-{i}"),
        ];
        for candidate in &candidates {
          if Path::new(candidate).exists() {
            if let Some(sock) = LinuxSocket::open_unix(candidate) {
              println!("[rsrpc-bridge] Connected to Linux socket: {candidate}");
              return Some(sock);
            }
          }
        }
      }
    }
    None
  }

  pub(crate) fn run() {
    let pipe_name = to_wide(r"\\.\pipe\discord-ipc-0");
    println!("[rsrpc-bridge] Starting Wine/Proton named pipe bridge...");

    loop {
      let handle: HANDLE = unsafe {
        CreateNamedPipeW(
          pipe_name.as_ptr(),
          PIPE_ACCESS_DUPLEX,
          PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
          1,
          BUF_SIZE as u32,
          BUF_SIZE as u32,
          0,
          core::ptr::null_mut(),
        )
      };

      if handle == INVALID_HANDLE_VALUE {
        let err = unsafe { GetLastError() };
        eprintln!("[rsrpc-bridge] CreateNamedPipeW failed: {err}");
        thread::sleep(std::time::Duration::from_secs(1));
        continue;
      }

      println!("[rsrpc-bridge] Awaiting game connection on \\\\.\\pipe\\discord-ipc-0");
      let connected = unsafe { ConnectNamedPipe(handle, core::ptr::null_mut()) };
      let is_connected = connected == TRUE || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;

      if !is_connected {
        unsafe { CloseHandle(handle) };
        continue;
      }

      println!("[rsrpc-bridge] Game client connected to named pipe");
      let linux_sock = match find_linux_socket() {
        Some(sock) => sock,
        None => {
          eprintln!("[rsrpc-bridge] Failed to connect to host Linux Discord socket");
          unsafe {
            DisconnectNamedPipe(handle);
            CloseHandle(handle);
          }
          continue;
        }
      };

      let running = Arc::new(AtomicBool::new(true));
      let linux_sock = Arc::new(linux_sock);

      // Thread: Linux Socket -> Named Pipe
      let pipe_writer = handle as usize;
      let sock_reader = Arc::clone(&linux_sock);
      let running_t1 = Arc::clone(&running);
      let t1 = thread::spawn(move || {
        let mut buf = [0u8; BUF_SIZE];
        while running_t1.load(Ordering::Relaxed) {
          let n = sock_reader.read(&mut buf);
          if n <= 0 {
            break;
          }
          let mut written: u32 = 0;
          let ok = unsafe {
            WriteFile(
              pipe_writer as HANDLE,
              buf.as_ptr(),
              n as u32,
              &mut written,
              core::ptr::null_mut(),
            )
          };
          if ok == FALSE {
            break;
          }
        }
        running_t1.store(false, Ordering::Relaxed);
      });

      // Thread: Named Pipe -> Linux Socket (runs on this thread)
      let mut buf = [0u8; BUF_SIZE];
      while running.load(Ordering::Relaxed) {
        let mut read: u32 = 0;
        let ok = unsafe {
          ReadFile(
            handle,
            buf.as_mut_ptr(),
            BUF_SIZE as u32,
            &mut read,
            core::ptr::null_mut(),
          )
        };
        if ok == FALSE {
          let err = unsafe { GetLastError() };
          if err != ERROR_BROKEN_PIPE {
            eprintln!("[rsrpc-bridge] ReadFile error: {err}");
          }
          break;
        }
        if read == 0 {
          break;
        }
        if !linux_sock.write_all(&buf[..read as usize]) {
          break;
        }
      }

      running.store(false, Ordering::Relaxed);
      let _ = t1.join();

      unsafe {
        DisconnectNamedPipe(handle);
        CloseHandle(handle);
      }
      println!("[rsrpc-bridge] Session closed, waiting for next connection");
    }
  }
}

#[cfg(windows)]
fn main() {
  imp::run();
}
