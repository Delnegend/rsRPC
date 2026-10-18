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
    tracing_subscriber::fmt().init();
    tracing::error!("rsrpc-bridge is a Windows executable designed to run inside Wine/Proton.");
    tracing::error!(
        "To build for Wine/Proton: cargo build -p rsrpc-bridge --target x86_64-pc-windows-gnu"
    );
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, FALSE, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, TRUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{PIPE_ACCESS_DUPLEX, ReadFile, WriteFile};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    const BUF_SIZE: usize = 65536;

    #[repr(C)]
    struct SockAddrUn {
        sun_family: u16,
        sun_path: [u8; 108],
    }

    /// Issue an i386 Linux syscall through the `int 0x80` gate.
    ///
    /// `int 0x80` traps straight into the kernel's i386 compat entry, so the
    /// call reaches the kernel under Wine. The x86-64 `syscall` instruction is
    /// not usable here: Wine decodes `rax` as one of its own unix-call ids.
    /// The bridge therefore targets `i686-pc-windows-gnu`.
    ///
    /// `ebx` cannot be an inline-asm operand because LLVM reserves it for PIC,
    /// so it is saved around the trap and the arguments are loaded from
    /// `edx`/`ecx`/`edi` into `ebx`/`ecx`/`edx` inside the block.
    ///
    /// Every return value is a raw Linux `errno`: negative on failure.
    #[inline]
    unsafe fn syscall(num: usize, a1: usize, a2: usize, a3: usize) -> isize {
        let ret: isize;
        unsafe {
            core::arch::asm!(
                "push ebx",
                "mov ebx, edx",
                "mov edx, edi",
                "int 0x80",
                "pop ebx",
                inlateout("eax") num as isize => ret,
                in("edx") a1,
                in("ecx") a2,
                in("edi") a3,
                options(preserves_flags)
            );
        }
        ret
    }

    /// `socketcall(2)`: `eax` = `SYS_socketcall` (102), `ebx` = call number,
    /// `ecx` = pointer to the argument block.
    #[inline]
    unsafe fn socketcall(call: usize, args: *const usize) -> isize {
        let ret: isize;
        unsafe {
            core::arch::asm!(
                "push ebx",
                "mov ebx, edx",
                "int 0x80",
                "pop ebx",
                inlateout("eax") 102isize => ret,
                in("edx") call,
                in("ecx") args as usize,
                options(preserves_flags)
            );
        }
        ret
    }

    /// A connected Unix stream to the host rsRPC socket.
    struct LinuxSocket(i32);

    impl LinuxSocket {
        fn open_unix(path: &str) -> Option<Self> {
            let path_bytes = path.as_bytes();
            if path_bytes.len() >= 108 {
                return None;
            }

            // socketcall(SYS_SOCKET = 1, { AF_UNIX, SOCK_STREAM, 0 })
            let socket_args = [1usize, 1, 0];
            let fd = unsafe { socketcall(1, socket_args.as_ptr()) };
            if fd < 0 {
                return None;
            }
            let fd = fd as i32;

            let mut addr = SockAddrUn {
                sun_family: 1, // AF_UNIX
                sun_path: [0; 108],
            };
            addr.sun_path[..path_bytes.len()].copy_from_slice(path_bytes);

            // socketcall(SYS_CONNECT = 3, { fd, &addr, addrlen })
            let connect_args = [fd as usize, &addr as *const _ as usize, 110];
            let res = unsafe { socketcall(3, connect_args.as_ptr()) };
            if res != 0 {
                unsafe {
                    syscall(6, fd as usize, 0, 0); // SYS_close
                }
                tracing::debug!("connect({path}) failed: errno {}", -res);
                return None;
            }
            Some(Self(fd))
        }

        fn read(&self, buf: &mut [u8]) -> isize {
            unsafe { syscall(3, self.0 as usize, buf.as_mut_ptr() as usize, buf.len()) } // SYS_read
        }

        fn write_all(&self, mut buf: &[u8]) -> bool {
            while !buf.is_empty() {
                // SYS_write
                let n = unsafe { syscall(4, self.0 as usize, buf.as_ptr() as usize, buf.len()) };
                if n <= 0 {
                    return false;
                }
                buf = &buf[n as usize..];
            }
            true
        }

        fn close(&self) {
            unsafe {
                syscall(6, self.0 as usize, 0, 0); // SYS_close
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
                    // No existence probe: `GetFileAttributes` never reports a Unix socket
                    // under Wine, so a check would reject every candidate.
                    // `connect` is the only reliable probe.
                    if let Some(sock) = LinuxSocket::open_unix(candidate) {
                        tracing::info!("Connected to Linux socket: {candidate}");
                        return Some(sock);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn run() {
        tracing_subscriber::fmt().init();
        let pipe_name = to_wide(r"\\.\pipe\discord-ipc-0");
        tracing::info!("Starting Wine/Proton named pipe bridge...");

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
                tracing::error!("CreateNamedPipeW failed: {err}");
                thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }

            tracing::info!("Awaiting game connection on \\\\.\\pipe\\discord-ipc-0");
            let connected = unsafe { ConnectNamedPipe(handle, core::ptr::null_mut()) };
            let is_connected =
                connected == TRUE || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;

            if !is_connected {
                unsafe { CloseHandle(handle) };
                continue;
            }

            tracing::info!("Game client connected to named pipe");
            let linux_sock = match find_linux_socket() {
                Some(sock) => sock,
                None => {
                    tracing::error!("Failed to connect to host Linux Discord socket");
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
                        tracing::warn!("ReadFile error: {err}");
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
            tracing::info!("Session closed, waiting for next connection");
        }
    }
}

#[cfg(windows)]
fn main() {
    imp::run();
}
